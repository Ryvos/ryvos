# Graph executor

The graph executor is the mechanism the **[Director](../glossary.md#director)**
uses to turn a planned DAG into actual work. Where the Director's OODA
loop is the strategic layer — observe the goal, orient by generating a
graph, evaluate, diagnose, evolve — the executor is the tactical layer:
walk the graph, run each node as its own agent invocation, pass data
between nodes through a shared `HandoffContext`, and evaluate outgoing
edges to pick the next node. This document is the companion to
[director-ooda.md](director-ooda.md) and goes deeper on the mechanics of
the walker.

The relevant code lives in `crates/ryvos-agent/src/graph/`, split across
four files: `executor.rs` (the walker), `node.rs` (the node struct and
prompt builder), `edge.rs` (edges, condition variants, and the
expression evaluator), and `handoff.rs` (the shared context store). All
four are small — under 300 lines each — so this walkthrough covers them
mostly in full.

## GraphExecutor struct

`GraphExecutor` holds three fields: a hashmap of nodes keyed by id, a
vector of edges, and the entry node id. See
`crates/ryvos-agent/src/graph/executor.rs:49`:

```rust
pub struct GraphExecutor {
    nodes: HashMap<String, Node>,
    edges: Vec<Edge>,
    entry_node: String,
}
```

The executor is constructed once per run. It does not own the
`AgentRuntime` — that is passed into `execute()` by reference — because
the runtime is shared across the whole daemon and would otherwise have
to be cloned per-executor. Nodes are stored in a hashmap for O(1)
lookup during traversal; edges are stored as a vector because the scan
for "outgoing edges from node X" is already O(N) in the number of
edges and a hashmap keyed by `from` would not help when there are
typically 2-5 nodes with 2-5 edges per graph.

`GraphExecutor::new` at `crates/ryvos-agent/src/graph/executor.rs:59`
converts the node vector to the hashmap:

```rust
pub fn new(nodes: Vec<Node>, edges: Vec<Edge>, entry_node: impl Into<String>) -> Self {
    let node_map = nodes.into_iter().map(|n| (n.id.clone(), n)).collect();
    Self {
        nodes: node_map,
        edges,
        entry_node: entry_node.into(),
    }
}
```

The caller is responsible for ensuring `entry_node` is actually the id
of a node in the vector; the executor does not validate this at
construction. An invalid entry node produces a `Config` error on the
first iteration of `execute`, which is fine for a misuse that is
unlikely to reach production.

## Node

The `Node` struct at `crates/ryvos-agent/src/graph/node.rs:12` carries
all the configuration needed to run a single graph node as an agent
invocation:

```rust
pub struct Node {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub input_keys: Vec<String>,
    #[serde(default)]
    pub output_keys: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    #[serde(default)]
    pub goal: Option<Goal>,
    #[serde(default)]
    pub model: Option<ModelConfig>,
}
```

Field breakdown:

- `id` is the key used by edges and by the node hashmap; it must be
  unique within a graph.
- `name` is a human-readable label used in logs and in the web UI.
- `system_prompt` is the system message for this node's agent run. If
  unset, the executor passes `"Complete the task."` as a fallback.
- `input_keys` list the context keys the node wants to read. The
  executor looks these up in the `HandoffContext` and renders them
  into the prompt as a `## Context Data` block.
- `output_keys` list the keys the node writes to. After the agent run
  completes, the executor parses the output and stores it under these
  keys.
- `tools` is currently informational — the field is serialized but the
  current executor does not actually restrict the node's tool access
  based on it. Tool restriction is a planned feature.
- `max_turns` caps the node's inner ReAct loop; default 10, stricter
  than the top-level agent default of 25. A node is expected to be a
  focused subtask, not an open-ended conversation.
- `goal` is optional. When set, the executor runs the node via
  `run_with_goal` (which may route through the Director again, nested
  one level deep). When unset, it runs via plain `run`.
- `model` is an optional override; the field is present but the
  current executor does not plumb it through to the runtime. Per-node
  model override is a planned feature.

`Node::build_prompt` at `crates/ryvos-agent/src/graph/node.rs:97` is
the prompt renderer:

```rust
pub fn build_prompt(
    &self,
    base_prompt: &str,
    context_data: &std::collections::HashMap<String, serde_json::Value>,
) -> String {
    let mut prompt = String::new();

    if !self.input_keys.is_empty() {
        prompt.push_str("## Context Data\n\n");
        for key in &self.input_keys {
            if let Some(value) = context_data.get(key) {
                let display = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                prompt.push_str(&format!("**{}**: {}\n", key, display));
            }
        }
        prompt.push_str("\n---\n\n");
    }

    prompt.push_str(base_prompt);
    prompt
}
```

The logic is straightforward: if the node has input keys, prepend a
Markdown block with each requested key and its value. String values
are rendered as-is; all other JSON types are serialized via
`ToString`, which for numbers and booleans is fine and for arrays or
objects produces JSON-like text. Missing keys are silently omitted —
a node that asks for a key that is not in the context simply gets no
entry for that key, no error and no warning. This is deliberate: it
makes graphs resilient to the ordering of node execution and to
optional outputs from upstream nodes.

The builder methods at `crates/ryvos-agent/src/graph/node.rs:44-94`
are a standard builder pattern over `with_prompt`, `with_inputs`,
`with_outputs`, `with_goal`, `with_max_turns`, and `with_model`. They
are used by tests and by any Rust code that constructs graphs
programmatically; the Director constructs nodes via serde from the
LLM-generated JSON rather than via the builder, so the builder is
more of a test fixture than a production API.

## Edge

Edges connect nodes and carry a condition variant. See
`crates/ryvos-agent/src/graph/edge.rs:4`:

```rust
pub struct Edge {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub condition: EdgeCondition,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EdgeCondition {
    #[default]
    Always,
    OnSuccess,
    OnFailure,
    Conditional { expr: String },
    LlmDecide { prompt: String },
}
```

Five variants, each with distinct semantics:

- `Always` traverses unconditionally. This is the default when the
  JSON edge has no `condition` field, which is how the LLM-generated
  graphs usually look. A linear three-node chain with `Always` edges
  is the most common shape the Director produces.
- `OnSuccess` traverses only if the source node succeeded. "Succeeded"
  here means the agent run returned `Ok(_)`, not that a goal was
  satisfied — the distinction matters for nodes with goals attached,
  where a failed judge verdict still returns `Ok(_)` with best-effort
  output.
- `OnFailure` is the inverse: traverse only if the source node's
  agent run returned `Err(_)`. This is the edge kind the Director uses
  for fallback paths.
- `Conditional { expr }` evaluates a simple expression against the
  `HandoffContext` data. Supported operators are `==`, `!=`, and
  `contains`.
- `LlmDecide { prompt }` asks an LLM "given this context and this
  question, should we traverse this edge? Yes or no." The prompt is
  the question.

The `#[serde(tag = "type", rename_all = "snake_case")]` attribute
means the JSON wire format uses a discriminator field. An edge like
`{"from": "a", "to": "b", "condition": {"type": "on_success"}}` is
correctly parsed as `EdgeCondition::OnSuccess`, and a conditional is
`{"type": "conditional", "expr": "status == \"ok\""}`. This is
important because it is how the Director's LLM-generated graph JSON
round-trips into typed Rust.

## The condition expression evaluator

`evaluate_condition` at `crates/ryvos-agent/src/graph/edge.rs:84` is
the handwritten expression parser for `Conditional` edges:

```rust
pub fn evaluate_condition(
    expr: &str,
    context: &std::collections::HashMap<String, serde_json::Value>,
) -> bool {
    let expr = expr.trim();

    // key contains "value"
    if let Some((key, substr)) = parse_operator(expr, "contains") {
        return context
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.contains(substr));
    }

    // key != "value"
    if let Some((key, value)) = parse_operator(expr, "!=") {
        return context
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| s != value);
    }

    // key == "value"
    if let Some((key, value)) = parse_operator(expr, "==") {
        return context
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| s == value);
    }

    false
}
```

Three operators, checked in a fixed order: `contains` first, then
`!=`, then `==`. The ordering matters because `!=` contains `=` as a
substring and a naive split on `=` would mis-parse `key != "value"`
as `key !` plus `"value"`. By checking for `!=` before `==`, the
correct operator is found first.

`parse_operator` at `crates/ryvos-agent/src/graph/edge.rs:118` is a
two-line `splitn` that trims the key and strips surrounding quotes
from the value. No escape handling, no nested quotes, no multi-word
keys. The parser is deliberately minimal because the expressions the
Director generates are always simple — `status == "success"` is the
canonical shape and the evaluator is tuned for it.

Failure modes degrade to `false`. An unparseable expression
(`"this is not valid"`) returns `false` and the edge does not fire.
A reference to a missing context key returns `false` and the edge
does not fire. A key whose value is not a string (say, a numeric
value stored via `set` instead of `set_str`) returns `false` because
`as_str()` on a non-string JSON value returns `None`. The effect is
that broken edge conditions fail closed — the executor moves on to
the next edge or terminates, rather than raising an error.

## LlmDecide edges

`LlmDecide` edges delegate the decision to a language model. See
`crates/ryvos-agent/src/graph/executor.rs:222`:

```rust
async fn evaluate_llm_edge(
    llm: &Arc<dyn LlmClient>,
    config: &ModelConfig,
    prompt: &str,
    context_data: &HashMap<String, serde_json::Value>,
) -> bool {
    let context_str = context_data
        .iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect::<Vec<_>>()
        .join("\n");

    let full_prompt = format!(
        "{}\n\nContext:\n{}\n\nRespond with ONLY \"yes\" or \"no\".",
        prompt, context_str
    );

    let messages = vec![ChatMessage::user(full_prompt)];

    match llm.chat_stream(config, messages, &[]).await {
        Ok(mut stream) => {
            let mut response = String::new();
            while let Some(delta) = stream.next().await {
                if let Ok(StreamDelta::TextDelta(text)) = delta {
                    response.push_str(&text);
                }
            }
            let answer = response.trim().to_lowercase();
            answer.contains("yes")
        }
        Err(e) => {
            warn!(error = %e, "LlmDecide edge evaluation failed, defaulting to no");
            false
        }
    }
}
```

Two design choices to note. First, the prompt includes the *entire*
context as a key-value dump, not a selected subset. This is a
simplification: `LlmDecide` edges are meant to be short questions like
"should we iterate again?" where the full context is useful framing,
and filtering to relevant keys would require the LLM to specify which
keys matter, which is more work than just passing everything. Second,
the yes/no detection uses substring `contains("yes")` rather than
exact match. A response of `"Yes, I think so"` counts as yes; a
response of `"No, I would not"` counts as no; a response of "yes
and no" counts as yes because `yes` comes first in the substring
search. This is good enough for the simple decisions the executor
uses `LlmDecide` for.

Failure defaults to `false`. An LLM call that errors out (network
blip, rate limit) produces a warning and a no. This is fail-safe for
the common use case of "should we take this retry edge?", where a
wrong no just ends the run and a wrong yes might loop forever.

## HandoffContext

`HandoffContext` is the shared data bag that threads through the
graph. See `crates/ryvos-agent/src/graph/handoff.rs:9`:

```rust
pub struct HandoffContext {
    data: HashMap<String, serde_json::Value>,
    #[serde(default)]
    version: u64,
}
```

Two fields: a string-keyed JSON value map and a version counter that
increments on every mutation. The version is present for debugging and
for the web UI's context viewer (which can show "what did the context
look like after node 2 ran?") but is not used by any traversal logic.

The mutation API is small:

- `set(key, value)` and `set_str(key, value)` — write a JSON value or
  a string.
- `merge(other)` — copy every key-value from another context.
- `ingest_output(output_keys, output_text)` — the core handoff
  primitive, covered below.

The read API is just `get(key)`, `get_str(key)`, `data()`, and
`version()`.

## ingest_output

`ingest_output` is how node outputs flow into the context. It tries
to be smart about JSON-structured outputs while falling back to plain
text when the output is not parseable. See
`crates/ryvos-agent/src/graph/handoff.rs:61`:

```rust
pub fn ingest_output(&mut self, output_keys: &[String], output_text: &str) {
    if output_keys.is_empty() {
        return;
    }

    // Try to parse the output as JSON for key extraction
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(output_text) {
        if let Some(obj) = json.as_object() {
            for key in output_keys {
                if let Some(val) = obj.get(key) {
                    self.data.insert(key.clone(), val.clone());
                }
            }
            self.version += 1;
            return;
        }
    }

    // Fallback: store the full output under each output key
    for key in output_keys {
        self.data.insert(
            key.clone(),
            serde_json::Value::String(output_text.to_string()),
        );
    }
    self.version += 1;
}
```

The two-path logic gives node authors flexibility. If a node wants
to emit structured output, it should produce JSON and the executor
will pull specific keys out. If a node is producing free text, the
full text gets stored under every output key the node declared. A
summarizer node with `output_keys = ["summary"]` that writes "The
deploy succeeded" puts `"summary" -> "The deploy succeeded"` in the
context. A structured extractor node with `output_keys = ["topic",
"score"]` that emits `{"topic": "Rust", "score": 9.5}` puts both
keys in the context with the correct types.

The fallback path is the safer default — if the node says "I write
into these keys" and its output is not JSON, the executor assumes the
output is the full text and mirrors it into every declared key. This
is wasteful when a node declares multiple output keys but produces
prose, but it prevents data loss in the common case of a node whose
LLM sometimes produces JSON and sometimes does not.

The empty-output-keys early return at the top means a node with no
declared outputs does not affect the context at all. This is the
"side-effect node" shape — a node that runs for its own effects
(writing a file, sending a Telegram message) without exporting any
structured data to downstream nodes.

## The execute loop

`GraphExecutor::execute` is the main walker. See
`crates/ryvos-agent/src/graph/executor.rs:72`:

```rust
pub async fn execute(
    &self,
    runtime: &AgentRuntime,
    initial_context: HandoffContext,
    llm: Option<&(Arc<dyn LlmClient>, ModelConfig)>,
) -> Result<ExecutionResult> {
    let start = Instant::now();
    let mut context = initial_context;
    let mut node_results = Vec::new();
    let mut current_node_id = self.entry_node.clone();
    let mut visited: Vec<String> = Vec::new();

    loop {
        // Prevent infinite loops
        if visited.iter().filter(|id| **id == current_node_id).count() > 5 {
            warn!(node_id = %current_node_id,
                  "Node visited more than 5 times, terminating graph");
            break;
        }
        visited.push(current_node_id.clone());

        let node = match self.nodes.get(&current_node_id) {
            Some(n) => n,
            None => {
                return Err(RyvosError::Config(format!(
                    "Node '{}' not found in graph",
                    current_node_id
                )));
            }
        };
        /* ... execute node ... */
    }
```

The visit counter is the cycle breaker. Every node id is appended to
`visited` on entry, and a node that has been visited more than five
times terminates the whole graph with a warning. Five is a
compromise: one-shot and two-shot loops are normal (a retry-on-failure
edge is the obvious pattern), but a node that runs five times on the
same graph is almost certainly stuck. The counter is per-node, not
per-graph, so a graph with two separate nodes that each visit once in
a loop is fine as long as neither individual node crosses the
threshold.

The node lookup returns a `Config` error if the id is missing. This
catches both the construction-time misuse of specifying a bad entry
node and the runtime misuse of an edge pointing to a nonexistent
node. Both are caller bugs.

The node-execution body is the heart of the loop. See
`crates/ryvos-agent/src/graph/executor.rs:105`:

```rust
info!(node_id = %node.id, node_name = %node.name, "Executing graph node");

// Build the prompt from context
let base_prompt = node
    .system_prompt
    .as_deref()
    .unwrap_or("Complete the task.");
let prompt = node.build_prompt(base_prompt, context.data());

// Execute node
let node_start = Instant::now();
let session = SessionId::new();
let result = if let Some(ref goal) = node.goal {
    runtime.run_with_goal(&session, &prompt, Some(goal)).await
} else {
    runtime.run(&session, &prompt).await
};

let elapsed_ms = node_start.elapsed().as_millis() as u64;
let (output, succeeded) = match result {
    Ok(text) => (text, true),
    Err(e) => {
        error!(node_id = %node.id, error = %e, "Graph node failed");
        (e.to_string(), false)
    }
};

// Ingest output into context
context.ingest_output(&node.output_keys, &output);

// Store a status key for conditional edges
context.set_str(
    format!("{}_status", node.id),
    if succeeded { "success" } else { "failure" },
);
```

Step by step:

1. Build the prompt by feeding the node's `system_prompt` (or the
   fallback) and the current context data into `Node::build_prompt`.
2. Create a *fresh* session id for the node's agent run. This is
   important: every node gets its own session, not a shared graph
   session. Each node's agent run is isolated from its siblings'
   history.
3. Dispatch to `run_with_goal` or `run` based on whether the node
   has a goal. The goal path is recursive — a node with a goal may
   itself invoke the Director, which may itself construct a
   GraphExecutor with its own nodes. There is no static recursion
   guard; the per-node visit counter is the only cycle protection.
4. Convert the result into `(output, succeeded)`. On error, the
   error message becomes the output — this is so downstream nodes
   can see the error text via context if they want to react to it,
   and so the final `ExecutionResult` carries something meaningful
   even in failure cases.
5. Call `ingest_output` with the node's declared `output_keys` and
   the output text. This is where the JSON-or-fallback split runs.
6. Write `{node_id}_status` to the context as `"success"` or
   `"failure"`. This is the convention that lets `Conditional` edges
   check node status — `research_status == "success"` is the
   standard shape.

The status-key convention is the glue between node execution and
edge evaluation. The `OnSuccess`/`OnFailure` edge variants check the
`succeeded` boolean directly, but `Conditional` edges read from the
context, so the status has to be in the context. Writing
`{node_id}_status` makes both paths work.

## Edge evaluation and next-node selection

After ingesting output, the executor finds outgoing edges and picks
the first one whose condition matches. See
`crates/ryvos-agent/src/graph/executor.rs:155`:

```rust
let outgoing: Vec<&Edge> = self
    .edges
    .iter()
    .filter(|e| e.from == current_node_id)
    .collect();

if outgoing.is_empty() {
    debug!(node_id = %current_node_id, "No outgoing edges, graph complete");
    break;
}

let mut next_node: Option<String> = None;

for edge in &outgoing {
    let matches = match &edge.condition {
        EdgeCondition::Always => true,
        EdgeCondition::OnSuccess => succeeded,
        EdgeCondition::OnFailure => !succeeded,
        EdgeCondition::Conditional { expr } => evaluate_condition(expr, context.data()),
        EdgeCondition::LlmDecide { prompt: decide_prompt } => {
            if let Some((llm_client, config)) = llm {
                evaluate_llm_edge(llm_client, config, decide_prompt, context.data())
                    .await
            } else {
                warn!("LlmDecide edge but no LLM configured, skipping");
                false
            }
        }
    };

    if matches {
        next_node = Some(edge.to.clone());
        break; // Take the first matching edge
    }
}
```

The "first matching edge wins" rule is important. If node A has two
outgoing edges, one `OnFailure` to B and one `Always` to C, the
order in which the edges appear in `self.edges` decides which edge
fires on failure. The order matters because the user (or the
Director's LLM) controls it through the edge list — putting the more
specific `OnFailure` edge before the catch-all `Always` ensures the
catch-all only fires when the specific edge does not match. This is
the conventional order anyway.

The `LlmDecide` branch requires the optional `llm` parameter to be
present. If not, it logs a warning and skips the edge as if the
condition were false. Graphs that use `LlmDecide` must be executed
with the `llm` parameter supplied; plain graphs without
`LlmDecide` edges can be executed with `None`.

If no edge matches, `next_node` stays `None` and the loop exits at
the match below:

```rust
match next_node {
    Some(next) => {
        current_node_id = next;
    }
    None => {
        debug!(node_id = %current_node_id,
               "No edge conditions matched, graph complete");
        break;
    }
}
```

A node with outgoing edges that none match terminates the graph.
This is the "dead end" termination and the debug log makes it
visible for troubleshooting.

## ExecutionResult

At termination, the executor returns an `ExecutionResult`. See
`crates/ryvos-agent/src/graph/executor.rs:32`:

```rust
pub struct ExecutionResult {
    pub node_results: Vec<NodeResult>,
    pub context: HandoffContext,
    pub total_elapsed_ms: u64,
    pub succeeded: bool,
}
```

Four fields. `node_results` is the in-order history of every node
execution — a node visited multiple times appears multiple times.
`context` is the final state of the handoff bag, including all the
per-node status keys and all the ingested outputs. `total_elapsed_ms`
is the wall-clock time of `execute()` from start to return.
`succeeded` is the AND of every node's individual `succeeded` flag.

`NodeResult` at `crates/ryvos-agent/src/graph/executor.rs:19` is a
single node's record:

```rust
pub struct NodeResult {
    pub node_id: String,
    pub output: String,
    pub succeeded: bool,
    pub elapsed_ms: u64,
}
```

The Director uses `ExecutionResult` to extract the final output,
diagnose failures, and decide whether to evolve and retry. See
[director-ooda.md](director-ooda.md) for how the result is consumed
at the OODA level.

The convention is that the *last* node in the graph writes to a
`final_output` key in the context, and the Director reads
`context.get_str("final_output")` to get the run's string result. If
no node writes `final_output`, the Director falls back to the last
node's `output` field. This is a convention enforced by the
Director's graph-generation prompt (which instructs the LLM to
produce a `final_output` key) rather than by the executor, which
does not know about any specific key names.

## Cross-references

- [director-ooda.md](director-ooda.md) — the OODA loop that owns the
  executor and consumes its results.
- [agent-loop.md](agent-loop.md) — `runtime.run` and
  `runtime.run_with_goal`, the per-node execution path.
- [judge.md](judge.md) — what happens inside `run_with_goal` when a
  node carries a goal.
- [../crates/ryvos-agent.md](../crates/ryvos-agent.md) — crate
  overview listing the graph submodule alongside the Director.
- [../crates/ryvos-core.md](../crates/ryvos-core.md) — the `Goal` and
  `ModelConfig` types the graph module references.
