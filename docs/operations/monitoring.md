# Monitoring

Ryvos surfaces runtime state through three channels: REST endpoints on the
gateway for poll-based checks, the WebSocket event stream for push-based
monitoring, and structured tracing on stdout for log aggregation. This
document maps the operator questions — is it running, how much is it
spending, what went wrong — onto the specific endpoints and log streams
that answer them.

Everything described here runs inside a single `ryvos daemon --gateway`
process. There is no separate metrics exporter, no second port, and no
push agent. External systems that need Ryvos data either scrape the REST
endpoints or subscribe to the WebSocket lane.

## Health check

`GET /api/health` is unauthenticated and returns a small JSON document on
success. It is the endpoint the container `HEALTHCHECK` in the Dockerfile
(`Dockerfile:59`) polls every thirty seconds.

```bash
curl -s http://localhost:18789/api/health
```

```json
{
  "status": "ok",
  "version": "0.8.3",
  "uptime_secs": 12345
}
```

This endpoint is deliberately dumb — it answers "is the HTTP listener up
and the version I expected" and nothing more. Use it for liveness probes,
not for deep health. See
[../api/gateway-rest.md](../api/gateway-rest.md) for the full endpoint
schema.

## Metrics

`GET /api/metrics` requires at least Viewer access (any configured API key
role is sufficient; see [../api/auth-and-rbac.md](../api/auth-and-rbac.md))
and returns an aggregate JSON document covering the whole daemon instance.
The shape is assembled in `crates/ryvos-gateway/src/routes.rs:184`.

| Field | Source | Description |
|---|---|---|
| `total_runs` | `cost.db` `run_log` | Total runs recorded since the cost store was created. |
| `active_sessions` | `SessionManager` | Sessions currently held in memory (any channel). |
| `total_tokens` | `cost.db` sum of input + output | Cumulative LLM token consumption. |
| `total_cost_cents` | `cost.db` sum of `cost_cents` | Cumulative dollar-equivalent cost. |
| `monthly_budget_cents` | `[budget].monthly_budget_cents` | Configured ceiling (0 means unlimited). |
| `budget_utilization_pct` | derived | `(total_cost_cents / monthly_budget_cents) * 100`. |
| `uptime_secs` | `start_time` on `AppState` | Seconds since the gateway was constructed. |

The metrics endpoint is the recommended scrape target for external
monitoring. A thirty-second scrape interval is typical. Alert rules worth
configuring:

- `budget_utilization_pct > 90` — the Guardian will hard-stop at 100, so
  90 is the correct warning level.
- `active_sessions == 0` for an extended window when traffic is expected.
- `uptime_secs < 60` — flapping daemon.

Prometheus does not have a native consumer for this shape. A minimal
shim — a cron job that converts the JSON into the Prometheus text format
and writes it to a `node_exporter` textfile — is enough for anyone
already running Prometheus. There is no plan to add a Prometheus exporter
directly to the gateway; the JSON endpoint is the stable contract.

## Audit stats

`GET /api/audit/stats` returns aggregate counts over the **[audit trail](../glossary.md#audit-trail)**.

| Field | Description |
|---|---|
| `total_entries` | Row count of `audit_log`. |
| `tool_breakdown` | `HashMap<String, u64>` of tool name to invocation count. |
| `heartbeat_sessions` | Distinct sessions that fired a **[Heartbeat](../glossary.md#heartbeat)** event. |
| `viking_entries` | Distinct `viking_*` tool invocations. |

This endpoint is how the Web UI dashboard populates the tool-usage chart;
it is also a useful alert source for "tool X is being called more than
expected". Because the audit trail is append-only, counts are monotonic
and delta-friendly.

## Cost and run endpoints

`GET /api/costs` accepts `group_by={model,provider,day}` and `from`/`to`
ISO-8601 parameters. The response is a grouped aggregation of
`cost_events`. The shape is documented in
[../api/gateway-rest.md](../api/gateway-rest.md) and matches the query
that the Web UI's Billing page uses.

`GET /api/runs` returns paginated run history from `cost.db` `run_log`,
joined in Rust with `audit.db` entries for associated tool calls. Useful
parameters:

| Parameter | Default | Description |
|---|---|---|
| `limit` | `50` | Max rows per page. |
| `offset` | `0` | Pagination offset. |
| `session_id` | none | Filter to a single session. |
| `status` | none | `running`, `completed`, or `failed`. |

Runs in `running` status that have not transitioned after a long window
are a sign that the Guardian missed a stall or the daemon was killed
mid-run. A periodic sweep that counts `running` runs older than the
Guardian stall timeout is a good sanity check.

## Tracing logs

Tracing is initialized in `src/main.rs:201` with
`EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("ryvos=info,warn"))`.
The daemon emits structured tracing events to stdout; systemd and launchd
capture them into journal and file respectively.

| Command | Purpose |
|---|---|
| `journalctl --user -u ryvos -f` | Follow the systemd journal for the user unit. |
| `tail -f ~/.ryvos/daemon.log` | Follow the launchd stdout redirect. |
| `docker logs -f ryvos` | Follow the container's stdout. |

Useful filters:

- `RUST_LOG=ryvos=debug` — every Ryvos crate at debug.
- `RUST_LOG=ryvos_agent=trace,ryvos_llm=debug,info` — deep agent loop
  with LLM request details.
- `RUST_LOG=ryvos_gateway=debug` — request routing and auth decisions.

The tracing layer emits span-structured events; a downstream log
aggregator (Loki, CloudWatch, Elasticsearch) ingests the output
unmodified. The `target=false` setting in the initializer suppresses
crate names from the line prefix, keeping lines short.

## JSONL run logs

The **[RunLogger](../internals/agent-loop.md)** writes a separate JSONL
stream for every run, controlled by `[agent.log]`. Layout:

```text
<log_dir>/<session_id>/<timestamp>.jsonl
```

Level 1 logs one line per run (summary). Level 2 (the default) logs one
line per turn. Level 3 logs one line per step, including tool call
arguments and results. Each line is a JSON object with keys like
`event`, `ts`, `session_id`, `run_id`, `turn`, and whatever payload the
event variant carries.

For a single-run replay, `jq` is the right tool:

```bash
jq -c 'select(.event=="ToolEnd")' ~/.ryvos/logs/<session>/<ts>.jsonl
```

Debugging patterns built on top of this feed are covered in
[../guides/debugging-runs.md](../guides/debugging-runs.md).

## WebSocket event stream

`wss://host:18789/ws` is a bidirectional lane into the
**[EventBus](../glossary.md#eventbus)**. Clients authenticate with a
bearer token (any role), subscribe to filters, and receive every
`AgentEvent` variant in real time. The frame format and subscription
grammar are specified in
[../api/gateway-websocket.md](../api/gateway-websocket.md).

Every event relevant to monitoring flows through this lane:

| Event | Meaning |
|---|---|
| `ToolStart` / `ToolEnd` | Individual tool invocations with inputs and outputs. |
| `RunComplete` | Final status of a run plus token totals. |
| `RunError` | Run terminated in a failure state. |
| `GuardianStall` | Guardian detected no progress for `stall_timeout_secs`. |
| `GuardianDoomLoop` | Guardian detected a **[doom loop](../glossary.md#doom-loop)**. |
| `GuardianBudgetAlert` | Token or dollar budget warning or hard-stop. |
| `HeartbeatOk` | Heartbeat tick completed with no actionable finding (suppressed from channels). |
| `HeartbeatAlert` | Heartbeat tick found something actionable; routed to channels. |
| `GoalEvaluated` | Judge returned a goal evaluation. |
| `JudgeVerdict` | Explicit verdict (Accept, Retry, Escalate, Continue). |

A scripted subscriber that listens for the three `Guardian*` variants and
forwards them to a pager is the cleanest way to alert on agent
misbehavior. The event stream is push-based and does not impose the
poll-interval latency of the REST endpoints.

## Guardian and heartbeat signals

The Guardian (see [../internals/guardian.md](../internals/guardian.md)) is
the first line of runtime health monitoring. A Guardian event signals one
of three conditions:

- **Stall.** The agent published no activity within `stall_timeout_secs`.
  Usually indicates an LLM hang, a network blackhole, or a tool
  subprocess that blocked.
- **Doom loop.** The agent called the same tool with the same arguments
  more than `doom_loop_threshold` times in a row. Usually indicates a
  tool that is failing to make progress.
- **Budget alert.** Token or dollar budget exceeded the configured soft
  or hard thresholds. Hard stops cancel the run.

The Heartbeat (see [../internals/heartbeat.md](../internals/heartbeat.md))
fires every `interval_secs` and publishes either `HeartbeatOk` (green,
suppressed from channels) or `HeartbeatAlert` (red, routed to the target
channel). A monitoring system that watches the event stream should alert
on sustained absence of `HeartbeatOk` — silence is a worse signal than an
alert.

## External monitoring integration

The pattern for integrating an external system (Datadog, Grafana,
Uptime Kuma) is:

1. Scrape `/api/metrics` every 30 s for cost and uptime numbers.
2. Scrape `/api/audit/stats` every 5 min for tool usage trends.
3. Subscribe to `/ws` for real-time Guardian and Heartbeat events.
4. Alert on `budget_utilization_pct > 90`, `RunError` bursts, and
   `GuardianBudgetAlert { is_hard_stop: true }`.

The REST endpoints are idempotent and read-only — a tight poll interval
has no side effects beyond SQLite page cache churn. The WebSocket stream
is bounded to 256 slots per subscriber; a slow consumer drops events
rather than blocking the daemon.

Cross-references:
[../api/gateway-rest.md](../api/gateway-rest.md),
[../api/gateway-websocket.md](../api/gateway-websocket.md),
[../internals/event-bus.md](../internals/event-bus.md),
[../guides/debugging-runs.md](../guides/debugging-runs.md).
