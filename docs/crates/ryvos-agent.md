# ryvos-agent

The orchestration crate. `ryvos-agent` is the largest crate in the workspace
and the one where the runtime story actually happens: it composes
`ryvos-core`'s traits, `ryvos-llm`'s providers, `ryvos-tools`'s registry, and
`ryvos-memory`'s stores into the loops, watchdogs, and learners that define
how a single user message becomes a finished response. Every other
orchestration-layer concern — the ReAct **[agent runtime](../glossary.md#agent-runtime)**,
the **[Director](../glossary.md#director)**, the **[Guardian](../glossary.md#guardian)**,
the **[Judge](../glossary.md#judge)**, the **[SafetyMemory](../glossary.md#safetymemory)**,
the **[failure journal](../glossary.md#failure-journal)**, the
**[security gate](../glossary.md#security-gate)**, the
**[approval broker](../glossary.md#approval-broker)**, the
**[checkpoint](../glossary.md#checkpoint)** store, and the cron scheduler —
lives here.

This document is the structural reference for the crate. It walks the module
tree, names the key types, and points at the internals documents that cover
each subsystem in depth. For the narrative view of how these pieces
interleave at runtime, read
[../architecture/execution-model.md](../architecture/execution-model.md) in
parallel.

## Position in the workspace

`ryvos-agent` sits at the top of the orchestration layer. Its direct
workspace dependencies are `ryvos-core` (for traits, types, config, and the
**[EventBus](../glossary.md#eventbus)**), `ryvos-llm` (for the `LlmClient`
implementations), `ryvos-tools` (for the
**[tool registry](../glossary.md#tool-registry)**), and `ryvos-memory` (for
session and cost persistence). Nothing in the orchestration layer depends on
`ryvos-agent`; everything in the integration layer (`ryvos-gateway`,
`ryvos-channels`, `ryvos-tui`) does.

The crate exposes a small surface from its `lib.rs`: one `AgentRuntime`, one
`Director`, one `Guardian`, and a handful of supporting stores. All of these
re-exports live in `crates/ryvos-agent/src/lib.rs`. The file is short — 76
lines — because every subsystem lives in its own module and the lib only
re-exports the types that callers need.

One detail worth noting up front: `AgentRuntime` implements
`ryvos_core::types::AgentSpawner`. The impl is in `crates/ryvos-agent/src/lib.rs:69`
and looks like this:

```rust
impl AgentSpawner for AgentRuntime {
    fn spawn(&self, prompt: String) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            let sub_session = SessionId::new();
            self.run(&sub_session, &prompt).await
        })
    }
}
```

This is how the `spawn_agent` built-in tool creates a sub-agent without
knowing anything about the runtime: the tool takes an `AgentSpawner` out of
`ToolContext` and calls `spawn()`. The
**[PrimeOrchestrator](../glossary.md#prime)** (in `crates/ryvos-agent/src/prime.rs`)
also implements `AgentSpawner` and produces restricted sub-agents under a
tighter security policy.

## Key types at a glance

Before walking the modules, it helps to see the cast of characters in one
place. The table below lists the public structs and enums that matter most
to callers; every entry has its own section below with the full story.

| Type | Module | Purpose |
|---|---|---|
| `AgentRuntime` | `agent_loop` | The ReAct loop; one per daemon, cloned via `Arc` |
| `Director` | `director` | Goal-driven OODA orchestrator; replaces the loop on goal runs |
| `DirectorResult` | `director` | Output of a Director run (output, cycles, failures) |
| `Guardian`, `GuardianAction` | `guardian` | Background watchdog + its action channel |
| `Heartbeat` | `heartbeat` | Timer-driven self-check |
| `Judge` | `judge` | Two-level goal evaluator |
| `GoalEvaluator`, `RunEvaluator` | `evaluator` | Per-cycle and per-run LLM-as-judge |
| `SecurityGate` | `gate` | Passthrough auditing chokepoint for tool calls |
| `SafetyMemory`, `SafetyLesson`, `SafetyOutcome`, `Severity` | `safety_memory` | Self-learning safety store |
| `FailureJournal`, `FailureRecord` | `healing` | Per-session failure tracker backing Reflexion |
| `OutputValidator`, `OutputCleaner`, `ValidationResult` | `output_validator` | Structured-output repair |
| `ApprovalBroker` | `approval` | HITL approval coordination with oneshot channels |
| `AuditTrail`, `AuditEntry` | `audit` | Append-only SQLite audit log |
| `CheckpointStore`, `Checkpoint` | `checkpoint` | Per-turn crash-recovery snapshots |
| `SessionManager` | `session` | In-memory channel-keyed session index |
| `RunLogger` | `run_log` | JSONL event subscriber |
| `CronScheduler` | `scheduler` | Persistent cron driver |
| `MultiAgentOrchestrator`, `AgentCapability`, `OrchestratorBuilder`, `DispatchMode` | `orchestrator` | Multi-agent routing |
| `PrimeOrchestrator`, `PrimeRuntimeBuilder` | `prime` | Restricted sub-agent spawner |
| `Node`, `Edge`, `EdgeCondition`, `HandoffContext`, `GraphExecutor`, `NodeResult`, `ExecutionResult` | `graph` | DAG executor the Director targets |
| `ContextBuilder`, `ExtendedContext` | `context` | Onion-context assembler |
| `FailureTracker` | `intelligence` | In-memory per-tool failure counter |

## Module map

The crate has twenty Rust modules plus a `graph/` submodule with four more.
They group naturally into six families. The rest of this section walks each
family, naming the key types and linking to the internals document that
covers the subsystem in depth.

### Core runtime

The agent loop lives in one module: `crates/ryvos-agent/src/agent_loop.rs`.
It is the longest file in the crate (~1200 lines) because it holds the whole
ReAct state machine in a single `impl AgentRuntime` block. The struct itself
is compact:

```rust
pub struct AgentRuntime {
    config: AppConfig,
    llm: Arc<dyn LlmClient>,
    tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
    gate: Option<Arc<SecurityGate>>,
    store: Arc<dyn SessionStore>,
    event_bus: Arc<EventBus>,
    cancel: CancellationToken,
    journal: Option<Arc<FailureJournal>>,
    guardian_hints: Option<Arc<Mutex<mpsc::Receiver<GuardianAction>>>>,
    checkpoint_store: Option<Arc<CheckpointStore>>,
    cost_store: Option<Arc<CostStore>>,
    last_message_id: Arc<Mutex<Option<String>>>,
    cli_session_override: Arc<Mutex<Option<String>>>,
    pub spawner: Arc<Mutex<Option<Arc<dyn AgentSpawner>>>>,
    pub viking_client: Arc<Mutex<Option<Arc<VikingClient>>>>,
    safety_memory: Option<Arc<SafetyMemory>>,
}
```

Most fields are `Option`: a runtime is constructed with `AgentRuntime::new`
or `AgentRuntime::new_with_gate` and then configured in place with
`set_safety_memory`, `set_journal`, `set_guardian_hints`,
`set_checkpoint_store`, `set_cost_store`, and `set_viking_client`. The daemon
bootstrap in `src/main.rs` assembles the runtime in one place and then hands
it out through `Arc`s.

The two public entry points are `run` (for reactive runs) and
`run_with_goal` (for goal-driven runs). When `run_with_goal` is called with
a goal and the `[agent.director]` config section enables the Director, the
runtime delegates to `run_with_director`, which constructs a `Director` and
hands off control. Otherwise the standard ReAct loop runs.

The standard loop does eight things per turn: build the
**[onion context](../glossary.md#onion-context)**, prune to the token
budget (or memory-flush if near the limit), call `llm.chat_stream` and
accumulate deltas, execute any pending tool calls through the gate, inject
**[Reflexion](../glossary.md#reflexion)** hints when a tool has failed
repeatedly, evaluate the Judge if a goal is attached, write a checkpoint,
and update the cost store. Internal structure of the loop is covered in
depth in [../internals/agent-loop.md](../internals/agent-loop.md).

A few shape decisions in the loop are worth naming up front. First, tool
call accumulation is incremental: each `ToolUseStart` delta creates a
`ToolCallAccumulator` that appends `ToolInputDelta` fragments to a single
JSON string, and the accumulated payload is parsed only when the
`Stop(ToolUse)` signal arrives. This keeps streaming parallel to partial
tool planning and lets the loop dispatch every tool call in a single
batch once the whole **[turn](../glossary.md#turn)** is received. Second,
tool calls within a batch run concurrently via `futures::future::join_all`,
so three independent reads fan out and return in parallel. Third,
per-turn stop conditions are explicit: `StopReason::EndTurn` with no
tool calls ends the run cleanly, `StopReason::MaxTokens` ends it with
the truncated response, exceeding `max_turns` or `max_duration_secs`
errors out, and the shared `CancellationToken` fires the moment the
Guardian sends `CancelRun` or the operator Ctrl-Cs the daemon. Fourth,
the loop reads `GuardianAction` values between turns, not mid-turn: a
hint injected by the Guardian becomes a new user message inserted
before the next LLM call.

### Goal-driven execution

Five modules form the Director subsystem and its dependencies.

`director.rs` holds the Director and its `DirectorResult`:

```rust
pub struct DirectorResult {
    pub output: String,
    pub succeeded: bool,
    pub evolution_cycles: u32,
    pub total_nodes_executed: usize,
    pub semantic_failures: Vec<SemanticFailure>,
}

pub struct Director {
    llm: Arc<dyn LlmClient>,
    config: ModelConfig,
    event_bus: Arc<EventBus>,
    max_evolution_cycles: u32,
    failure_threshold: usize,
}
```

The top-level `Director::run` method implements the
**[OODA](../glossary.md#ooda)** cycle: generate a DAG for the goal, execute
the graph, evaluate the result, and on failure diagnose, evolve the plan,
and retry — up to `max_evolution_cycles` times. Each cycle publishes events
on the EventBus so the gateway UI, the TUI, and the audit trail can follow
along. The deep dive is in [../internals/director-ooda.md](../internals/director-ooda.md),
and the motivation is recorded in [ADR-009](../adr/009-director-ooda-loop.md).

`judge.rs` contains the two-level Judge. Level 0 is a synchronous,
deterministic check over `OutputContains` and `OutputEquals` criteria: no
LLM call, no network, no I/O. Level 2 is an LLM call that returns a
**[Verdict](../glossary.md#verdict)** (`Accept` / `Retry` / `Escalate` /
`Continue`) by JSON-parsing the model response. The `Judge::evaluate`
entry point tries Level 0 first and falls back to Level 2 only when the goal
has any `LlmJudge` criteria. Level 0 also runs automatically at the end of
every Director cycle. See [../internals/judge.md](../internals/judge.md) for
the full state machine and prompt templates.

`evaluator.rs` holds two related evaluators that are used in different
contexts. `GoalEvaluator` is the per-cycle evaluator the Director consults;
`RunEvaluator` is a simpler run-level LLM-as-judge used by the standard
agent loop for self-reflection, producing a `RunOutcome` with `success`,
`confidence`, `reasoning`, and `suggestions` fields.

The `graph/` submodule contains the DAG executor that the Director uses to
run multi-node plans. It has four files:

- `graph/node.rs` — `Node` is a single step in the graph with an `id`, an
  optional `system_prompt`, `input_keys`, `output_keys`, a `tools` list, a
  `max_turns` cap, an optional `goal`, and an optional per-node model
  override.
- `graph/edge.rs` — `Edge` connects nodes with an `EdgeCondition`:
  `Always`, `OnSuccess`, `OnFailure`, `Conditional { expr }`, or
  `LlmDecide { prompt }`. The conditional expression grammar supports
  `key == "value"`, `key != "value"`, and `key contains "substr"`.
- `graph/handoff.rs` — `HandoffContext` is the shared key-value map that
  flows through the graph. Keys are strings, values are
  `serde_json::Value`. Nodes read their declared `input_keys` and write
  their declared `output_keys`.
- `graph/executor.rs` — `GraphExecutor` walks the graph from an entry
  node, running each node through `AgentRuntime::run_with_goal`, ingesting
  the output into the HandoffContext, and following the first matching
  outgoing edge. Safety against cycles comes from a per-node visit counter
  that caps re-execution at five visits per node.

The narrative on graph execution — how parallel branches fan out, how
edge conditions resolve, how handoff context accumulates — is in
[../internals/graph-executor.md](../internals/graph-executor.md).

`orchestrator.rs` and `prime.rs` implement two different flavors of
multi-agent orchestration. `MultiAgentOrchestrator` (in `orchestrator.rs`)
holds a registry of `AgentCapability` descriptors and routes tasks to the
best-matching agent. `AgentCapability` records each agent's tools,
specializations, optional goal, optional security policy, and optional
model override, and its `match_score` method ranks agents against a task
description. The orchestrator supports three `DispatchMode`s: `Parallel`
(fan out across all agents, collect every result), `Relay` (chain tasks so
each agent's output feeds the next), and `Broadcast` (same task to every
agent). `PrimeOrchestrator` (in `prime.rs`) is the narrower case used for
short-lived sub-agents: it holds a `SecurityPolicy` for sub-agents and a
`PrimeRuntimeBuilder`, and its `spawn_restricted` method constructs a fresh
`AgentRuntime` with a new `SecurityGate` bound to that stricter policy.

### Safety and self-correction

Four modules implement the passthrough safety model.

`gate.rs` holds `SecurityGate`, the single chokepoint every tool call
passes through:

```rust
pub struct SecurityGate {
    policy: SecurityPolicy,
    tools: Arc<RwLock<ToolRegistry>>,
    broker: Arc<ApprovalBroker>,
    event_bus: Arc<EventBus>,
    safety_memory: Option<Arc<SafetyMemory>>,
    audit_trail: Option<Arc<AuditTrail>>,
}
```

`SecurityGate::execute` runs five steps in order: log the call to the audit
trail (pre-execution), fetch any relevant lessons from `SafetyMemory`,
generate a one-line safety reasoning string, wait for a
**[soft checkpoint](../glossary.md#soft-checkpoint)** if `pause_before`
lists the tool (opt-in only), execute the tool, and finally call
`assess_outcome` to classify the result. It never refuses to dispatch a
call on classification alone; the only way a tool is stopped is an
explicit `ApprovalDecision::Denied` from a human. The rationale is in
[ADR-002](../adr/002-passthrough-security.md), and the deprecated
**[T0–T4](../glossary.md#t0t4)** tiers are kept only as informational
metadata.

`safety_memory.rs` holds `SafetyMemory` itself. The public types are
`Severity`, `SafetyOutcome` (four variants: `Harmless`, `NearMiss`,
`Incident`, `UserCorrected`), and `SafetyLesson` (a single learned rule
with an `id`, a `timestamp`, an `action` pattern, the `outcome`, a human
`reflection`, an optional `principle_violated`, a `corrective_rule`, a
`confidence` score, and a `times_applied` counter). The store is backed by
`safety.db` via a WAL-mode SQLite connection.

Three free functions in the same module do the classification work. They
are small enough to document inline:

- `detect_destructive_command(cmd)` runs a compiled regex set over bash
  command strings. The patterns cover `rm -rf` and its flag permutations,
  `dd if=`, `mkfs`, recursive world-writable `chmod`, the classic fork
  bomb, raw writes to `/dev/sd*`, `curl|sh` and `wget|sh`, and overwrites
  of `/etc/`. A match returns a short label and classifies the outcome as
  `NearMiss`.
- `detect_secret_in_output(output)` scans tool output for AWS access and
  secret keys, GitHub PAT / OAuth / App tokens, OpenAI and Anthropic
  `sk-*` keys, PEM private keys, Slack bot and user tokens, JWTs, and
  `password=` patterns. A match classifies the outcome as `Incident` with
  `High` severity. Outputs shorter than 20 characters are skipped to
  avoid false positives.
- `assess_outcome(tool_name, input, result, is_error)` is the single entry
  point called from the gate. It runs the destructive-command check on
  bash inputs, the secret-scan on the output, and a handful of
  error-message checks (permission denied, missing-file on delete tools,
  and so on), then returns a `SafetyOutcome`.

When the outcome is anything other than `Harmless`, the gate writes a new
`SafetyLesson` to `safety.db`. On future runs, `SafetyMemory::relevant_lessons`
fetches lessons that mention the current tool name (ordered by confidence
and `times_applied`) and `format_for_context` renders the top-ranked ones
as a Markdown block that the context builder injects into the system
prompt. The full lifecycle — recording, reinforcing, pruning, and injecting
— is documented in [../internals/safety-memory.md](../internals/safety-memory.md).

`healing.rs` owns the failure journal and the reflexion path. The
`FailureJournal` struct wraps a SQLite connection on `healing.db` and
holds three tables: `failure_journal` (per tool, per turn), `success_journal`
(for health scoring), and `decisions` (tool-choice decisions with
alternatives and outcomes). The module also exposes
`reflexion_hint_with_history(tool_name, failure_count, past)`, which builds
a `ChatMessage` summarizing the last three matching past failures and
suggesting a different approach; this message is injected into the
conversation before the next turn when the in-memory `FailureTracker` in
`intelligence.rs` has seen the same tool fail at least `N` times in a row.
Reflexion is advisory — nothing blocks, nothing retries automatically. The
design is covered in [../internals/reflexion.md](../internals/reflexion.md).

`output_validator.rs` contains two cooperating types. `OutputValidator`
takes a list of `required_keys`, a `max_length`, and an optional JSON
schema, and returns a `ValidationResult` of `Valid` or
`Invalid { issues }`. `OutputCleaner` then repairs invalid output. It has
two modes: `heuristic_only` (no LLM; strips common markdown code fences,
trims prose, and extracts the first JSON object) and a full LLM repair
mode that asks the configured model to rewrite the output in a valid
shape. The structured-output validator is used by the Director to ensure
its graph-generation responses parse as DAGs, and by any caller that sets
`OutputValidator::required_keys`. See
[../internals/output-validator.md](../internals/output-validator.md).

### Background systems

Three modules run concurrently with the agent loop for the lifetime of
the daemon.

`guardian.rs` holds the Guardian watchdog:

```rust
pub struct Guardian {
    config: GuardianConfig,
    event_bus: Arc<EventBus>,
    cancel: CancellationToken,
    hint_tx: mpsc::Sender<GuardianAction>,
    cost_store: Option<Arc<CostStore>>,
    budget_config: Option<BudgetConfig>,
}

pub enum GuardianAction {
    InjectHint(String),
    CancelRun(String),
}
```

Construction returns both the Guardian and its `mpsc::Receiver` half; the
receiver is handed to `AgentRuntime::set_guardian_hints` so the agent loop
can drain it between turns. `Guardian::run` is an event loop subscribed to
the EventBus that watches for three pathologies. **[Doom loops](../glossary.md#doom-loop)**
are detected by a rolling window of `ToolCallRecord`s, each containing the
tool name and a normalized JSON fingerprint (keys sorted, whitespace
stripped, first 300 characters); when the last `doom_loop_threshold`
records all match, the Guardian sends `InjectHint("…")` and the agent
loop picks it up on the next turn. Stalls are detected by a
`last_progress` `Instant` that resets on every `ToolStart` or progress
event; if no progress arrives within `stall_timeout_secs`, a stall hint
is injected. Budget enforcement has two flavors: a token budget that warns
at `token_warn_pct` and hard-stops at 100%, and a dollar budget driven by
the `BudgetConfig` (`monthly_budget_cents`, `warn_pct`, `hard_stop_pct`)
that reads accumulated spend from the `CostStore`. A hard stop translates
to `CancelRun`, which the agent loop applies by firing its
`CancellationToken`. Full detection logic is in
[../internals/guardian.md](../internals/guardian.md).

`heartbeat.rs` holds the timer-driven **[Heartbeat](../glossary.md#heartbeat)**:

```rust
pub struct Heartbeat {
    config: HeartbeatConfig,
    runtime: Arc<AgentRuntime>,
    event_bus: Arc<EventBus>,
    cancel: CancellationToken,
    workspace: PathBuf,
    session_meta: Option<Arc<SessionMetaStore>>,
    audit_trail: Option<Arc<AuditTrail>>,
}
```

`Heartbeat::run` sleeps for `interval_secs` (default 1800), checks whether
the current time is within the configured active hours, publishes a
`HeartbeatFired` event, builds a prompt by reading `HEARTBEAT.md` from the
workspace and optionally injecting a safety retrospective over recent
flagged audit entries, and runs a short agent turn. If the response
matches one of the ack patterns (`HEARTBEAT_OK`, `all good`, `no issues`,
and a handful of others) and is short, the response is suppressed;
otherwise a `HeartbeatAlert` event is published so channels can forward
the finding. See [../internals/heartbeat.md](../internals/heartbeat.md).

`scheduler.rs` holds `CronScheduler`, which runs persistent cron jobs
defined in `[[cron.jobs]]` config blocks. Each job has a name, a schedule
expression, a prompt, an optional channel, and an optional goal id. On
each tick the scheduler resolves the next fire time from the `cron`
crate's `Schedule`, sleeps until it passes, and then invokes
`AgentRuntime::run_with_goal` with the job's prompt. Results are
published on the EventBus for channel routing. Full details are in
[../internals/cron-scheduler.md](../internals/cron-scheduler.md).

### Session and persistence

Five modules handle the stores that keep a session alive and accountable.

`session.rs` is the thinnest file in the crate. `SessionManager` holds a
`HashMap<String, SessionInfo>` keyed by a channel prefix (for example
`telegram:user:12345`). `get_or_create` returns the session id for a key,
creating one on first contact; `set_cli_session_id` and
`get_cli_session_id` remember the last CLI-provider session id for resume;
`record_run_stats` accumulates run counts, token totals, and
**[billing type](../glossary.md#billing-type)** per session; and `restore`
rehydrates a session from the `SessionMetaStore`. The deeper
per-channel isolation story is in
[../internals/session-manager.md](../internals/session-manager.md).

`checkpoint.rs` owns `CheckpointStore`, a SQLite-backed store at
`checkpoints.db` (wrapped inside `sessions.db` in the default layout) that
saves one row per turn. A `Checkpoint` row contains the session id, a
run id (so retries within the same session stay distinct), the turn
number, serialized messages, cumulative input and output tokens, and a
timestamp. On save, the store deletes older checkpoints for the same run
and inserts the new one — the store holds only the latest turn, not a
full history. Crash recovery is documented in
[../internals/checkpoint-resume.md](../internals/checkpoint-resume.md).

`audit.rs` owns `AuditTrail`, the single writer for `audit.db`. Each
`AuditEntry` records the timestamp, session id, tool name, input and
output summaries, an optional safety-reasoning string, the `SafetyOutcome`,
and the list of lesson ids that were available at dispatch time. The
schema has indexes on `session_id`, `tool_name`, and `timestamp DESC`
for the common read patterns — per-session audit, per-tool audit, and
recent-first. Reads go through `AuditReader` in `ryvos-mcp` (see
[ryvos-mcp.md](ryvos-mcp.md)) because the MCP server needs a read-only
handle that is safe to share while the daemon writes.

`run_log.rs` is the JSONL run logger. `RunLogger::run` is a background task
that subscribes to the EventBus and writes one JSON object per line to
`{log_dir}/{session_id}/{timestamp}.jsonl`. The `level` parameter
controls verbosity (1 = summary, 2 = per-turn, 3 = per-step), and the
append-only format is deliberately crash-resilient: even if the daemon
crashes mid-run, every previously flushed line is still a valid JSON
document.

`approval.rs` holds `ApprovalBroker`. The broker keeps a
`HashMap<request_id, (ApprovalRequest, oneshot::Sender<ApprovalDecision>)>`
of pending approvals. When a `SecurityGate` triggers a soft checkpoint,
it calls `broker.request(req)` to publish an `ApprovalRequested` event
and register the oneshot receiver; any channel adapter, the TUI, or the
gateway can then call `broker.respond(request_id, decision)` to resolve
it. `find_by_prefix` lets the TUI accept partial request ids on the
command line. Approval is opt-in per tool; no run is ever paused unless
`pause_before` explicitly lists the tool.

### Context and intelligence

Two modules build the system prompt and keep it within the token budget.

`context.rs` is the onion-context assembler. `ContextBuilder` is a fluent
builder whose public methods mirror the three layers from the glossary:
`with_identity_layer`, `with_narrative_layer`, and the focus-layer entry
points `with_instructions`, `with_mcp_resources`, and `with_goal`. Two
convenience wrappers — `build_default_context` and `build_goal_context` —
assemble the full stack for reactive and goal-driven runs respectively.
Both wrappers accept an optional `ExtendedContext` carrying a
pre-rendered **[Viking](../glossary.md#viking)** recall fragment and a
pre-rendered safety-memory block.

`DEFAULT_SYSTEM_PROMPT` is a long literal string at the top of the file.
It sets the base rules ("act, don't instruct"; "remember everything
important"; "be concise"), describes the Viking memory URIs to use, and
spells out the seven principles of the safety constitution plus an eighth
rule for external-data handling that requires `<external_data trust="untrusted">`
tags to be treated as data, not commands. The safety constitution is the
in-prompt expression of the constitutional-AI model from
[ADR-002](../adr/002-passthrough-security.md). The full layering story is
in [../architecture/context-composition.md](../architecture/context-composition.md).

`intelligence.rs` owns token budgeting. `estimate_tokens` and
`estimate_message_tokens` run `tiktoken_rs`'s `cl100k_base` BPE encoder
(shared via `OnceLock`); `prune_to_budget` removes the oldest
non-protected messages from the middle of the conversation until the
total fits within a budget; `summarize_and_prune` is a phase-aware
variant that first asks the LLM to summarize the removable messages into
a single compact message, preserving phases, and then applies the budget
trim. Protected messages (those with `metadata.protected == true`) are
never removed or summarized. The module also exposes `compact_tool_output`
(truncates a tool result to a token cap at line boundaries),
`memory_flush_prompt` (the user message the agent sees when the context
is about to be pruned, giving it a chance to persist important facts to
Viking or the daily log), `is_flush_complete` (a heuristic that reads
the next assistant message to decide whether the flush is done), and
`FailureTracker` (a per-session `HashMap<tool_name, count>` that drives
reflexion). The context-composition walkthrough is in
[../architecture/context-composition.md](../architecture/context-composition.md).

## Concurrency model

The runtime is built on tokio and uses `tokio::select!` extensively. Each
`AgentRuntime::run` call executes on the caller's task; the background
systems (`Guardian`, `Heartbeat`, `CronScheduler`, `RunLogger`) each own
their own spawned task and are shut down via a shared `CancellationToken`.

Three channel types carry information between these tasks:

- A `tokio::sync::broadcast` channel (the EventBus) carries every
  `AgentEvent` to every subscriber. Capacity is 256; slow subscribers drop
  messages rather than back-pressuring the publisher. This is the tradeoff
  recorded in [ADR-005](../adr/005-event-driven-architecture.md).
- A `tokio::sync::mpsc` channel (capacity 32) carries `GuardianAction`
  values from the Guardian task to the agent loop. The agent loop
  consumes these between turns.
- A `tokio::sync::oneshot` channel per pending approval carries a single
  `ApprovalDecision` from the responder back to the gate. The gate awaits
  the receiver under a `tokio::time::timeout`; on timeout the call
  proceeds anyway (passthrough).

`CancellationToken` propagation is the single source of truth for
shutdown. The daemon creates one token, clones it into every subsystem,
and on SIGINT calls `cancel()`; every task is built around a
`tokio::select!` that either awaits `cancel.cancelled()` or reads from
its primary event source. The event-bus semantics, lifetimes, and
filtered-subscription patterns are in
[../internals/event-bus.md](../internals/event-bus.md).

## Where to go next

The subsystem deep dives live under [../internals/](../internals/). Start
with [../internals/agent-loop.md](../internals/agent-loop.md) for the
ReAct state machine, then follow whichever pointer matches your question:
[../internals/director-ooda.md](../internals/director-ooda.md) for
goal-driven execution,
[../internals/guardian.md](../internals/guardian.md) for the background
watchdog, [../internals/safety-memory.md](../internals/safety-memory.md)
for the learning path,
[../internals/judge.md](../internals/judge.md) for two-level evaluation,
or [../internals/checkpoint-resume.md](../internals/checkpoint-resume.md)
for crash recovery.

For the architectural story that ties these pieces together across a
single user message, read
[../architecture/execution-model.md](../architecture/execution-model.md).
For the design decisions that shaped the crate, the relevant ADRs are
[002](../adr/002-passthrough-security.md),
[005](../adr/005-event-driven-architecture.md),
[006](../adr/006-separate-sqlite-databases.md), and
[009](../adr/009-director-ooda-loop.md).
