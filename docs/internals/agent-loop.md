# Agent loop

Every interaction with Ryvos lands in `AgentRuntime::run`. Whether the prompt
came from a Telegram message, a TUI keystroke, a scheduled cron job, or an
MCP tool invocation, the session manager routes it through the same entry
point and the same state machine. This document walks that state machine
line by line across the 1246-line `crates/ryvos-agent/src/agent_loop.rs`.
It is the technical counterpart to the narrative view in
[../architecture/execution-model.md](../architecture/execution-model.md) and
the structural view in [../crates/ryvos-agent.md](../crates/ryvos-agent.md).

The **[agent runtime](../glossary.md#agent-runtime)** is a single struct
(`AgentRuntime`) with three public entry points — `run`, `run_with_goal`,
and the internal `run_with_director` — and a per-**[turn](../glossary.md#turn)**
loop that runs up to `max_turns` times (default 25) before giving up. Between
turns, the loop drains hints from the **[Guardian](../glossary.md#guardian)**,
rebuilds context, prunes messages to the token budget, and persists a
**[checkpoint](../glossary.md#checkpoint)** for crash recovery.

## Entry points

The three entry points form a chain. See
`crates/ryvos-agent/src/agent_loop.rs:226`:

```rust
pub async fn run(&self, session_id: &SessionId, user_message: &str) -> Result<String> {
    self.run_with_goal(session_id, user_message, None).await
}

pub async fn run_with_goal(
    &self,
    session_id: &SessionId,
    user_message: &str,
    goal: Option<&Goal>,
) -> Result<String> {
    // Director delegation: if enabled and a goal is provided, use Director orchestration
    if let (Some(goal), Some(director_cfg)) = (goal, self.config.agent.director.as_ref()) {
        if director_cfg.enabled {
            return self.run_with_director(session_id, user_message, goal).await;
        }
    }
    // ... standard ReAct loop below ...
}
```

`run` is the common case: no goal, no **[Director](../glossary.md#director)**,
just a user message and the ReAct loop. `run_with_goal` adds a **[goal](../glossary.md#goal)**
that the **[Judge](../glossary.md#judge)** will evaluate after the model stops
emitting tool calls. The Director delegation at the top of `run_with_goal`
is the fork in the road: if the config enables the Director *and* the call
carries a goal, control moves to `run_with_director`, which constructs a
`Director` and hands it the goal object, the runtime reference, and the
session id. Otherwise the standard loop runs.

The rest of this document covers the standard loop. The Director path is
documented in [director-ooda.md](director-ooda.md).

## Phase 1: setup and guards

The standard loop opens at
`crates/ryvos-agent/src/agent_loop.rs:246` with four setup steps:

```rust
let start = Instant::now();
let max_turns = self.config.agent.max_turns;
let max_duration = Duration::from_secs(self.config.agent.max_duration_secs);

// Apply CLI session ID override to model config for --resume
let mut model_config = self.config.model.clone();
if let Some(cli_id) = self.cli_session_override.lock().unwrap().take() {
    info!(cli_session = %cli_id, "Applying CLI session override for --resume");
    model_config.cli_session_id = Some(cli_id);
}

// Clear last message ID before starting
*self.last_message_id.lock().unwrap() = None;

self.event_bus.publish(AgentEvent::RunStarted {
    session_id: session_id.clone(),
});
```

Four things are set up here:

1. **Timing.** `start` is captured once so the per-turn duration check has a
   stable reference. `max_turns` and `max_duration` come from config.
2. **CLI session override.** For **[CLI providers](../glossary.md#cli-provider)**
   (`claude-code`, `copilot`), runs can resume an existing upstream session
   via `--resume`. `cli_session_override` is a one-shot `Option<String>` set
   before the call — `.take()` clears it so it does not apply to the next
   run. The override is spliced into a cloned `ModelConfig` so the runtime's
   shared config stays untouched.
3. **last_message_id reset.** This field captures the `MessageId` delta
   emitted by CLI providers (the upstream session id for the next resume).
   Clearing it at run start prevents stale ids from leaking into a new run.
4. **RunStarted event.** The **[EventBus](../glossary.md#eventbus)** is
   notified. Every subscriber — Guardian, audit writer, run log, gateway —
   learns that a run has begun.

`RunStarted` is the first signal the Guardian uses to set its `run_active`
flag and reset the stall clock, so it must be published before the first
LLM call.

## Phase 2: context assembly

Context assembly is the next block, starting around
`crates/ryvos-agent/src/agent_loop.rs:264`. It has three parts: resolve the
system prompt, load optional extended-context sources, and build the
three-layer **[onion context](../glossary.md#onion-context)**.

```rust
let workspace = self.config.workspace_dir();
let prompt_override = self
    .config
    .agent
    .system_prompt
    .as_deref()
    .map(|spec| context::resolve_system_prompt(spec, &workspace));

// Load Viking sustained context (Layer 2.5 Recall)
let mut extended = context::ExtendedContext::default();
if let Some(ref vc) = *self.viking_client.lock().await {
    let query_hint = user_message;
    let policy = ryvos_memory::viking::ContextLevelPolicy::default();
    let viking_ctx = ryvos_memory::viking::load_viking_context(vc, query_hint, &policy).await;
    if !viking_ctx.is_empty() {
        extended.viking_context = viking_ctx;
    }
}
```

`resolve_system_prompt` supports a `file:` prefix — `file:prompts/main.md`
reads the file from the workspace (or from an absolute path) and uses its
contents as the system prompt, falling back to the spec string if the read
fails. A literal string without `file:` is used as-is. This is how operators
override the default system prompt without recompiling.

The **[Viking](../glossary.md#viking)** block loads hierarchical memory
fragments relevant to the current user message. `load_viking_context`
takes the user message as a free-text query, runs it against the
`viking.db` FTS5 index under a `ContextLevelPolicy` (which picks L0/L1/L2
tiers based on token budget), and returns a pre-rendered markdown block
that the onion builder will splice into the narrative layer.

The safety-memory block comes next. See
`crates/ryvos-agent/src/agent_loop.rs:290`:

```rust
if let Some(ref sm) = self.safety_memory {
    let tool_names: Vec<String> = if let Some(ref gate) = self.gate {
        gate.definitions().await.iter().map(|t| t.name.clone()).collect()
    } else {
        self.tools.read().await.definitions().iter().map(|t| t.name.clone()).collect()
    };
    let safety_ctx = sm.format_for_context(&tool_names, 5).await;
    if !safety_ctx.is_empty() {
        extended.safety_context = safety_ctx;
    }
}
```

The runtime collects tool names from the **[security gate](../glossary.md#security-gate)**
(if present) or directly from the tool registry, then asks
**[SafetyMemory](../glossary.md#safetymemory)** for relevant lessons across
those tools. The top five lessons (by confidence and reinforcement count)
are rendered as a markdown block and stored on `extended.safety_context`.
Both the Viking and safety blocks are empty strings if nothing was found,
and the builder silently omits empty blocks from the final prompt. See
[safety-memory.md](safety-memory.md) for the lesson retrieval details.

With Viking and safety lessons loaded, the runtime builds the system
message:

```rust
let system_msg = if goal.is_some() {
    context::build_goal_context_extended(&workspace, prompt_override.as_deref(), goal, &extended)
} else {
    context::build_default_context_extended(&workspace, prompt_override.as_deref(), &extended)
};
```

The two builders differ in that the goal variant adds the focus-layer goal
block; otherwise they are the same. Both assemble the identity, narrative,
and focus layers and return a single `ChatMessage` with role `System`. The
full composition is documented in
[../architecture/context-composition.md](../architecture/context-composition.md).

## Phase 3: run id and cost tracking

Before loading history, the runtime generates a UUID `run_id` and records
the run in the cost store. See
`crates/ryvos-agent/src/agent_loop.rs:332`:

```rust
let run_id = uuid::Uuid::new_v4().to_string();

if let Some(ref cost_store) = self.cost_store {
    let billing_type = if self.config.model.provider == "claude-code"
        || self.config.model.provider == "claude-cli"
        || self.config.model.provider == "claude-sub"
    {
        ryvos_llm::providers::claude_code::ClaudeCodeClient::detect_billing_type(&self.config.model)
    } else if self.config.model.provider == "copilot"
        || self.config.model.provider == "github-copilot"
        || self.config.model.provider == "copilot-cli"
    {
        BillingType::Subscription
    } else {
        BillingType::Api
    };
    if let Err(e) = cost_store.record_run(&run_id, &session_id.0, /* ... */, billing_type) {
        warn!(error = %e, "Failed to record run start");
    }
}
```

The `run_id` is distinct from the `SessionId` — sessions outlive runs, and
the same session can contain many runs (one per user message). The run id
is what the checkpoint store uses to scope its per-run snapshots and what
the cost store uses to track per-run billing.

**[Billing type](../glossary.md#billing-type)** detection is a three-way
match. The `claude-code` provider is tricky because it can be billed
either way (api or subscription) depending on whether the user supplied an
`api_key`; the detection delegates to
`ClaudeCodeClient::detect_billing_type`. `copilot` is always
**[subscription-billed](../glossary.md#subscription-billing)**. Everything
else is **[API-billed](../glossary.md#api-billing)**. The detection runs
once per run, before any token counts exist, so subscription-billed runs
can be recorded as `Subscription` from the start and appear with `$0.00`
in reports.

## Phase 4: history load, user append, memory flush

Next, the runtime loads recent history, appends the user message, and
optionally triggers a memory flush if the context is nearing its budget.
See `crates/ryvos-agent/src/agent_loop.rs:363`:

```rust
let mut messages = vec![system_msg];
let history = self.store.load_history(session_id, 100).await?;
messages.extend(history);

let user_msg = ChatMessage::user(user_message);
self.store
    .append_messages(session_id, std::slice::from_ref(&user_msg))
    .await?;
messages.push(user_msg);
```

History comes from the `SessionStore` (backed by `sessions.db`) with a
hard cap of 100 messages. The cap is defensive — if the session has grown
beyond that, the pruner below will compact it further, but at least the
initial load cannot balloon unbounded. The user message is appended to
both the in-memory message list and the persistent store in one step, so
a crash between here and the first LLM call still leaves the user message
recorded.

The memory flush is the interesting bit. See
`crates/ryvos-agent/src/agent_loop.rs:377`:

```rust
let budget = self.config.agent.max_context_tokens;

let flush_disabled = self.config.agent.disable_memory_flush.unwrap_or(false);
if !flush_disabled {
    let total_tokens: usize = messages.iter().map(crate::intelligence::estimate_message_tokens).sum();
    let flush_threshold = (budget as f64 * 0.85) as usize;
    if total_tokens > flush_threshold {
        info!(total_tokens, flush_threshold, "Running memory flush before compaction");
        messages.push(memory_flush_prompt());
        // ... run a mini-turn that lets the agent call memory tools ...
    }
}
```

The flush fires when the assembled message list (system + history + user)
crosses 85% of the token budget. Its purpose is to let the agent persist
important facts to durable storage (`memory_write`, `daily_log_write`, or
Viking writes) *before* the pruner compacts old messages and loses them.
The flush prompt — from `memory_flush_prompt()` in `intelligence.rs` —
explicitly names the memory files and Viking URIs the agent should write
to and asks it to respond with `FLUSH_COMPLETE` when finished. The flush
message is marked `phase: Some("memory_flush")` and `protected: true` so
the pruner does not touch it.

The mini-turn runs one LLM stream, accumulates deltas, executes any
memory-related tool calls directly (no Judge, no Guardian hints, no
tracker), checks for `FLUSH_COMPLETE` via `is_flush_complete`, and then
removes all messages tagged with the `memory_flush` phase so the main
loop does not see them as history. It is a sidestep, not a real turn: it
does not count against `max_turns` and does not update cumulative token
counters.

## Phase 5: pre-turn pruning

After the optional flush, the message list is trimmed to the token budget
before the first real LLM call. See
`crates/ryvos-agent/src/agent_loop.rs:457`:

```rust
if self.config.agent.enable_summarization {
    let pruned = summarize_and_prune(&mut messages, budget, 6, &*self.llm, &model_config).await?;
    if pruned > 0 { info!(pruned, "Summarized and pruned messages to fit context budget"); }
} else {
    let pruned = prune_to_budget(&mut messages, budget, 6);
    if pruned > 0 { info!(pruned, "Pruned messages to fit context budget"); }
}
```

Both paths have the same contract: remove oldest non-protected messages
until the total token count is under `budget`, keeping the system message
(index 0) and the last `min_tail = 6` messages untouched. The difference
is whether the removed messages are dropped or replaced with an LLM-
generated summary. `summarize_and_prune` is the more expensive path — it
runs a full `llm.chat_stream` call to compose a summary before dropping
the originals — and is off by default.

`min_tail = 6` is a compromise between keeping enough recent context for
the LLM to follow the conversation and not letting the tail grow
unbounded. Six messages is enough to cover two ReAct rounds (user, tool
use, tool result, assistant, tool use, tool result) without eating the
budget.

## Phase 6: tool context

The `ToolContext` that will be threaded through every tool call is built
next. See `crates/ryvos-agent/src/agent_loop.rs:473`:

```rust
let tool_defs = self.tool_definitions().await;
let max_output_tokens = self.config.agent.max_tool_output_tokens;
let vc = self.viking_client.lock().await.clone();
let tool_ctx = ToolContext {
    session_id: session_id.clone(),
    working_dir: std::env::current_dir().unwrap_or_else(|_| workspace.clone()),
    store: Some(self.store.clone()),
    agent_spawner: self.spawner.lock().await.clone(),
    sandbox_config: self.config.agent.sandbox.clone(),
    config_path: None,
    viking_client: vc.map(|c| Arc::new(c) as Arc<dyn std::any::Any + Send + Sync>),
};
```

Every field is threaded through to every tool invocation in the turn. The
working directory falls back to the workspace if `current_dir` fails
(which can happen inside a sandbox). The `agent_spawner` is the
`AgentRuntime` itself via the `AgentSpawner` trait — this is how the
`spawn_agent` tool constructs sub-agents without a direct reference to
the runtime. The `sandbox_config` is passed through so tools that care
about Docker sandboxing can read it, and the Viking client is type-erased
into an `Arc<dyn Any>` because `ToolContext` is defined in `ryvos-core`
and cannot depend on `ryvos-memory`.

## The per-turn loop

The heart of the file is the `for turn in 0..max_turns` loop that starts
at `crates/ryvos-agent/src/agent_loop.rs:492`. Each iteration is a full
ReAct turn: check cancellation and timeout, drain Guardian hints, stream
from the LLM, accumulate deltas, execute any tool calls, and check for a
stop condition.

### Turn start: cancellation, timeout, Guardian hints

```rust
for turn in 0..max_turns {
    if self.cancel.is_cancelled() {
        return Err(RyvosError::Cancelled);
    }
    if start.elapsed() > max_duration {
        return Err(RyvosError::MaxDurationExceeded(self.config.agent.max_duration_secs));
    }
    if let Some(ref hints_rx) = self.guardian_hints {
        let mut rx = hints_rx.lock().await;
        while let Ok(action) = rx.try_recv() {
            match action {
                GuardianAction::InjectHint(hint) => {
                    messages.push(ChatMessage::user(&hint));
                }
                GuardianAction::CancelRun(_) => {
                    return Err(RyvosError::Cancelled);
                }
            }
        }
    }
    // ... stream from LLM ...
}
```

Three checks run at the top of each iteration in strict order. Cancellation
comes first because it is the cheapest and the most decisive — if the user
stopped the run, nothing else matters. The timeout check enforces
`max_duration_secs` at turn granularity, so a run that has spent too long
in the current loop returns `RyvosError::MaxDurationExceeded` before
starting another LLM call.

The Guardian hint drain is a `while let Ok(action) = rx.try_recv()` — it
reads every queued action non-blockingly and processes each. `InjectHint`
adds the hint as a user message (so the LLM sees it as the next input).
`CancelRun` returns `RyvosError::Cancelled` immediately, even if the
`CancellationToken` has not fired yet. The double-check is deliberate:
the Guardian also calls `self.cancel.cancel()` on hard budget violations,
so the cancellation check at the top of the next iteration is backup if
the hint was already drained.

### Streaming from the LLM

Once hints are drained, the runtime calls `llm.chat_stream` and awaits
the first delta. See `crates/ryvos-agent/src/agent_loop.rs:523`:

```rust
let stream_result = tokio::select! {
    result = self.llm.chat_stream(&model_config, messages.clone(), &tool_defs) => result,
    _ = self.cancel.cancelled() => return Err(RyvosError::Cancelled),
};
let mut stream = stream_result?;
```

The `tokio::select!` lets the cancellation token preempt the stream setup
— important because some LLM clients (particularly HTTP ones) can spend
measurable time in connection setup before the first byte arrives. A
cancellation during setup returns immediately rather than waiting for the
stream to come up just so it can be thrown away.

### Delta accumulation

The delta loop is where the LLM's output is stitched into a complete
assistant message. See the main match at
`crates/ryvos-agent/src/agent_loop.rs:537`:

```rust
while let Some(delta) = stream.next().await {
    if self.cancel.is_cancelled() {
        return Err(RyvosError::Cancelled);
    }
    match delta? {
        StreamDelta::TextDelta(text) => {
            self.event_bus.publish(AgentEvent::TextDelta(text.clone()));
            text_content.push_str(&text);
        }
        StreamDelta::ThinkingDelta(text) => { thinking_content.push_str(&text); }
        StreamDelta::ToolUseStart { index, id, name } => {
            while tool_calls.len() <= index { tool_calls.push(ToolCallAccumulator::default()); }
            tool_calls[index].id = id;
            tool_calls[index].name = name;
        }
        StreamDelta::ToolInputDelta { index, delta } => {
            if let Some(tc) = tool_calls.get_mut(index) { tc.input_json.push_str(&delta); }
        }
        StreamDelta::Stop(reason) => { stop_reason = Some(reason); }
        StreamDelta::Usage { input_tokens, output_tokens } => {
            total_input_tokens += input_tokens;
            total_output_tokens += output_tokens;
            self.event_bus.publish(AgentEvent::UsageUpdate { input_tokens, output_tokens });
        }
        StreamDelta::MessageId(id) => {
            *self.last_message_id.lock().unwrap() = Some(id.clone());
            model_config.cli_session_id = Some(id);
        }
        // CliToolExecuted / CliToolResult handled separately — see below
    }
}
```

Seven delta types are handled:

- **`TextDelta`** — the visible text stream. Republished on the EventBus
  so the UI can show it live; also appended to `text_content`.
- **`ThinkingDelta`** — extended thinking tokens (Claude and friends).
  Appended to `thinking_content` but *not* republished to the bus; the
  gateway exposes a separate subscription for thinking streams.
- **`ToolUseStart { index, id, name }`** — a new tool call begins.
  The accumulator vec is resized to hold this index and the fields are
  set.
- **`ToolInputDelta { index, delta }`** — JSON fragments for the tool
  call's input, appended to the accumulator at `index`. JSON is
  accumulated as a raw string and parsed only when `Stop(ToolUse)`
  arrives.
- **`Stop(reason)`** — the LLM signals it is done. The `reason` is one of
  `EndTurn`, `StopSequence`, `ToolUse`, or `MaxTokens`, and it drives the
  stop-condition logic below.
- **`Usage`** — cumulative token counts for this stream. Added to the
  running totals and republished so the Guardian can run its budget
  checks.
- **`MessageId`** — the upstream session id emitted by CLI providers.
  Captured in `last_message_id` (for the daemon's own resume tracking)
  and stitched into `model_config` (so the next turn in the same run
  targets the same upstream session).

### Thinking-only fallback

Some models produce thinking tokens but no visible text before stopping —
this is common with Qwen 3.5 and DeepSeek-R1 served through OpenAI-
compatible APIs. Without a fallback, the user sees nothing. See
`crates/ryvos-agent/src/agent_loop.rs:762`:

```rust
if text_content.is_empty() && !thinking_content.is_empty() && tool_calls.is_empty() {
    text_content = thinking_content.clone();
}
```

The rule is narrow: no visible text, some thinking, and no tool calls.
Under those three conditions, the thinking becomes the response. The
thinking block is still stored separately in the assistant message for
provenance; the fallback just ensures the user has something to read.

### Building the assistant message

Once the stream closes and the fallback (if any) has run, the runtime
assembles the assistant message and persists it. See
`crates/ryvos-agent/src/agent_loop.rs:767`:

```rust
let mut content_blocks = Vec::new();
if !thinking_content.is_empty() {
    content_blocks.push(ContentBlock::Thinking { thinking: thinking_content });
}
if !text_content.is_empty() {
    content_blocks.push(ContentBlock::Text { text: text_content.clone() });
}
for tc in &tool_calls {
    let input: serde_json::Value =
        serde_json::from_str(&tc.input_json).unwrap_or(serde_json::Value::Null);
    content_blocks.push(ContentBlock::ToolUse {
        id: tc.id.clone(),
        name: tc.name.clone(),
        input,
    });
}

let assistant_msg = ChatMessage {
    role: Role::Assistant,
    content: content_blocks,
    timestamp: Some(chrono::Utc::now()),
    metadata: None,
};

self.store.append_messages(session_id, std::slice::from_ref(&assistant_msg)).await?;
messages.push(assistant_msg);
self.event_bus.publish(AgentEvent::TurnComplete { turn });
```

Block order is deterministic: thinking first, then text, then tool calls.
Tool call inputs are parsed from the accumulated JSON string at this
point; a parse failure falls back to `serde_json::Value::Null` rather
than crashing the turn, so a malformed tool call still produces a
well-typed message. The new message is written to the session store
*and* appended to the in-memory list. The bus gets `TurnComplete` so
subscribers (particularly the Guardian, which uses it to refresh
`last_progress`) know the turn is done.

## Stop condition handling

After the assistant message is built, the runtime inspects the stop
reason and the tool call list to decide whether to finish the run,
execute tools and loop, or return a truncated response. See
`crates/ryvos-agent/src/agent_loop.rs:803`:

```rust
let is_final_response = tool_calls.is_empty();
match stop_reason {
    Some(StopReason::EndTurn) | Some(StopReason::StopSequence) | None => {
        if is_final_response {
            // Apply heuristic output repair
            let repaired = OutputCleaner::heuristic_repair(&text_content);
            final_text = repaired;
            // ... Judge evaluation if goal provided ...
            // ... delete checkpoint, publish RunComplete, record cost ...
            return Ok(final_text);
        }
    }
    Some(StopReason::MaxTokens) => {
        warn!("LLM hit max tokens");
        if is_final_response {
            final_text = OutputCleaner::heuristic_repair(&text_content);
            // ... publish RunComplete, record cost ...
            return Ok(final_text);
        }
    }
    Some(StopReason::ToolUse) => {
        // Expected, execute tools below
    }
}
```

The branches map to four outcomes:

- **`EndTurn` or `StopSequence` with no tool calls.** The model is done.
  The text goes through `OutputCleaner::heuristic_repair` (which strips
  markdown fences, trims prose, and extracts JSON if the output looked
  like JSON), then the Judge runs if a goal is attached, and then the
  run ends.
- **`EndTurn` or `StopSequence` with tool calls.** The model stopped
  cleanly but still emitted tool calls. This is how most LLMs signal
  "I've decided, now run these". Execution continues below the match.
- **`MaxTokens` with no tool calls.** The model was cut off mid-response.
  The truncated text is repaired and returned as-is. Judge is not run,
  because the response is known to be incomplete.
- **`ToolUse`.** The expected case when the model wants to call tools.
  Control falls through to the tool execution block.

### Judge evaluation

When the run has a goal and the model stopped without tool calls, the
**[Judge](../glossary.md#judge)** evaluates the final text. See
`crates/ryvos-agent/src/agent_loop.rs:812`:

```rust
if let Some(goal) = goal {
    let judge = Judge::new(self.llm.clone(), self.config.model.clone());
    match judge.evaluate(&final_text, &messages, goal).await {
        Ok(verdict) => {
            self.event_bus.publish(AgentEvent::JudgeVerdict { /* ... */ });
            match &verdict {
                Verdict::Accept { confidence } => {
                    let results = goal.evaluate_deterministic(&final_text);
                    let eval = goal.compute_evaluation(results, vec![]);
                    self.event_bus.publish(AgentEvent::GoalEvaluated { /* ... */ });
                }
                Verdict::Retry { reason, hint } if turn + 1 < max_turns => {
                    let retry_msg = format!(
                        "The judge determined your response needs improvement: {}. Hint: {}",
                        reason, hint
                    );
                    messages.push(ChatMessage::user(&retry_msg));
                    continue;
                }
                Verdict::Escalate { reason } => {
                    warn!(reason = %reason, "Judge escalated — returning output as-is");
                }
                _ => {} // Continue or Retry on last turn
            }
        }
        Err(e) => { warn!(error = %e, "Judge evaluation failed, proceeding"); }
    }
}
```

Four **[verdicts](../glossary.md#verdict)** are possible. `Accept` ends the
run cleanly with the current output. `Retry` builds a user message
containing the judge's reason and hint, pushes it onto the messages list,
and `continue`s the turn loop — effectively a new turn where the agent
sees "the judge didn't like your answer, here's why". `Retry` only fires
while there are turns left; on the last turn it falls through to the
clean-exit path. `Escalate` returns the output as-is but logs a warning;
the intent is that the user is going to see a partial or unsatisfactory
answer and an escalation signal. `Continue` is a no-op — the judge has
no opinion yet.

A Judge evaluation error logs a warning and proceeds with the current
output; the goal is not treated as failed just because the Judge could
not run. The Judge itself is documented in [judge.md](judge.md) (planned).

### Clean exit bookkeeping

After the Judge (or in the no-goal branch), the runtime deletes the
checkpoint, publishes `RunComplete`, records the final cost, and returns
the text. See `crates/ryvos-agent/src/agent_loop.rs:851`:

```rust
if let Some(ref cp_store) = self.checkpoint_store {
    cp_store.delete_run(&session_id.0, &run_id).ok();
}

self.event_bus.publish(AgentEvent::RunComplete {
    session_id: session_id.clone(),
    total_turns: turn + 1,
    input_tokens: total_input_tokens,
    output_tokens: total_output_tokens,
});
if let Some(ref cost_store) = self.cost_store {
    let cost = ryvos_memory::estimate_cost_cents(/* ... */);
    if let Err(e) = cost_store.complete_run(
        &run_id, total_input_tokens, total_output_tokens,
        (turn + 1) as u64, cost, "complete",
    ) { warn!(error = %e, "Failed to record run completion"); }
}
return Ok(final_text);
```

The checkpoint delete is intentional: a run that completed successfully
does not need a resume record, and leaving stale checkpoints around makes
later resume decisions confusing. If the delete fails, it is logged and
ignored — the checkpoint store will reap it later or the next run for
the same session will overwrite it.

`RunComplete` is the signal the Guardian uses to reset its per-run state
(token counter, recent tools, stall clock). The cost store completion
call records the run's final metrics with a "complete" status; the error
path below writes an "error" status instead.

## Tool execution phase

When the stop reason is `ToolUse` (or `EndTurn` with tool calls present),
control falls through to the tool execution block. This is where the
security gate, parallel dispatch, failure tracking, and reflexion all
come together.

### Decision recording

Before executing tools, the runtime records a `Decision` for each tool
call in the FailureJournal. See
`crates/ryvos-agent/src/agent_loop.rs:931`:

```rust
let decision_ids: Vec<String> = tool_calls.iter().map(|tc| {
    let decision = Decision {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        session_id: session_id.0.clone(),
        turn,
        description: format!("Tool call: {}", tc.name),
        chosen_option: tc.name.clone(),
        alternatives: if tool_calls.len() > 1 {
            tool_calls.iter().filter(|other| other.id != tc.id)
                .map(|other| DecisionOption { name: other.name.clone(), confidence: None })
                .collect()
        } else { vec![] },
        outcome: None,
    };
    if let Some(ref journal) = self.journal {
        journal.record_decision(&decision).ok();
    }
    self.event_bus.publish(AgentEvent::DecisionMade { decision: decision.clone() });
    decision.id
}).collect();
```

The decision record captures *what the agent chose* and, implicitly,
*what it could have chosen instead* (every other tool in the same batch).
The outcome is left as `None` — the execution result is backfilled after
the tools run. The decision id list is kept in order so the backfill can
match results to decisions by index.

### Parallel vs serial dispatch

The runtime dispatches tool calls in parallel if `parallel_tools` is
enabled and there is more than one call, serially otherwise. See
`crates/ryvos-agent/src/agent_loop.rs:986`:

```rust
let tool_results: Vec<(String, String, ToolResult)> =
    if self.config.agent.parallel_tools && tool_calls.len() > 1 {
        let futs: Vec<_> = tool_calls.iter().zip(parsed_inputs.into_iter())
            .map(|(tc, input)| {
                let gate = self.gate.clone();
                let tools = Arc::clone(&self.tools);
                let ctx = tool_ctx.clone();
                let name = tc.name.clone();
                let id = tc.id.clone();
                async move {
                    let result = if let Some(gate) = gate {
                        gate.execute(&name, input, ctx).await
                    } else {
                        tools.read().await.execute(&name, input, ctx).await
                    };
                    let tool_result = match result {
                        Ok(r) => r,
                        Err(e) => {
                            error!(tool = %name, error = %e, "Tool execution failed");
                            ToolResult::error(e.to_string())
                        }
                    };
                    (name, id, tool_result)
                }
            }).collect();
        futures::future::join_all(futs).await
    } else {
        // Serial path — same logic, no spawn
        /* ... */
    };
```

Before dispatch, the runtime publishes a `ToolStart` event for every tool
in the batch so the Guardian sees the full batch at once for doom-loop
detection. The security gate's `execute` method is `&self`, so parallel
calls into the same gate are safe — each call owns its own approval wait,
safety classification, and audit write. Tools that are inherently serial
(e.g. `git_commit` against the same repo) are expected to handle
serialization internally; the runtime does not special-case them.

Errors are wrapped into `ToolResult::error` rather than propagated so a
single failing tool cannot abort the batch. The loop continues and
returns results for every tool, and the ReAct model gets to see the
errors in the next turn.

### Post-execution bookkeeping

After the batch returns, the runtime does four things per result: backfill
the decision outcome, compact the output, track success or failure, and
build a `ToolResult` content block. See
`crates/ryvos-agent/src/agent_loop.rs:1037`:

```rust
for (idx, (_name, _id, tool_result)) in tool_results.iter().enumerate() {
    if let (Some(ref journal), Some(dec_id)) = (&self.journal, decision_ids.get(idx)) {
        let outcome = DecisionOutcome {
            tokens_used: 0,
            latency_ms: tool_exec_elapsed_ms,
            succeeded: !tool_result.is_error,
        };
        journal.update_decision_outcome(dec_id, &outcome).ok();
    }
}

for (name, id, tool_result) in tool_results {
    let compacted_content = compact_tool_output(&tool_result.content, max_output_tokens);
    let compacted_result = ToolResult {
        content: compacted_content.clone(),
        is_error: tool_result.is_error,
    };
    self.event_bus.publish(AgentEvent::ToolEnd { name: name.clone(), result: compacted_result });
    // ... failure tracking + reflexion hint injection ...
    // ... build ToolResult content block ...
}
```

`compact_tool_output` truncates the output at a newline boundary to fit
within `max_tool_output_tokens * 4` characters — the factor-of-four
approximation is cheap and consistent with the cl100k_base tokenizer's
average bytes-per-token. Truncated outputs get a `[truncated]` marker
appended so the LLM knows the output was not complete.

Failure tracking is where **[Reflexion](../glossary.md#reflexion)** comes
in. See `crates/ryvos-agent/src/agent_loop.rs:1064`:

```rust
if tool_result.is_error {
    let count = failure_tracker.record_failure(&name);
    if let Some(ref journal) = self.journal {
        journal.record(FailureRecord { /* ... */ }).ok();
    }
    if count >= threshold {
        let past = self.journal.as_ref()
            .and_then(|j| j.find_patterns(&name, 5).ok())
            .unwrap_or_default();
        let hint = if past.is_empty() {
            reflexion_hint(&name, count)
        } else {
            reflexion_hint_with_history(&name, count, &past)
        };
        messages.push(hint);
    }
} else {
    failure_tracker.record_success(&name);
    if let Some(ref journal) = self.journal {
        journal.record_success(&session_id.0, &name).ok();
    }
}
```

The in-memory `FailureTracker` counts consecutive failures per tool name
in the current session; it resets on success. When the count crosses
`reflexion_failure_threshold` (default 3), the runtime pushes a reflexion
hint as a user message, effectively saying "this tool has failed N times,
here is what similar past failures looked like, try something else". The
hint is built by `reflexion_hint_with_history` if past patterns are
available from the failure journal (historical failures across sessions),
or by the simpler `reflexion_hint` otherwise.

Reflexion is purely advisory — the runtime never blocks a tool from
running again or substitutes a different tool. The hint is just another
message in the context, and the LLM decides what to do with it.

### Results message and re-pruning

Tool results are packaged into a single user-role message and appended to
the history. See `crates/ryvos-agent/src/agent_loop.rs:1117`:

```rust
let results_msg = ChatMessage {
    role: Role::User,
    content: tool_result_blocks,
    timestamp: Some(chrono::Utc::now()),
    metadata: Some(MessageMetadata {
        protected: true,
        ..Default::default()
    }),
};

self.store.append_messages(session_id, std::slice::from_ref(&results_msg)).await?;
messages.push(results_msg);

let pruned = prune_to_budget(&mut messages, budget, 6);
if pruned > 0 { debug!(pruned, "Re-pruned messages after tool execution"); }
```

Two details to highlight. First, the results message is marked
`protected: true` so the pruner will never remove it — tool results are
the context the LLM needs to continue, and dropping them mid-flight would
confuse it. Second, the post-execution prune is deterministic
(`prune_to_budget`, not the LLM-summarizing variant) because mid-loop is
not the right time to take an extra LLM round trip. If the summarizer is
configured, it runs at the start of the *next* run, not in the middle of
this one.

### Per-turn checkpoint

The last thing each turn does is save a checkpoint. See
`crates/ryvos-agent/src/agent_loop.rs:1139`:

```rust
if let Some(ref cp_store) = self.checkpoint_store {
    if let Ok(json) = CheckpointStore::serialize_messages(&messages) {
        let cp = crate::checkpoint::Checkpoint {
            session_id: session_id.0.clone(),
            run_id: run_id.clone(),
            turn,
            messages_json: json,
            total_input_tokens,
            total_output_tokens,
            timestamp: Utc::now(),
        };
        if let Err(e) = cp_store.save(&cp) {
            warn!(error = %e, "Failed to save checkpoint");
        }
    }
}
```

The checkpoint store overwrites the previous row for this run (it only
holds the latest turn, not the full history). If the daemon crashes here,
the next start will find a live checkpoint for this `(session_id, run_id)`
pair and the `--resume` flow will rehydrate messages from it. A
successful run deletes its checkpoint in the clean-exit branch above.

## CLI tool post-hoc safety

The two delta variants that have not been covered yet —
`CliToolExecuted` and `CliToolResult` — handle a special case. The
**[CLI providers](../glossary.md#cli-provider)** (`claude-code` and
`copilot`) run tools inside their own subprocess and stream out events
describing what they did. Ryvos cannot intercept these calls at
dispatch time (the security gate never sees them), so instead it
records them *post-hoc* into the same audit trail and safety memory
that intercepted calls use. This is what keeps security consistent
across providers.

See `crates/ryvos-agent/src/agent_loop.rs:581` for the
`CliToolExecuted` handler. The delta arrives with a tool name and an
input summary; the runtime publishes `ToolStart`, runs
`assess_outcome` on a synthesized input (`{"command": input_summary}`),
logs the result to the **[audit trail](../glossary.md#audit-trail)** if
a gate is present, and records a safety lesson if the assessment
came back as a `NearMiss`. It finishes by publishing `ToolEnd` with a
placeholder result (`"[executed by CLI provider]"`) so downstream UIs
see the call as closed.

The `CliToolResult` handler at
`crates/ryvos-agent/src/agent_loop.rs:668` runs in parallel against the
actual output string. It calls `assess_outcome` a second time, now with
the real output and the `is_error` flag from the CLI provider. A
non-harmless outcome is logged at `info` level, written to the audit
trail, and — if the outcome is `Incident` — recorded as a safety lesson
with confidence derived from severity. Errors are additionally wired
into the failure journal so the **[Reflexion](../glossary.md#reflexion)**
pipeline sees them.

The result is that Ryvos's safety posture does not degrade on CLI
providers, even though it cannot gate their tool calls. SafetyMemory
lessons accumulated from claude-code runs are still loaded into the
context of subsequent runs (regardless of provider), and the global
audit trail is a single source of truth for every tool the agent ever
ran.

## MaxTurnsExceeded

If the per-turn loop runs all the way to `max_turns` without returning,
the runtime records an error-status completion in the cost store and
returns a typed error. See `crates/ryvos-agent/src/agent_loop.rs:1185`:

```rust
if let Some(ref cost_store) = self.cost_store {
    let cost = ryvos_memory::estimate_cost_cents(/* ... */);
    if let Err(e) = cost_store.complete_run(
        &run_id, total_input_tokens, total_output_tokens,
        max_turns as u64, cost, "error",
    ) { warn!(error = %e, "Failed to record run error"); }
}
Err(RyvosError::MaxTurnsExceeded(max_turns))
```

`RyvosError::MaxTurnsExceeded` is returned rather than panicking; the
caller (channel adapter, TUI, gateway) is expected to format it as a
user-visible error. The checkpoint is *not* deleted on this path —
`max_turns` can be raised and the run resumed.

## Cancellation semantics

Cancellation in the agent loop is cooperative: every await point
observes `self.cancel` directly or indirectly. There are four explicit
check sites per turn:

1. Top of the turn loop (`is_cancelled` before starting the turn).
2. The `tokio::select!` around `llm.chat_stream` (cancellation preempts
   stream setup).
3. Inside the delta accumulation loop (checked before each delta's match
   processing).
4. The Guardian hint drain (`CancelRun` returns `RyvosError::Cancelled`).

Tool calls inside the batch are not checked explicitly — they are
expected to honor the `ToolContext` they were given and poll
`CancellationToken` via their own `tokio::select!` or `spawn_blocking`
pattern. Practically, most built-in tools are short enough that latency
to cancel is under a second.

On cancel, the runtime returns `RyvosError::Cancelled` immediately. The
checkpoint from the *previous* turn is still on disk (the cancel happens
before the current turn's checkpoint is written), so the run is
resumable. The cost store does not get a `complete_run` call on this
path; the open run row is cleaned up by the cost store's own reap logic
on the next daemon start.

## Where to go next

- [director-ooda.md](director-ooda.md) — the alternate execution model
  for runs with goals and the Director enabled.
- [guardian.md](guardian.md) — the watchdog whose hints the loop drains
  between turns and whose cancellation the loop honors.
- [safety-memory.md](safety-memory.md) — the store that powers the
  pre-run lesson load in Phase 2 and the post-execution classification
  inside the security gate.
- [judge.md](judge.md) — the Judge whose verdict gates the end-of-run
  clean exit (planned).
- [checkpoint-resume.md](checkpoint-resume.md) — the checkpoint store
  that saves per-turn snapshots and the resume flow that reloads them
  (planned).
- [../architecture/execution-model.md](../architecture/execution-model.md) —
  the high-level view that shows the agent loop alongside the Director,
  Guardian, and Heartbeat across a full run.
- [../architecture/context-composition.md](../architecture/context-composition.md) —
  the onion context assembly detailed separately from the runtime.
