# Glossary

Canonical vocabulary for the Ryvos codebase. Every term introduced in the source,
design documents, or ADRs is defined here exactly once. All other docs link back
to this file rather than redefining terms inline.

Terms are listed alphabetically. Each heading is stable so that cross-links of
the form `../glossary.md#term-name` remain valid across reorganizations.

## Agent runtime

The in-process ReAct loop that streams messages from an LLM, executes tool calls,
prunes context, and emits events. Implemented by `AgentRuntime` in `crates/ryvos-agent/src/agent_loop.rs`.
The agent runtime is the default execution model for every incoming user message;
the [Director](#director) replaces it only when a goal is attached to the run.

## API billing

Metered per-token billing. A provider is classified as *API billing* when it
reports input and output token counts that map to a per-token price. Anthropic,
OpenAI, Gemini, Cohere, Groq, OpenRouter, and Bedrock are API-billed. Usage is
accumulated in `cost.db` and enforced by the monthly budget. See [CLI provider](#cli-provider)
for the contrasting case.

## Approval broker

`ApprovalBroker` coordinates human checkpoints for tools listed under `pause_before`
in the config. It publishes an `ApprovalRequested` event on the [EventBus](#eventbus),
waits for a decision from any channel (Telegram, Discord, Slack, WhatsApp, Web UI,
or TUI), and releases the pending tool call. It is strictly opt-in; Ryvos never
requires approval by default.

## AGENTS.toml

Repository-local agent profile. Declares which tools are available, which system
prompt fragments to load, and optional defaults. Part of [narrative context](#onion-context).

## Audit trail

Append-only SQLite log of every tool invocation, its arguments, its result, and
the safety reasoning attached to it. Stored in `audit.db`. Written by `AuditTrail`
in `crates/ryvos-agent/src/audit.rs`. The audit trail is the primary post-incident
analysis tool and is also the raw material for [SafetyMemory](#safetymemory) learning.

## Billing type

Classification of an LLM provider as either [API](#api-billing) or [Subscription](#subscription-billing).
Detected automatically from the provider configuration. Surfaces through cost
tracking so that subscription-billed runs report `$0.00` for token spend.

## Channel adapter

A trait implementation that bridges Ryvos to an external messaging platform. The
four built-in adapters are Telegram, Discord, Slack, and WhatsApp. Each implements
`ChannelAdapter` from `ryvos-core` and lives in `crates/ryvos-channels`. Adapters
are responsible for inbound message translation, outbound response formatting,
and per-platform approval UI. See ADR-010 for the pattern rationale.

## Checkpoint

A per-turn snapshot of the session's messages, tool results, and accumulated
token counts, persisted to the `checkpoints` table in `sessions.db`. If the
process crashes mid-run, the next start reloads the checkpoint and resumes the
conversation with no context loss. Managed by `CheckpointStore`. Not to be
confused with a [soft checkpoint](#soft-checkpoint).

## CLI provider

An LLM provider that is implemented by spawning and communicating with a local
CLI subprocess rather than hitting an HTTP endpoint. The two supported CLI
providers are `claude-code` and `copilot`. Both are [subscription-billed](#subscription-billing)
because the billing relationship is between the user and the upstream vendor.
See ADR-004 for the full design.

## Constitutional AI

Ryvos's safety philosophy: the agent reasons about every action against eight
built-in principles (Preservation, Intent Match, Proportionality, Transparency,
Boundaries, Secrets, Learning, External Data). These principles are injected at the top of the
system prompt and reinforced by [SafetyMemory](#safetymemory) lessons. Constitutional
AI is the opposite of classification-based blocking; the LLM's own judgment is
the first line of defense. See ADR-002.

## Director

The goal-driven orchestrator implemented by `Director` in `crates/ryvos-agent/src/director.rs`.
The Director runs an [OODA](#ooda) loop: it observes the current state, generates
a DAG of subtasks, decides which nodes are ready, executes them (potentially in
parallel), evaluates outcomes, diagnoses failures, and evolves the plan up to
three cycles before escalating. When a run has an attached [goal](#goal), the
Director replaces the standard agent loop for that run. See ADR-009.

## Doom loop

Pathological pattern in which the agent calls the same tool with the same
arguments repeatedly, making no progress. Detected by the [Guardian](#guardian)
via JSON fingerprinting over a rolling window of recent tool calls. When
triggered, the Guardian publishes a `DoomLoopDetected` event and injects a
corrective hint into the next turn's context.

## EventBus

A central `tokio::sync::broadcast` channel (capacity 256) through which every
subsystem publishes and consumes lifecycle events. Defined as `EventBus` in
`crates/ryvos-core/src/event.rs`. Events are typed enums with payloads; any
component can subscribe with a filter. See ADR-005 for the motivation and the
tradeoffs of broadcast delivery (notably: slow subscribers drop messages).

## Failure journal

Tracks tool failures per session, detects repeat patterns, and feeds [Reflexion](#reflexion)
hints back into subsequent turns. Implemented by `FailureJournal` in
`crates/ryvos-agent/src/healing.rs` and backed by `healing.db`.

## Focus layer

The third layer of the [onion context](#onion-context). Contains the immediate
goal, hard and soft constraints, and any just-in-time tool documentation for
the current turn.

## Goal

A structured task definition with weighted success criteria (`OutputContains`,
`OutputEquals`, `LlmJudge`, `Custom`) and optional constraints on time, cost,
safety, scope, and quality. When a run has a goal, the [Director](#director)
takes over. The [Judge](#judge) evaluates outcomes against the goal's criteria.

## Guardian

Background watchdog subscribed to the [EventBus](#eventbus). Implemented by
`Guardian` in `crates/ryvos-agent/src/guardian.rs`. The Guardian detects three
failure modes: [doom loops](#doom-loop), stalls (no activity for
`stall_timeout_secs`), and budget exhaustion (token or dollar). On detection it
publishes a typed event and, where applicable, injects a corrective hint. The
Guardian is event-driven and runs for the lifetime of the daemon.

## Heartbeat

A periodic self-check the agent runs every `N` seconds (default 1800). Reads
`HEARTBEAT.md` as its prompt, reviews the workspace, reports concrete findings,
and suppresses "all good" acks so users are not flooded. Implemented by
`Heartbeat` in `crates/ryvos-agent/src/heartbeat.rs`. Timer-driven, not
event-driven.

## Identity layer

The first and innermost layer of the [onion context](#onion-context). Assembled
from `SOUL.md`, `IDENTITY.md`, and core operator metadata. Answers "who is this
agent and who does it serve?". Loaded once per run and never pruned.

## Judge

The two-level goal evaluator. Level 0 is fast and deterministic, running
`OutputContains` / `OutputEquals` checks against the final assistant message.
Level 2 is slow and delegates to an LLM that returns a [Verdict](#verdict). The
Judge is called at the end of every Director cycle. Implemented by `Judge` in
`crates/ryvos-agent/src/judge.rs`.

## Lane

A per-WebSocket-connection FIFO queue inside the gateway. Each connected client
gets its own bounded channel (buffer 32) so requests from a single connection
are serialized, while different connections are processed concurrently. Prevents
a slow client from stalling the whole gateway.

## MCP

Model Context Protocol. An open standard for connecting LLM agents to external
tools and data sources. Ryvos is both an MCP **client** (it can connect to any
MCP server over stdio or streamable HTTP) and an MCP **server** (it exposes
nine of its own tools over the same protocol). See ADR-008.

## Narrative layer

The second layer of the [onion context](#onion-context). Contains the recent
past and sustained memory: `AGENTS.toml`, `TOOLS.md`, daily logs, [Viking](#viking)
recall fragments, and compacted conversation history.

## Onion context

Ryvos's three-layer system prompt assembly. Layers from innermost to outermost:
[Identity](#identity-layer), [Narrative](#narrative-layer), [Focus](#focus-layer).
Built fresh at the start of every turn by the context module in `crates/ryvos-agent/src/context.rs`.
The onion metaphor captures the fact that inner layers rarely change while outer
layers are rebuilt frequently.

## OODA

Observe, Orient, Decide, Act. A decision-making loop originally from military
strategy, adapted as the execution model for the [Director](#director). Each
phase corresponds to a distinct LLM interaction or deterministic computation.
See ADR-009.

## Passthrough security

Ryvos's safety model since v0.6.0. No tool is blocked. Safety is provided by
four mechanisms working together: [constitutional AI](#constitutional-ai),
[SafetyMemory](#safetymemory), [Reflexion](#reflexion), and the full
[audit trail](#audit-trail). Users may optionally opt in to [soft checkpoints](#soft-checkpoint)
for specific tools. See ADR-002.

## Prime

`PrimeOrchestrator` — the subsystem that spawns restricted sub-agents with
tighter security policies than the main agent. Used when delegating a task to a
short-lived agent that should not inherit full tool access.

## ReAct

Reasoning + Acting loop. The LLM streams tokens (thinking + text), emits tool
calls, receives tool results as new messages, and continues until the model
emits a stop without further tool calls. Ryvos implements ReAct with streaming,
parallel tool dispatch, and per-turn context re-pruning.

## Reflexion

Self-correction pattern. After `N` consecutive failures of the same tool in a
session, Ryvos injects a structured hint message that summarizes the past
failures and suggests a different approach. Fed by the [failure journal](#failure-journal).
Purely advisory: Reflexion never blocks execution.

## Run

A single end-to-end execution of the agent for one user message or triggered
event. Contains one or more [turns](#turn). Runs are checkpointed, audited, and
cost-tracked as a unit.

## SafetyMemory

A SQLite-backed self-learning safety database (`safety.db`). Records every
tool-call outcome as one of `Harmless`, `NearMiss`, `Incident`, or
`UserCorrected`. Useful lessons are reinforced; stale ones are pruned. Relevant
lessons are injected into the system prompt before similar future actions.
Implemented by `SafetyMemory` in `crates/ryvos-agent/src/safety_memory.rs`.

## Security gate

`SecurityGate` in `crates/ryvos-agent/src/gate.rs`. The passthrough checkpoint
every tool call passes through. It audits the call, consults [SafetyMemory](#safetymemory),
optionally triggers a [soft checkpoint](#soft-checkpoint), and releases the call.
It never blocks based on classification alone.

## Session

A logically distinct conversation, identified by a `SessionId`. Persisted in
`sessions.db` across process restarts. Each channel, API client, or TUI
invocation gets its own session.

## Skill

A drop-in plugin packaged as a TOML manifest plus Lua or Rhai script, loaded
from `~/.ryvos/skills/`. Skills appear as tools in the [tool registry](#tool-registry)
once validated. Implemented by `crates/ryvos-skills`.

## Soft checkpoint

An optional per-tool pause configured via `pause_before` in the config. When
triggered, the tool call is held, an approval request is published on the
[EventBus](#eventbus), and the call proceeds only after a human responds. Opt-in;
not to be confused with the automatic [checkpoint](#checkpoint) used for resume.

## SOUL.md

The agent's personality file, generated during the `ryvos soul` onboarding
interview. Shapes tone, proactivity, and operator context. Loaded into the
[identity layer](#identity-layer) of every run. Fifteen questions total: four
about the user, five about tone, three about projects, three about character.

## Subscription billing

Flat-fee billing. A provider is classified as *subscription-billed* when usage
is not metered per token. The two subscription-billed providers are `claude-code`
and `copilot`; both are [CLI providers](#cli-provider). Their runs show `$0.00`
for token spend even though real LLM calls are made.

## T0–T4

Deprecated security tiers from the pre-v0.6 blocking security model.
`T0` = read-only, `T1` = write, `T2` = mutating, `T3` = dangerous, `T4` = system.
Retained as informational metadata on every tool but no longer gate execution.
Current security is [passthrough](#passthrough-security). See ADR-002 for the
deprecation rationale.

## Tool registry

The central collection of all tools available to a run: built-in tools (70+
across 12 categories), [skills](#skill), and tools proxied from [MCP](#mcp)
servers. Provided by `crates/ryvos-tools`. Each tool implements the `Tool`
trait from `ryvos-core`.

## Turn

One round trip of the agent loop: user or tool-result input, LLM stream, tool
batch execution, context re-prune. A [run](#run) consists of one or more turns.
`max_turns` is a hard cap per run (default 25).

## Verdict

The output of the [Judge](#judge). Four variants:
- `Accept(confidence)` — goal satisfied, end the run.
- `Retry(reason, hint)` — not yet satisfied, continue with a hint.
- `Escalate(reason)` — stuck in a way the agent cannot resolve; notify the user.
- `Continue` — no opinion yet, keep going.

## Viking

Hierarchical persistent memory system with three context levels: `L0` summary,
`L1` details, `L2` full. Entries are addressed with `viking://` URIs such as
`viking://user/profile/` or `viking://agent/patterns/`. Inspired by ByteDance's
OpenViking. Two deployment modes: local SQLite via `VikingStore` in `viking.db`,
or a standalone HTTP server (`ryvos viking-server`) on port 1933. See ADR-003.
