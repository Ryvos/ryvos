# Tool registry

The **[tool registry](../glossary.md#tool-registry)** is the collection of
every tool the **[agent runtime](../glossary.md#agent-runtime)** can invoke
during a turn. It holds built-in tools (roughly 60, organized into a dozen
categories), user-installed **[skills](../glossary.md#skill)**, and tools
proxied from external **[MCP](../glossary.md#mcp)** servers — all behind a
single trait and a single lookup table. When the LLM emits a tool call, the
runtime's dispatcher resolves the name against the registry, executes the
tool through the **[security gate](../glossary.md#security-gate)**, and
returns the result to the next turn's message list.

This document walks the registry implementation in
`crates/ryvos-tools/src/registry.rs`, the `Tool` trait in
`crates/ryvos-core/src/traits.rs`, and the dispatcher in
`crates/ryvos-agent/src/gate.rs`. For the upstream crate reference, see
[../crates/ryvos-tools.md](../crates/ryvos-tools.md). For how to add a
built-in tool, see [../guides/adding-a-tool.md](../guides/adding-a-tool.md).
For the MCP bridge that injects external tools into this same registry,
see [mcp-bridge.md](mcp-bridge.md).

## The Tool trait

Every callable unit implements the `Tool` trait from
`crates/ryvos-core/src/traits.rs:21`:

```rust
pub trait Tool: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>>;

    fn timeout_secs(&self) -> u64 { 30 }
    fn requires_sandbox(&self) -> bool { false }
    fn tier(&self) -> crate::security::SecurityTier {
        crate::security::SecurityTier::T1
    }
}
```

Four methods are required. `name` is the identifier the LLM emits when it
wants to call the tool — it must be unique across the whole registry, and
it must be stable, because users and LLMs may hard-code names in prompts
and skills. `description` is the human-readable one-liner sent to the LLM
alongside the schema. `input_schema` is a JSON Schema value describing
the expected input object; the LLM uses this to generate well-formed
arguments. `execute` is the async entry point that receives the parsed
input value and a `ToolContext` handle to the rest of the runtime, and
returns a `ToolResult`.

Three methods are defaulted and rarely overridden. `timeout_secs` gives
the wall-clock budget the registry will allow for a single call; the
default is 30 seconds, which is enough for almost every tool that isn't
running a compiler or a long HTTP request. Tools that need longer (MCP
bridged tools, code-run tools, network tools, the agent spawner) override
this up to 300. `requires_sandbox` flags a tool as needing Docker or
Bubblewrap isolation; it is informational metadata the runtime can use to
decide whether to reach for the sandbox configuration in `ToolContext`.
`tier` returns the **[deprecated security tier](../glossary.md#t0-t4)**.
Under **[passthrough security](../glossary.md#passthrough-security)**
the tier no longer gates execution, but it survives as a hint for
auditing, Web UI badges, and **[SafetyMemory](../glossary.md#safetymemory)**
pattern matching.

The trait is `Send + Sync + 'static` so the registry can store tools
behind `Arc<dyn Tool>` and share them across tokio tasks without
lifetimes leaking into calling code. The `BoxFuture<'_, _>` return type
in `execute` is necessary because the workspace's MSRV does not yet
support async fn in traits for trait objects.

## ToolContext, ToolResult, ToolDefinition

Three small types support the trait. `ToolResult` at
`crates/ryvos-core/src/types.rs:236`:

```rust
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}
```

`content` is the string the LLM will see as the tool output. `is_error`
signals that the tool ran but failed semantically (a search returned
nothing, an API returned a 4xx, a command exited with a non-zero status);
it does not represent a Rust-level error, which is raised via the `Err`
arm of the returned `Result` and translated into a `RyvosError` variant
by the registry.

`ToolDefinition` at `crates/ryvos-core/src/types.rs:259` is the LLM-facing
view:

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}
```

This is what the agent runtime passes to `llm.chat_stream` as the list of
available tools. Each provider translates it into the provider-specific
tool-use format (Anthropic's `tools` array, OpenAI's `functions` array,
Gemini's `function_declarations`, and so on).

`ToolContext` at `crates/ryvos-core/src/types.rs:267` is the shared
runtime state threaded through every invocation:

```rust
pub struct ToolContext {
    pub session_id: SessionId,
    pub working_dir: std::path::PathBuf,
    pub store: Option<Arc<dyn crate::traits::SessionStore>>,
    pub agent_spawner: Option<Arc<dyn AgentSpawner>>,
    pub sandbox_config: Option<crate::config::SandboxConfig>,
    pub config_path: Option<std::path::PathBuf>,
    pub viking_client: Option<Arc<dyn std::any::Any + Send + Sync>>,
}
```

The session id is the **[session](../glossary.md#session)** scoping every
call — tools like `memory_write`, `session_history`, and Viking writes
use it to avoid cross-session leakage. The working directory is the
filesystem root for commands that touch the user's repo; in a sandboxed
run it is the mount point inside the container. The store is an optional
`Arc` of the main session store, exposed to tools that need to read
other sessions' history. `agent_spawner` is the `AgentRuntime` itself
reached through the `AgentSpawner` trait, which is how `spawn_agent`
constructs sub-agents without a direct dependency on `ryvos-agent`.
`sandbox_config` and `config_path` are optional; `viking_client` is
type-erased into `Arc<dyn Any>` because `ToolContext` lives in
`ryvos-core` and cannot name `ryvos-memory`'s `VikingClient` type.

The context is cloned cheaply — every field is either `Copy`, `Arc`, or
`Option` — so each tool call gets its own shallow copy.

## Registry storage and API

`ToolRegistry` at `crates/ryvos-tools/src/registry.rs:9` is a thin
wrapper around a hashmap:

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}
```

`register` at `crates/ryvos-tools/src/registry.rs:21` takes any
`impl Tool` (not a trait object), wraps it in an `Arc`, and inserts by
name. Re-registering the same name overwrites — a tested behavior that
skills and MCP refresh rely on. `unregister` at
`crates/ryvos-tools/src/registry.rs:27` removes by name and returns
`true` if the entry existed. `get` and `list` are trivial lookups.
`definitions` collects a `Vec<ToolDefinition>` by walking every entry
and calling the three read-only methods.

The registry is small, hot, and frequently consulted. In a running
daemon it is wrapped in `Arc<tokio::sync::RwLock<ToolRegistry>>`:
readers (every tool dispatch) take a shared lock; writers (MCP refresh,
skill reload) take an exclusive lock. The lock contention is dominated
by reads, and the writes are rare enough that the lock rarely serializes
real work.

## Built-in tool registration

`ToolRegistry::with_builtins` at
`crates/ryvos-tools/src/registry.rs:76` is the constructor used at
daemon startup. It creates an empty registry and calls `register` for
every built-in tool, organized into named groups:

```rust
pub fn with_builtins() -> Self {
    let mut registry = Self::new();

    // Original 12 tools
    registry.register(crate::builtin::bash::BashTool);
    registry.register(crate::builtin::read::ReadTool);
    registry.register(crate::builtin::write::WriteTool);
    registry.register(crate::builtin::edit::EditTool);
    registry.register(crate::builtin::memory_search::MemorySearchTool);
    // ...

    // Sessions (5)
    registry.register(crate::builtin::sessions::SessionListTool);
    // ...
}
```

The comments group the tools by category: the original 12 general-purpose
tools, 5 session management tools, 3 memory tools, 9 filesystem tools,
6 git tools, 4 code tools, 4 network tools, 5 system tools, 8 data
tools, 3 scheduling tools, 2 database tools, 1 notification tool, 5
browser tools (registered via a helper), and 4 Viking tools. The
totals add up to roughly 60 built-ins; the exact number drifts as tools
are added and removed. The registry is the source of truth; the
category comments are documentation only.

Several former categories — Gmail/Google Workspace, Notion, Jira, Linear
— are commented out with a "use MCP servers instead" note. Ryvos used
to ship bespoke adapters for each; they were removed in v0.7 because the
same functionality is now available through standard MCP servers that
ship with Claude Code, Copilot, and the MCP registry.

## The dispatch pipeline

There are two dispatchers in the codebase, because registrations can be
used in two modes.

`ToolRegistry::execute` at `crates/ryvos-tools/src/registry.rs:54` is
the low-level dispatcher. It is used directly by code that does not
need the security gate (mostly tests and the MCP server interface). The
flow:

```rust
pub async fn execute(
    &self,
    name: &str,
    input: serde_json::Value,
    ctx: ToolContext,
) -> Result<ToolResult> {
    let tool = self
        .get(name)
        .ok_or_else(|| RyvosError::ToolNotFound(name.to_string()))?;

    let timeout = std::time::Duration::from_secs(tool.timeout_secs());

    match tokio::time::timeout(timeout, tool.execute(input, ctx)).await {
        Ok(result) => result,
        Err(_) => Err(RyvosError::ToolTimeout {
            tool: name.to_string(),
            timeout_secs: tool.timeout_secs(),
        }),
    }
}
```

Lookup, timeout, execute, translate. If `get` returns `None`, a
`RyvosError::ToolNotFound` bubbles up — this is the error the LLM sees
as a tool result when it hallucinates a tool name. If the inner future
takes longer than `timeout_secs`, `tokio::time::timeout` cancels the
future and the registry produces a `RyvosError::ToolTimeout` with the
tool name and the configured budget, which the agent runtime later
translates into a tool result whose `content` describes the timeout.

## The security gate dispatcher

`SecurityGate::execute` at `crates/ryvos-agent/src/gate.rs:67` is the
dispatcher the agent runtime actually uses. It wraps
`ToolRegistry::execute` with audit, safety memory, and optional
soft-checkpoint handling. The high-level flow is:

1. Resolve the tool by name against the registry's read lock.
2. Summarize the input via `summarize_input` for audit and approval
   previews.
3. Fetch relevant lessons from SafetyMemory (top three by confidence)
   if the store is configured.
4. Compute a `safety_reasoning` string from `(has_side_effects, lesson_count)`.
5. If `policy.should_pause(name)` is true and the tool has side effects,
   publish an `ApprovalRequested` event and wait for a decision from
   any channel, with a configurable timeout.
6. Call `execute_tool_direct` to run the tool with the same
   timeout/result-translation logic as the registry.
7. Run `assess_outcome` on the result to classify it as
   `Harmless` / `NearMiss` / `Incident` / `UserCorrected`.
8. Log an `AuditEntry` describing the call, the outcome, and the
   safety reasoning to the **[audit trail](../glossary.md#audit-trail)**.
9. If the outcome is an `Incident`, record a new `SafetyLesson`;
   if it is a `NearMiss`, reinforce the lessons that matched in step 3.
10. Return the original `Result<ToolResult>` to the caller.

See `crates/ryvos-agent/src/gate.rs:67`:

```rust
pub async fn execute(
    &self,
    name: &str,
    input: serde_json::Value,
    ctx: ToolContext,
) -> Result<ToolResult> {
    let tool = {
        let tools = self.tools.read().await;
        tools
            .get(name)
            .ok_or_else(|| RyvosError::ToolNotFound(name.to_string()))?
    };

    // 1. Summarize input
    let input_summary = summarize_input(name, &input);

    // 2. Check safety memory (informational)
    let mut lesson_ids = Vec::new();
    if let Some(ref memory) = self.safety_memory {
        if let Ok(lessons) = memory.relevant_lessons(name, 3).await {
            if !lessons.is_empty() {
                lesson_ids = lessons.iter().map(|l| l.id.clone()).collect();
            }
        }
    }
    // ... (steps 3–10 follow)
}
```

The important invariant is that the gate never blocks based on
classification. The only way a tool call is stopped is an explicit
`Denied` response to a soft checkpoint; everything else — even a tool
flagged as "destructive" with lessons in safety memory — proceeds and
lets the outcome teach the system. This is the core of passthrough
security and is covered in full in
[../adr/002-passthrough-security.md](../adr/002-passthrough-security.md).

## Parallel execution

When the LLM emits more than one tool call in a single turn, the agent
runtime can execute them concurrently. See
`crates/ryvos-agent/src/agent_loop.rs:986`:

```rust
let tool_results: Vec<(String, String, ToolResult)> =
    if self.config.agent.parallel_tools && tool_calls.len() > 1 {
        let futs: Vec<_> = tool_calls
            .iter()
            .zip(parsed_inputs.into_iter())
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
                    (name, id, result.unwrap_or_else(|e| ToolResult::error(e.to_string())))
                }
            })
            .collect();
        futures::future::join_all(futs).await
    } else {
        // Serial fallback
        // ...
    };
```

`futures::future::join_all` drives every future to completion and
collects the results in order. Each call gets its own cloned
`ToolContext` and `Arc` to the gate, so they share nothing mutable;
the only contention is the RwLock on the registry itself, which is
held only for the brief `get` call at the top of `SecurityGate::execute`.

Parallel execution is opt-in via `config.agent.parallel_tools`
(defaulted on) because some combinations of tools do have ordering
dependencies: an `edit` followed by a `bash cargo build` only makes
sense serially. Tools with real ordering sensitivity should state that
in their description so the LLM knows not to batch them; tools that
commute (reads, searches, independent HTTP calls) get real wall-clock
speedups. The approval broker handles parallel approvals correctly by
giving each pending call its own `oneshot::Receiver`.

## How skills and MCP tools plug in

The `Tool` trait is deliberately minimal so that non-built-in providers
can implement it without knowing anything about the runtime. Two
adapters take advantage of this.

`SkillTool` in `crates/ryvos-skills/src/tool.rs` wraps a single loaded
skill manifest. The manifest declares the tool name, description, and
input schema in TOML; the body is a Lua or Rhai script that receives
the parsed input and returns a JSON value. The `execute` implementation
calls into the script runtime, catches any evaluation errors, and wraps
the return value as a `ToolResult`. Skills are registered into the
registry at daemon startup by walking `~/.ryvos/skills/` and calling
`registry.register(SkillTool::from_manifest(...))` for each valid
file. The registry is agnostic about where a tool came from — a
`SkillTool` is just another `Arc<dyn Tool>`.

`McpBridgedTool` in `crates/ryvos-mcp/src/bridge.rs:32` wraps a single
tool exposed by an external MCP server. Its name is
`mcp__{server}__{tool}` to avoid collisions with built-ins, and its
`execute` delegates to `McpClientManager::call_tool`. The
`register_mcp_tools` function walks an `McpClientManager`'s tool list
for a server and inserts one `McpBridgedTool` per entry. When a server
notifies `tools/list_changed`, `refresh_tools` unregisters every
`mcp__{server}__*` name and re-registers the new set. See
[mcp-bridge.md](mcp-bridge.md) for the full story.

Both adapters end up as ordinary entries in the same hashmap. The agent
runtime cannot distinguish them from built-ins; the LLM cannot; the
audit trail records them under their display names just like
everything else. The uniformity is deliberate: every future tool
provider (Deno scripts, WASM modules, HTTP webhooks) can be dropped in
by implementing `Tool`, and the rest of the system does not need to
change.

## Tool definitions and LLM negotiation

At the top of each turn, the agent runtime calls
`tool_definitions()` on itself, which delegates to either
`SecurityGate::definitions` or `ToolRegistry::definitions` depending on
which is attached. The resulting `Vec<ToolDefinition>` is passed to
`llm.chat_stream` as the third argument, and the LLM provider
translates each entry into its native tool-use format. The list is
regenerated per turn, not per run, so a mid-run skill install or MCP
refresh becomes visible to the model on the next turn without
restarting the loop.

There is no cross-turn filtering — the LLM sees every tool every turn.
This is a deliberate choice: the **[Focus layer](../glossary.md#focus-layer)**
of the **[onion context](../glossary.md#onion-context)** may add
just-in-time documentation about a subset of tools relevant to the
current goal, but the tool list itself is full. Context compression
instead happens in the system prompt, where identity and narrative
material is pruned before tool definitions.

A registry with ~60 built-ins plus MCP bridges plus skills can push the
tool list above 5000 tokens on a big install. Providers that support
partial tool use (Anthropic's cache control, OpenAI's tool manifest
caching) amortize this cost across turns in the same run. Providers
that do not pay the full tokens per turn, which is why
`max_tool_output_tokens` and the context pruner exist as the second
line of defense.

## Testing

The `registry.rs` test module at
`crates/ryvos-tools/src/registry.rs:211` covers register/get,
unregister, list, definitions, execute success, execute not found,
`with_builtins`, and the overwrite-on-duplicate case. Each built-in has
its own tests in its module — `bash.rs`, `read.rs`, `edit.rs`, and the
rest — and `ryvos-test-utils` provides `test_tool_context` and
`MockTool` so skill, MCP, and gate tests can stand up a registry
without real I/O. The gate's own test suite at
`crates/ryvos-agent/src/gate.rs:283` covers the passthrough behaviors
(all tools execute), the unknown-tool error, the soft-checkpoint
timeout proceeding anyway, and the legacy tier fields being ignored by
dispatch.

## Where to go next

- [../guides/adding-a-tool.md](../guides/adding-a-tool.md) — the
  step-by-step recipe for implementing `Tool` and registering a new
  built-in.
- [../crates/ryvos-tools.md](../crates/ryvos-tools.md) — the module
  map and per-category tour of every shipped tool.
- [agent-loop.md](agent-loop.md) — the dispatch call site inside the
  per-turn loop, including parallel vs serial execution and tool
  result compaction.
- [mcp-bridge.md](mcp-bridge.md) — how `McpBridgedTool` wraps external
  MCP servers into the same registry.
- [../crates/ryvos-skills.md](../crates/ryvos-skills.md) — how the
  skill loader stands up `SkillTool` entries from `~/.ryvos/skills/`.
- [../adr/002-passthrough-security.md](../adr/002-passthrough-security.md)
  — why dispatch does not gate on tier or classification.
