# Debugging runs

## When to use this guide

When an agent **[run](../glossary.md#run)** produces a wrong answer,
stalls, loops on the same tool, breaches a budget, or picks a tool
you did not expect, Ryvos gives you three overlapping data sources to
diagnose what happened: the **JSONL run log**, the **audit trail**,
and the **decision journal**. Each captures a different slice of the
run's state, and together they are enough to reconstruct any past
execution.

Use this guide when a specific run went sideways and you need to know
why. It is not the right starting point for live monitoring — for
that, read
[../operations/monitoring.md](../operations/monitoring.md). This guide
walks each data source, explains the event types to filter on, and
shows the workflow for narrowing from "something broke" to "this
specific tool call produced this specific error."

## Three sources at a glance

| Source | Format | Lifetime | Granularity | Best for |
|---|---|---|---|---|
| JSONL run log | line-delimited JSON in `{log_dir}/{session_id}/{timestamp}.jsonl` | per run, append-only | one line per event | post-hoc reconstruction, grep-based search, tail -f |
| Audit trail | SQLite in `audit.db` | persistent across restarts | one row per tool call | tool-centric queries, outcome classification, historical aggregation |
| Decision journal | SQLite in `healing.db` | persistent across restarts | one row per tool-choice decision | pattern detection, Reflexion hints, understanding why the agent picked X |

## The JSONL run log

The `RunLogger` in `crates/ryvos-agent/src/run_log.rs` is a background
task that subscribes to the `EventBus` and writes one JSON object per
line to `{log_dir}/{session_id}/{timestamp}.jsonl`. The append-only
format is deliberately crash-resilient: even if the daemon dies
mid-run, every previously flushed line is still a valid JSON
document and the log can be parsed incrementally.

Configuration lives under `[agent.log]` in `ryvos.toml`:

```toml
[agent.log]
log_dir = "~/.ryvos/logs"
level = 2
```

`level` has three values:

- **`1`** — summary only. One line at `RunStarted`, one at
  `RunComplete` or `RunError`, plus any `GuardianStall`,
  `GuardianDoomLoop`, `GuardianBudgetAlert`, `JudgeVerdict`, or
  `BudgetExceeded` events. Cheap enough to always leave on.
- **`2`** — per-turn. Adds `TurnComplete`, `ToolStart`, `ToolEnd`,
  `UsageUpdate`, and `ApprovalRequested` events. This is the default
  and what most debugging starts from.
- **`3`** — per-step. Adds `TextDelta` and `ThinkingDelta` events, so
  every token the model streams shows up in the log. Verbose and
  rarely needed, but essential when you need to see exactly what the
  model was generating when a tool call happened.

### Reading a JSONL run

The log directory layout is one subdirectory per session, one file
per run:

```text
~/.ryvos/logs/
  telegram:user:12345/
    2026-04-11T14-22-05.jsonl
    2026-04-11T15-01-33.jsonl
  discord:channel:999:user:12345/
    2026-04-11T14-45-12.jsonl
```

Each line is a JSON object with a common envelope and a
variant-specific body:

```json
{"ts":"2026-04-11T14:22:05.123Z","run_id":"a1b2...","event_type":"RunStarted","session_id":"telegram:user:12345"}
{"ts":"2026-04-11T14:22:05.418Z","run_id":"a1b2...","event_type":"ToolStart","tool_name":"bash","input_summary":"command=ls /tmp"}
{"ts":"2026-04-11T14:22:05.721Z","run_id":"a1b2...","event_type":"ToolEnd","tool_name":"bash","duration_ms":303,"is_error":false}
{"ts":"2026-04-11T14:22:06.002Z","run_id":"a1b2...","event_type":"RunComplete","turns":1,"tokens_in":1245,"tokens_out":87}
```

Common filters during debugging:

- **Tool errors**: `event_type == "ToolEnd"` with `is_error: true`.
- **Model reasoning before a bad tool pick**: at level 3, filter for
  `ThinkingDelta` events in the window between the previous
  `TurnComplete` and the offending `ToolStart`.
- **Doom loop**: repeated `ToolStart` events with identical
  `tool_name` and `input_summary`. The
  **[Guardian](../glossary.md#guardian)** emits `GuardianDoomLoop`
  when it notices the pattern itself.
- **Budget issues**: `GuardianBudgetAlert` events correlated with
  cumulative token counts on `UsageUpdate`.

Any JSON-aware tool works: `jq`, the gateway's `/api/runs/{id}/log`
endpoint, or a tail pipe into `less`.

## The audit trail

`audit.db` is the persistent record of every tool call ever made. The
schema has one row per `AuditEntry` with a timestamp, session id,
tool name, one-line input summary, one-line output summary, a safety
reasoning string, the resolved `SafetyOutcome`, and the list of
lesson ids that were available when the call was dispatched. Indexes
on `session_id`, `tool_name`, and `timestamp DESC` make the common
queries cheap.

Three ways to read it:

- **CLI**: `ryvos audit stats` prints per-tool counts and the
  distribution of outcomes. `ryvos audit query --tool bash` filters
  to one tool. `ryvos audit query --session {id}` filters to one
  session. `ryvos audit query --since 2026-04-01` limits by date.
- **Web UI**: the `/audit` page shows the same data with sortable
  columns and live updates as new entries land. Filter widgets cover
  tool name, outcome, and session.
- **MCP**: the `audit_query` and `audit_stats` tools Ryvos exposes
  when it runs as an MCP server let external clients (Claude Code,
  Claude Desktop) read the trail through the same interface.

The audit trail is append-only and never rewritten. A row is the
definitive record of what happened; nothing in the daemon mutates or
deletes it. `SafetyMemory` reads the audit trail to seed its lessons,
but writes its own `safety.db` rather than modifying the audit rows.

## The decision journal and failure journal

`healing.db` hosts two related tables:

- **`failure_journal`** — one row per failed tool call, with the
  tool name, error summary, and the turn it happened on. The
  `FailureJournal::find_patterns` method scans recent rows to detect
  repeat-failure patterns (the same tool failing with the same error
  three times in a row), which is what drives **[Reflexion](../glossary.md#reflexion)**
  hint injection.
- **`decisions`** — one row per tool-choice decision, with the tool
  the agent picked, the alternatives it considered, and the eventual
  outcome. This is the answer to "why did the agent choose X instead
  of Y". The REST endpoint `/api/decisions` and the Web UI Decisions
  page both read this table.

Reflexion works by reading `failure_journal`, building a
`ChatMessage` that summarizes the last three matching past failures,
and injecting it before the next turn when an in-memory
`FailureTracker` has seen the same tool fail at least `N` times in a
row. The hint is advisory — nothing blocks and nothing retries
automatically. See
[../internals/reflexion.md](../internals/reflexion.md) for the
trigger logic.

## Guardian events

The **[Guardian](../glossary.md#guardian)** publishes three event
types when a run is going sideways:

- **`GuardianStall`** — no progress for `stall_timeout_secs`. Repeated
  stalls usually mean a tool is hung upstream (child process,
  network).
- **`GuardianDoomLoop`** — the same tool has been called with the
  same arguments `doom_loop_threshold` times in a row. Usually means
  the tool reports success on a no-op that the agent reads as
  progress.
- **`GuardianBudgetAlert`** — token or dollar budget crossed the
  warn or hard-stop threshold. The hard-stop path fires the shared
  `CancellationToken` and translates into an early `RunError`.

All three land in the run log and on the `EventBus`; the Web UI
highlights them on the Runs page.

## Judge verdicts

For goal-driven runs, `JudgeVerdict` events capture the
**[Judge](../glossary.md#judge)**'s call-by-call evaluation. The four
variants are `Accept(confidence)`, `Retry(reason, hint)`,
`Escalate(reason)`, and `Continue`. Filter the log for
`JudgeVerdict` in chronological order to see how confidence moved
through the run — a run that ends in `Escalate` usually has a trail
of `Retry` verdicts with the same `reason` repeated, meaning plan
evolution did not find a way out.

## Typical debugging workflow

1. **Find the session and run id** from the Web UI Runs page or the
   audit trail. Note the `run_id` and `session_id`.
2. **Read the JSONL.** Open
   `~/.ryvos/logs/{session_id}/{timestamp}.jsonl`; `RunStarted` marks
   the beginning, `RunComplete` or `RunError` the end.
3. **Find the first anomaly** — the first `ToolEnd` with
   `is_error: true`, the first `GuardianStall`/`GuardianDoomLoop`, or
   the first `JudgeVerdict` that shifted to `Retry` or `Escalate`.
4. **Correlate with the audit trail** via
   `ryvos audit query --session {id}`. The audit row has more context
   than the log line (safety reasoning, lesson ids, outcome
   classification).
5. **Check the decision journal.** If the agent picked the wrong
   tool, `/api/decisions` shows the alternatives it considered. If
   the right tool was never in the candidate set, the problem is
   context-level (missing schema, stale system prompt) rather than
   reasoning-level.
6. **Check SafetyMemory.** A lesson in `safety.db` that is actively
   pushing the agent away from the correct action is worth pruning.
7. **Raise the log level to 3** and re-run. The per-token deltas show
   exactly what the model was generating when the anomaly occurred.

## Verification

1. Run a turn that calls at least one tool; confirm the JSONL file
   under `~/.ryvos/logs/{session}/{timestamp}.jsonl` contains
   `RunStarted` through `RunComplete`.
2. `ryvos audit query --session {id}` lists the tool calls in the
   same order as the log.
3. The Web UI Runs page matches the historical JSONL view once
   `RunComplete` fires.
4. For goal-driven runs, `JudgeVerdict` events are present and
   `EvolutionTriggered` fires on plan evolution.

For the agent loop state machine these events come from, read
[../internals/agent-loop.md](../internals/agent-loop.md). For the
Reflexion pattern and its triggers, read
[../internals/reflexion.md](../internals/reflexion.md). For the
full crate reference, read
[../crates/ryvos-agent.md](../crates/ryvos-agent.md). The REST
endpoints for programmatic log and decision access are in
[../api/gateway-rest.md](../api/gateway-rest.md). For live
monitoring dashboards and metric exports, read
[../operations/monitoring.md](../operations/monitoring.md).
