# ryvos-core

`ryvos-core` is the foundation crate of the Ryvos workspace. It defines the
types, traits, configuration schema, error hierarchy, event system, goal
model, and security primitives that every other crate in the workspace depends
on. It depends on nothing else inside the workspace and performs no I/O, no
SQLite access, no HTTP calls, and no tokio-task spawning — everything here is
pure data and type definitions.

Because every crate transitively depends on `ryvos-core`, changes to public
types in this crate are the most expensive edits in the codebase. Anything
load-bearing in the system — the shape of a `ChatMessage`, the signature of
`LlmClient::chat_stream`, the variants of `AgentEvent` — is defined in exactly
one place here. The static structure of the workspace around this crate is
described in [../architecture/system-overview.md](../architecture/system-overview.md);
this document explains the contents.

## Position in the stack

`ryvos-core` sits at the bottom of the four-layer crate stack. `ryvos-llm`,
`ryvos-memory`, and `ryvos-tools` build directly on top of it. `ryvos-agent`,
the orchestration layer, pulls in all three and re-exports `ryvos-core` types
through its own public API. Every integration-layer crate — `ryvos-gateway`,
`ryvos-channels`, `ryvos-skills`, `ryvos-tui` — imports `ryvos-core` to obtain
trait definitions or payload types, never the other way around.

The crate's `Cargo.toml` (`crates/ryvos-core/Cargo.toml:7`) lists only
foundation dependencies: `serde` and `serde_json` for the wire format,
`thiserror` for the error enum, `toml` for config parsing, `uuid` and
`chrono` for identifiers and timestamps, `futures` for `BoxFuture` and
`BoxStream` return types, `tokio` for the broadcast channel that backs the
**[EventBus](../glossary.md#eventbus)**, `tracing` for the hook subsystem's
warnings, and `regex` for the (deprecated) dangerous-pattern matcher.

## Modules

`ryvos-core` exposes eight modules from `crates/ryvos-core/src/lib.rs:16`.
Each one owns a single concern and is described in the subsections that
follow.

### types

`crates/ryvos-core/src/types.rs` is the largest single file in the crate
(594 lines) and the conversation model lives here. It defines `SessionId`,
`Role`, `ContentBlock`, `MessageMetadata`, `ChatMessage`, `StopReason`,
`StreamDelta`, `ToolResult`, `ToolDefinition`, `ToolContext`, `AgentSpawner`,
`MessageEnvelope`, `MessageContent`, `SearchResult`, `Verdict`, `Decision`,
`DecisionOption`, `DecisionOutcome`, `BillingType`, `CostEvent`, `CostSummary`,
`AgentEvent`, and `ThinkingLevel`. The module is `pub use`d at the crate
root, so any downstream crate can write `use ryvos_core::ChatMessage;`.

### traits

`crates/ryvos-core/src/traits.rs` defines the four extension-point traits
that make Ryvos pluggable: `LlmClient`, `Tool`, `ChannelAdapter`, and
`SessionStore`. All four are `Send + Sync + 'static` so they can be shared
across tokio tasks behind an `Arc`. The signatures use `BoxFuture` and
`BoxStream` because the agent runtime stores trait objects heterogeneously
and cannot rely on `async fn` in traits at the workspace's current MSRV.

### config

`crates/ryvos-core/src/config.rs` is the largest file in the crate (1339
lines) and contains the full configuration schema parsed from `ryvos.toml`.
It defines `AppConfig` at the root and roughly forty nested structs below
it. `AppConfig::load` (`crates/ryvos-core/src/config.rs:1124`) reads the
file, expands `${ENV_VAR}` patterns via the helper in the same file, and
deserializes the result with `toml::from_str`. Missing env vars are left
literal so that configs can be copied between hosts without silent failures.

### error

`crates/ryvos-core/src/error.rs` defines `RyvosError` (a 24-variant
`thiserror` enum) and `type Result<T> = std::result::Result<T, RyvosError>`.
Every fallible operation in the workspace returns `ryvos_core::Result<T>`.

### event

`crates/ryvos-core/src/event.rs` provides the `EventBus` broadcast channel
along with `EventFilter` and `FilteredReceiver`. Every crate that wants to
publish a lifecycle event does so by constructing an `AgentEvent` and calling
`EventBus::publish`. See ADR-005 for the design rationale.

### goal

`crates/ryvos-core/src/goal.rs` contains the **[goal](../glossary.md#goal)**
model used by the **[Director](../glossary.md#director)** and the
**[Judge](../glossary.md#judge)**. `Goal`, `SuccessCriterion`,
`CriterionType`, `Constraint`, `ConstraintCategory`, `ConstraintKind`,
`CriterionResult`, `ConstraintViolation`, `GoalEvaluation`, `GoalObject`,
`SemanticFailure`, and `FailureCategory` are all defined here. Deterministic
criterion evaluation lives on `Goal::evaluate_deterministic` at
`crates/ryvos-core/src/goal.rs:170`.

### security

`crates/ryvos-core/src/security.rs` retains the deprecated
**[T0–T4](../glossary.md#t0t4)** tier enum for config-file compatibility and
provides two active helpers — `tool_has_side_effects` and `summarize_input` —
that the safety pipeline uses for reasoning and audit display. It also defines
`SecurityPolicy`, `ApprovalRequest`, `ApprovalDecision`, and the
`DangerousPatternMatcher` retained for config-backward-compat. See ADR-002
for the deprecation rationale.

### hooks

`crates/ryvos-core/src/hooks.rs` is a tiny (20-line) module with a single
function, `run_hooks`, that fires shell command hooks fire-and-forget with a
caller-supplied `$RYVOS_*` environment. All other crates use it by loading
the relevant `HooksConfig` field and passing the command list plus env vars.

## Conversation types

Three types form the conversation model that every LLM provider, every tool,
and every channel adapter speaks: `ChatMessage`, `ContentBlock`, and
`StreamDelta`.

A `ChatMessage` is a role (`System`, `User`, `Assistant`, or `Tool`) plus a
vector of `ContentBlock`s, an optional timestamp, and optional
`MessageMetadata` used by the context compactor to mark messages as protected
or to tag them with a phase. Convenience constructors on
`crates/ryvos-core/src/types.rs:99` (`ChatMessage::user`,
`ChatMessage::assistant_text`, `ChatMessage::tool_result`) cover the common
cases and set the timestamp to `Utc::now()`. The `text()` method flattens all
`ContentBlock::Text` blocks into a single string, and `tool_uses()` extracts
all `ContentBlock::ToolUse` blocks as tuples of `(id, name, input)` for the
agent loop's dispatch phase.

`ContentBlock` has four variants: `Text`, `ToolUse`, `ToolResult`, and
`Thinking`. The tag-based serde representation (`#[serde(tag = "type")]`)
means blocks round-trip cleanly through JSON, which the checkpoint store, the
run logger, and the gateway WebSocket all depend on.

`StreamDelta` is the wire format for streaming LLM responses. It has ten
variants: `TextDelta` and `ThinkingDelta` carry streamed text chunks;
`ToolUseStart`, `ToolInputDelta`, and the terminal `Stop(StopReason)` form
the tool-call protocol; `Usage` reports per-call token counts; `MessageId`
carries the provider's request ID for cost correlation; and `CliToolExecuted`
/ `CliToolResult` are emitted only by the two
**[CLI providers](../glossary.md#cli-provider)** (`claude-code` and
`copilot`) to report tools that ran inside the subprocess — Ryvos cannot
block those but can still audit them. `StopReason` itself has four variants:
`EndTurn`, `ToolUse`, `MaxTokens`, `StopSequence`.

`ToolResult` is a two-field struct (`content: String`, `is_error: bool`)
with `ToolResult::success` and `ToolResult::error` constructors. It is the
only thing a `Tool` impl ever returns to the runtime.

`ToolContext` is the per-invocation bag of dependencies that the runtime
passes into every `Tool::execute` call. It carries the session id, the
working directory, an optional `SessionStore` handle, an optional
`AgentSpawner` (for sub-agent tools), an optional `SandboxConfig`, an
optional config file path (for cron/config tools that mutate `ryvos.toml`),
and an optional Viking client erased to `dyn Any` to avoid pulling the
`ryvos-memory` crate into `ryvos-core`.

## Core traits

`LlmClient` has a single method:

```rust
fn chat_stream(
    &self,
    config: &ModelConfig,
    messages: Vec<ChatMessage>,
    tools: &[ToolDefinition],
) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>>;
```

The double-boxed return type — a future that yields a stream — is what lets
the agent loop handle both providers that buffer the whole response before
streaming (`copilot`) and providers that open a long-lived stream
immediately (`anthropic`). Every provider in `ryvos-llm` implements exactly
this one method.

`Tool` has four required methods and three defaults. The required methods
are `name`, `description`, `input_schema` (returning a JSON Schema value),
and `execute` (which takes owned JSON input plus a `ToolContext`). The
defaults are `timeout_secs` (30 seconds), `requires_sandbox` (`false`), and
`tier` (`SecurityTier::T1`). The tier default exists solely for backward
compatibility with the pre-v0.6 blocking security model; it is informational
today.

`ChannelAdapter` has five methods: `name`, `start` (takes an `mpsc::Sender`
down which the adapter pushes inbound `MessageEnvelope`s), `send` (outbound
for a specific session), `send_approval` (default impl returns `Ok(false)`
to let the runtime fall back to plain text approval UI), `broadcast`
(default is no-op for adapters that do not support it), and `stop` for
graceful shutdown. The four built-in adapters in `ryvos-channels` all
implement this trait; see ADR-010.

`SessionStore` has three methods: `append_messages`, `load_history` (with a
limit), and `search` (for full-text retrieval across all sessions). The
production implementation is `SqliteSessionStore` in `ryvos-memory`; the
in-memory implementation used in tests is `InMemorySessionStore` in
`ryvos-test-utils`.

## Configuration hierarchy

`AppConfig` is the root of the configuration tree
(`crates/ryvos-core/src/config.rs:142`). Its fields are listed below, each
with a one-line description. All non-`Option` fields have `#[serde(default)]`
so that a minimal `ryvos.toml` with just `[model]` is valid.

- `agent: AgentConfig` — turn limits, workspace, system prompt, context
  budget, parallel tool dispatch, guardian settings, director settings.
- `model: ModelConfig` — primary LLM provider, model id, API key,
  temperature, max tokens, thinking level, retry policy.
- `fallback_models: Vec<ModelConfig>` — ordered failover targets when the
  primary errors or is overloaded.
- `gateway: Option<GatewayConfig>` — Axum bind address, embedded Web UI
  toggle, role-based API key table.
- `channels: ChannelsConfig` — per-platform sub-configs for Telegram,
  Discord, Slack, and WhatsApp.
- `mcp: Option<McpConfig>` — external MCP server list and the embedded
  `.mcp.json` merger.
- `hooks: Option<HooksConfig>` — eight shell-hook lists (described below).
- `wizard: Option<WizardMetadata>` — last-run timestamp of the onboarding
  wizard, used by `ryvos doctor`.
- `cron: Option<CronConfig>` — persistent cron jobs and their Director
  routing options.
- `heartbeat: Option<HeartbeatConfig>` — interval, prompt path, active hours.
- `web_search: Option<WebSearchConfig>` — provider and API key for the
  `web_search` tool.
- `security: SecurityConfig` — soft checkpoints, `pause_before` list,
  approval timeout, sub-agent policy.
- `embedding: Option<EmbeddingConfig>` — local or remote embedder for
  semantic memory.
- `daily_logs: Option<DailyLogsConfig>` — retention and directory for the
  `daily_log_write` tool.
- `registry: Option<RegistryConfig>` — skill registry URL and cache dir.
- `budget: Option<BudgetConfig>` — monthly dollar cap, soft/hard thresholds,
  per-model pricing overrides.
- `openviking: Option<OpenVikingConfig>` — Viking memory endpoint and tier
  defaults.
- `google: Option<GoogleConfig>` — Gmail, Calendar, and Drive OAuth.
- `notion: Option<NotionConfig>` — Notion integration token.
- `jira: Option<JiraConfig>` — Jira Cloud credentials.
- `linear: Option<LinearConfig>` — Linear API key.
- `integrations: IntegrationsConfig` — one-click OAuth integration registry.

Nested under `AgentConfig` are `CheckpointConfig`, `DirectorConfig`,
`GuardianConfig`, `LogConfig`, `SandboxConfig`, and a `HashMap<String,
ModelConfig>` of per-agent model overrides. `ModelConfig` itself owns
`RetryConfig` and `ActiveHoursConfig`. `CronConfig` owns a list of
`CronJobConfig`. `McpConfig` owns `McpServerConfig` and the merged
`McpJsonConfig`. `GatewayConfig` owns a list of `ApiKeyConfig`.
`SecurityConfig` owns `SubAgentPolicyConfig`. `IntegrationsConfig` owns
per-provider integration configs. Full field-level documentation is in the
rustdoc for each struct; the crate doc deliberately avoids exhaustive field
lists so that new fields do not force a doc update.

`AppConfig::load` is the only public entry point. It reads the file,
expands `${ENV_VAR}` patterns with a small hand-written scanner, then
deserializes with `toml::from_str`. Missing variables are left as the literal
`${VAR_NAME}` string rather than replaced with empty strings, which means a
typo in an environment variable name is caught by TOML parsing rather than
silently producing a blank API key. `AppConfig::workspace_dir` expands a
leading `~/` in the workspace path to `$HOME`.

## Error taxonomy

`RyvosError` has 24 variants grouped by the subsystem that raises them:

| Group | Variants |
|---|---|
| LLM | `LlmRequest`, `LlmStream`, `UnsupportedProvider`, `LlmParse` |
| Tool | `ToolNotFound`, `ToolExecution`, `ToolTimeout`, `ToolValidation` |
| Agent | `MaxTurnsExceeded`, `MaxDurationExceeded`, `Cancelled` |
| Config | `Config`, `ConfigNotFound` |
| Storage | `Database` |
| Channel | `Channel` |
| Gateway | `Gateway` |
| Security | `ToolBlocked` (deprecated), `ApprovalDenied`, `ApprovalTimeout`, `SecurityViolation` |
| Budget | `BudgetExceeded` |
| MCP | `Mcp` |
| I/O | `Io` (`#[from] std::io::Error`), `Json` (`#[from] serde_json::Error`) |

`ToolBlocked` is retained for backward compatibility but is never raised by
the passthrough `SecurityGate` in current code. `Io` and `Json` use
`#[from]` so that `?` propagation from standard-library operations works
without manual conversion. The `Result<T>` alias at the bottom of
`crates/ryvos-core/src/error.rs` is the canonical return type for every
fallible function in the workspace.

## Event bus

The **[EventBus](../glossary.md#eventbus)** is a thin wrapper around
`tokio::sync::broadcast::Sender<AgentEvent>` with a default capacity of 256
(`crates/ryvos-core/src/event.rs:37`). `publish` is fire-and-forget: if no
receivers are attached the send is silently dropped rather than raising an
error. `subscribe` returns a raw receiver; `subscribe_filtered` returns a
`FilteredReceiver` wrapped around an `EventFilter` that can constrain
delivery by session id, event type, or (eventually) node id for graph
execution.

Filtering is implemented client-side: every subscriber still receives every
published event, and the filter is applied in the `recv` loop. This keeps
the bus itself stateless and avoids the per-subscriber fan-out machinery
that a server-side filter would need. The tradeoff is that a slow subscriber
that filters aggressively still has to drain the broadcast buffer; if it
cannot keep up it loses messages. See
[../internals/event-bus.md](../internals/event-bus.md) for the full delivery
semantics and ADR-005 for the design rationale.

`AgentEvent` has 29 variants covering every lifecycle moment in the
runtime: `RunStarted`, `TextDelta`, `ToolStart`, `ToolEnd`, `TurnComplete`,
`RunComplete`, `RunError`, `CronFired`, `CronJobComplete`,
`ApprovalRequested`, `ApprovalResolved`, `ToolBlocked`, `GuardianStall`,
`GuardianDoomLoop`, `GuardianBudgetAlert`, `GuardianHint`, `UsageUpdate`,
`GoalEvaluated`, `DecisionMade`, `JudgeVerdict`, `HeartbeatFired`,
`HeartbeatOk`, `HeartbeatAlert`, `BudgetWarning`, `BudgetExceeded`,
`GraphGenerated`, `NodeComplete`, `EvolutionTriggered`, and
`SemanticFailureCaptured`. The `extract_session_id` helper at
`crates/ryvos-core/src/event.rs:115` is where the filter learns how to
project events down to a single session — new variants that carry a
`session_id` must be added to that match arm or they will silently bypass
session filtering.

## Hooks

Shell command hooks are opt-in callbacks that run at fixed points in the
agent lifetime. `HooksConfig` (`crates/ryvos-core/src/config.rs:19`) has
eight fields, each a `Vec<String>`: `on_start`, `on_message`,
`on_tool_call`, `on_response`, `on_turn_complete`, `on_tool_error`,
`on_session_start`, `on_session_end`. `run_hooks` spawns each command via
`sh -c` with the caller-supplied environment variables set (conventionally
prefixed `RYVOS_` — for example `RYVOS_SESSION_ID`, `RYVOS_TOOL_NAME`,
`RYVOS_TURN`). Stdout and stderr are discarded. A non-zero exit is logged
via `tracing::warn` but does not propagate into the agent run; hooks are
strictly fire-and-forget. This is deliberate: a broken hook should never
be able to hang or crash the runtime.

## Goal evaluation

`Goal` bundles a description, a weighted list of `SuccessCriterion`, a list
of `Constraint`, a `success_threshold` (default `0.9`), a `version` counter
bumped on each Director evolution cycle, and a `metrics` map for runtime
stats. `CriterionType` has four variants: `OutputContains` and `OutputEquals`
are deterministic and evaluated locally; `LlmJudge` delegates to an LLM with
a custom prompt; `Custom` is a named hook evaluated by the caller.

`Goal::evaluate_deterministic` (`crates/ryvos-core/src/goal.rs:170`) walks
the criteria and returns a `CriterionResult` for each deterministic criterion,
skipping `LlmJudge` and `Custom`. `Goal::compute_evaluation` combines a full
set of criterion results with a list of constraint violations and produces a
`GoalEvaluation` whose `passed` flag is true only when no hard constraint is
violated **and** the weighted score clears the threshold. Weights are
normalized across whichever criteria were evaluated, so a run where only
half the criteria ran (for example because the LLM judge errored out) still
produces a meaningful score on the criteria that did run.

`GoalObject` wraps a `Goal` with a `failure_history: Vec<SemanticFailure>`
and an `evolution_count`. `SemanticFailure` tags a failure with a
`FailureCategory` (`LogicContradiction`, `VelocityDrift`,
`ConstraintViolation`, `FailureAccumulation`, `QualityDeficiency`) so the
Director can decide whether to retry, reframe, or escalate. See
[../internals/director-ooda.md](../internals/director-ooda.md) for the
full evolution flow and ADR-009 for the OODA rationale.

The `Verdict` enum in `types.rs` is the Judge's output: `Accept {
confidence }` ends the run, `Retry { reason, hint }` lets the agent loop
continue with a hint injected into the next turn, `Escalate { reason }`
notifies the user that the goal is unreachable, and `Continue` is a
no-opinion verdict used when the Judge has insufficient signal. See
[../internals/judge.md](../internals/judge.md) for how the Judge composes
deterministic and LLM evaluation passes.

## Security primitives

Ryvos switched to
**[passthrough security](../glossary.md#passthrough-security)** in v0.6.0.
`ryvos-core` retains the `SecurityTier` enum (`T0` through `T4`) because
tool trait signatures and the tool-registry TOML still carry the tier as
informational metadata. `SecurityPolicy` reflects the new model:
`auto_approve_up_to` and `deny_above` are deprecated fields kept for config
compatibility, `approval_timeout_secs` governs the
**[soft-checkpoint](../glossary.md#soft-checkpoint)** timeout, and
`pause_before: Vec<String>` is the opt-in list of tool names that should
trigger a human checkpoint before execution. `SecurityPolicy::should_pause`
at `crates/ryvos-core/src/security.rs:112` is the predicate the
**[security gate](../glossary.md#security-gate)** consults per tool call.

`tool_has_side_effects` is a hand-maintained match over tool names that
returns `true` for any tool whose execution produces a persistent change on
the system (bash, write, edit, file operations, git mutations, HTTP writes,
SQL mutations, agent spawns, cron changes, Viking writes, and so on).
Safety code uses it to decide whether an action is worth recording in
**[SafetyMemory](../glossary.md#safetymemory)**.

`summarize_input` produces a one-line display form of a tool's input JSON
for the audit log and soft-checkpoint UI. It special-cases a handful of
common tool shapes (`bash` extracts `command`, `write`/`edit` extract
`file_path`, `web_search` extracts `query`, `spawn_agent` extracts the first
80 characters of `prompt`) and falls back to a 120-character-truncated JSON
dump. The rest of the safety pipeline lives in `ryvos-agent`; see ADR-002
for the full deprecation and passthrough rationale.

## Invariants and threading model

`ryvos-core` is a library of pure data types and traits. The only
concurrency primitive it owns is the `tokio::broadcast::Sender` inside
`EventBus`, and that channel is cloneable and `Send + Sync`. Every trait is
`Send + Sync + 'static` so that trait objects can be stored inside `Arc`
containers and shared across spawned tasks. There are no `RefCell`s or
thread-local statics anywhere in the crate.

The hook runner (`run_hooks`) is the only function in the crate that
spawns a subprocess or does I/O, and it deliberately discards all output
and errors. Everything else — config parsing, goal evaluation, filter
matching, summarize helpers — is synchronous pure Rust.

Three gotchas are worth flagging for contributors:

First, adding a new `AgentEvent` variant requires updating two match
statements in `crates/ryvos-core/src/event.rs` (`extract_session_id` and
`event_type_name`). Missing the first silently breaks session filtering;
missing the second silently breaks type filtering. There is no exhaustive
match check for this today.

Second, `ToolContext::viking_client` is typed `Option<Arc<dyn Any + Send +
Sync>>` deliberately — typing it as an `Arc<dyn VikingClient>` would pull
`ryvos-memory` into `ryvos-core` and violate the layer order. Tools that
need Viking downcast with `Arc::downcast` on the consumer side.

Third, the config module uses `#[serde(default)]` aggressively, which means
that deleting a field from a struct does not break existing `ryvos.toml`
files but silently restores the default. A breaking config change has to be
announced in the `CHANGELOG.md`; there is no automatic migration layer.

## Cross references

- ADR-001 records the choice of Rust for the runtime. See
  [../adr/001-rust-runtime.md](../adr/001-rust-runtime.md).
- ADR-002 explains the passthrough security model and the
  `SecurityTier` deprecation. See
  [../adr/002-passthrough-security.md](../adr/002-passthrough-security.md).
- ADR-005 covers the event-driven architecture and why the EventBus is a
  tokio broadcast channel. See
  [../adr/005-event-driven-architecture.md](../adr/005-event-driven-architecture.md).
- ADR-009 covers the Director OODA loop that consumes `Goal` and
  `GoalObject`. See
  [../adr/009-director-ooda-loop.md](../adr/009-director-ooda-loop.md).
- The agent loop that drives these types end-to-end is documented in
  [../internals/agent-loop.md](../internals/agent-loop.md).
- The full EventBus delivery semantics, including slow-subscriber
  behavior, are in
  [../internals/event-bus.md](../internals/event-bus.md).
- The static layout of the workspace is in
  [../architecture/system-overview.md](../architecture/system-overview.md).
- The config tree is documented per-key in
  [../operations/configuration.md](../operations/configuration.md).
- Environment-variable expansion is described in
  [../operations/environment-variables.md](../operations/environment-variables.md).
