# Gateway REST API

The `ryvos-gateway` crate exposes an Axum HTTP server that wraps the
**[agent runtime](../glossary.md#agent-runtime)**, the
**[EventBus](../glossary.md#eventbus)**, the session store, and the optional
dashboard subsystems (cost, audit, Viking, integrations) behind forty-plus
REST endpoints. This document is the wire-level contract for every one of
those endpoints: the URL, the method, the required role, the query or body
shape, the response shape, and a concrete `curl` example.

The gateway is designed around a single binary and a single port. A fresh
`ryvos serve` binds to `127.0.0.1:18789` by default; the bind address comes
from `gateway.bind` in `ryvos.toml`. There is no separate admin port and no
hidden debug endpoint. Every route below is served from the same
`TcpListener`, and the full router definition lives in
`crates/ryvos-gateway/src/server.rs:137`.

A permissive `tower_http::cors::CorsLayer` is attached at the top of the
router, so a browser hosted on another origin (Ryvos Cloud, a local Vite
dev server, a CI dashboard) can call the API directly without a proxy.
CORS applies uniformly to every route, including the WebSocket upgrade
on `/ws` and the OAuth callback on `/api/integrations/callback`.

Handler bodies live in `crates/ryvos-gateway/src/routes.rs` and share one
pattern: extract `State<Arc<AppState>>` plus the `Authenticated` extractor,
check the role with `has_viewer_access` or `has_operator_access`, dispatch
to a collaborator through an `Arc`-wrapped field, and return
`Json<serde_json::Value>` or a typed struct. Endpoints whose collaborator
is not attached (for example, `/api/audit` with no audit trail) return
`404 Not Found` with no body.

The endpoints are grouped below by subsystem rather than alphabetically,
because the Web UI is organized the same way: the monitoring dashboard
consumes the metrics/runs/costs group, the Goals page consumes the
goals group, the Integrations page consumes the integrations group, and
so on. Within each group the read endpoints come first, followed by the
write endpoints. The role column records the minimum role required by
the handler's `has_viewer_access` or `has_operator_access` call; see
[auth-and-rbac.md](auth-and-rbac.md) for the role hierarchy and the full
permission matrix in one place.

Every endpoint's source line in the table below points at the exact
handler function. When the source and this document disagree, the
source is authoritative — the router in
`crates/ryvos-gateway/src/server.rs:137` and the handler bodies in
`crates/ryvos-gateway/src/routes.rs` are the single source of truth
for the REST contract.

## Authentication

Every protected route extracts an `Authenticated` from the request through
the extractor in `crates/ryvos-gateway/src/middleware.rs:11`. The extractor
calls `validate_auth` in `crates/ryvos-gateway/src/auth.rs:14`, which walks
a fixed four-step precedence chain. The first step that produces a decision
is authoritative; later steps are never consulted.

1. **Bearer header.** If the request carries `Authorization: Bearer <key>`,
   the value is compared against every `ApiKeyConfig` in
   `config.gateway.api_keys`. A match returns the configured role. If the
   header does not match any API key, it falls through once to the
   deprecated `gateway.token` field, which always grants `Admin`. An
   unmatched Bearer value is a hard denial; the extractor does not fall
   through to query parameters.
2. **Query `?token=...`.** If `gateway.token` is set and the Bearer header
   is absent, the `token` query parameter is checked; a match grants
   `Admin`.
3. **Query `?password=...`.** If `gateway.token` is not set but
   `gateway.password` is, the `password` query parameter is checked; a
   match grants `Admin`.
4. **Anonymous Admin.** If none of `api_keys`, `token`, or `password` is
   configured, `validate_auth` returns
   `AuthResult { name: "anonymous-admin", role: Admin }`. This is the
   default for self-hosted single-user deployments: a fresh `ryvos serve`
   with no auth configured grants the operator full access out of the box.

The three roles form a total order: `Admin > Operator > Viewer`. A
`Viewer` reads sessions, history, metrics, audit, Viking, heartbeat, and
goal history. An `Operator` adds write actions: sending messages,
responding to approvals, running goals, mutating cron/budget/model
fields, triggering webhooks, and connecting or disconnecting integrations.
An `Admin` adds the live config editor (`GET /api/config`,
`PUT /api/config`).

The canonical role matrix — including which API keys are conventionally
named `rk_*` and how to rotate them — lives in
[auth-and-rbac.md](auth-and-rbac.md). Two endpoints are deliberately
unauthenticated: `GET /api/health` and the embedded UI on `/` and
`/assets/*`. Every other route returns `401 Unauthorized` on a missing or
invalid auth, and `403 Forbidden` when the role is insufficient for the
specific handler.

## Error responses

Every REST handler returns a standard axum `StatusCode` on the error path.
The body is either empty (for 401/403/404) or a small JSON object with an
`error` field (for some dependency-missing cases). The full set in use is:

| Status | Meaning |
|---|---|
| `200 OK` | Success; body is JSON. |
| `400 Bad Request` | Malformed body (for example, `message` field empty on `POST /api/sessions/{id}/messages`). |
| `401 Unauthorized` | Authentication failed at the `validate_auth` chain. |
| `403 Forbidden` | Authenticated but the role does not satisfy `has_viewer_access` or `has_operator_access` for this handler. |
| `404 Not Found` | Either the route does not exist or an optional collaborator required by the route is not attached (for example, `/api/viking/*` with no Viking client). |
| `500 Internal Server Error` | A collaborator surfaced an error the handler could not translate; the body is empty. Indicates a bug worth filing. |

Some handlers return `200 OK` with an `error` field in the JSON body
rather than a 4xx status. This pattern is used for soft failures that
should not break a UI refresh loop (for example, a Viking search with a
malformed query, or an OAuth exchange that failed upstream).

## Health

### GET /api/health

| Field | Value |
|---|---|
| Role | none (unauthenticated) |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:19`.

Returns a static liveness probe. The only REST route that never invokes
the auth extractor. Use it for load balancer health checks and for the
pre-connect reachability probe in client libraries.

```bash
curl http://127.0.0.1:18789/api/health
```

```json
{
  "status": "ok",
  "version": "0.8.3"
}
```

## Sessions

### GET /api/sessions

| Field | Value |
|---|---|
| Role | Viewer |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:27`.

Returns the current list of **[session](../glossary.md#session)** keys
tracked by `SessionManager`. When a `SessionMetaStore` is attached, each
entry is enriched with `session_id`, `channel`, `last_active`,
`total_runs`, and `total_tokens`; otherwise only the key is returned.

```bash
curl -H "Authorization: Bearer rk_web_ui" \
  http://127.0.0.1:18789/api/sessions
```

```json
{
  "sessions": [
    {
      "id": "telegram:42",
      "session_id": "tg-abc123",
      "channel": "telegram",
      "last_active": "2026-04-10T12:34:56Z",
      "total_runs": 18,
      "total_tokens": 42380
    }
  ]
}
```

### GET /api/sessions/{id}/history

| Field | Value |
|---|---|
| Role | Viewer |
| Query | `limit` (default 50) |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:73`.

Returns the last `limit` messages for the given session key. The handler
resolves the key through `SessionMetaStore` so that a UI-friendly key like
`telegram:42` maps back to the real `SessionId`. Each message carries
`role`, `text`, and `timestamp`.

```bash
curl -H "Authorization: Bearer rk_web_ui" \
  "http://127.0.0.1:18789/api/sessions/telegram:42/history?limit=10"
```

```json
{
  "messages": [
    { "role": "user", "text": "Summarize yesterday's standup", "timestamp": "2026-04-10T09:00:00Z" },
    { "role": "assistant", "text": "Yesterday's standup covered...", "timestamp": "2026-04-10T09:00:04Z" }
  ]
}
```

### POST /api/sessions/{id}/messages

| Field | Value |
|---|---|
| Role | Operator |
| Query | — |
| Body | `{ "message": "..." }` |

Source: `crates/ryvos-gateway/src/routes.rs:119`.

Runs the agent for a session with the provided message and returns the
final text on completion. This is the REST equivalent of the WebSocket
`agent.send` RPC; use the WebSocket path instead if the client wants
streaming `text_delta` events. An empty `message` returns `400 Bad
Request`. The handler does not stream; it blocks until the agent run
finishes, and the response body is a single JSON object.

```bash
curl -X POST -H "Authorization: Bearer rk_web_ui" \
  -H "Content-Type: application/json" \
  -d '{"message":"What is 2+2?"}' \
  http://127.0.0.1:18789/api/sessions/my-session/messages
```

```json
{
  "session_id": "my-session",
  "response": "2 + 2 = 4."
}
```

On agent error, the status is still `200` but the body carries an `error`
field:

```json
{ "session_id": "my-session", "error": "context length exceeded" }
```

## Dashboard metrics

### GET /api/metrics

| Field | Value |
|---|---|
| Role | Viewer |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:175`.

Returns the rollup that the Web UI dashboard shows at the top of the
monitoring page. Fields are `total_runs`, `active_sessions`,
`total_tokens`, `total_cost_cents`, `monthly_budget_cents`,
`budget_utilization_pct`, and `uptime_secs`. When the cost store or
budget config is not attached, the corresponding fields are zero.

```json
{
  "total_runs": 1834,
  "active_sessions": 3,
  "total_tokens": 0,
  "total_cost_cents": 4212,
  "monthly_budget_cents": 10000,
  "budget_utilization_pct": 42,
  "uptime_secs": 86400
}
```

### GET /api/runs

| Field | Value |
|---|---|
| Role | Viewer |
| Query | `limit` (default 50), `offset` (default 0) |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:221`.

Paginated **[run](../glossary.md#run)** history pulled from `cost.db`
through `CostStore::run_history`. Each row describes one end-to-end
agent execution with its session ID, timing, turn count, input/output
token totals, and dollar cost. When the cost store is not attached, the
response is `{ "runs": [], "total": 0, ... }` with a `note` field
explaining that budget tracking is not configured.

```bash
curl -H "Authorization: Bearer rk_view" \
  "http://127.0.0.1:18789/api/runs?limit=5"
```

```json
{
  "runs": [
    {
      "id": 412,
      "session_id": "telegram:42",
      "model": "claude-sonnet-4-20250514",
      "provider": "anthropic",
      "total_turns": 3,
      "input_tokens": 1842,
      "output_tokens": 511,
      "cost_cents": 7,
      "started_at": "2026-04-10T12:30:00Z",
      "finished_at": "2026-04-10T12:30:12Z"
    }
  ],
  "total": 1834,
  "offset": 0,
  "limit": 5
}
```

### GET /api/costs

| Field | Value |
|---|---|
| Role | Viewer |
| Query | `from` (RFC3339), `to` (RFC3339), `group_by` (default `model`) |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:252`.

Returns the cost summary over `[from, to]` grouped by `model`, `provider`,
or `session`. Defaults are the last 30 days. The response has a `summary`
block with `total_cost_cents`, `total_input_tokens`, `total_output_tokens`,
`total_events`, and a `breakdown` array of `{key, cost_cents,
input_tokens, output_tokens}` rows. When the cost store is not attached,
every numeric field is zero and a `note` field explains why.

```bash
curl -H "Authorization: Bearer rk_view" \
  "http://127.0.0.1:18789/api/costs?group_by=provider"
```

```json
{
  "summary": {
    "total_cost_cents": 4212,
    "total_input_tokens": 184200,
    "total_output_tokens": 51100,
    "total_events": 1834
  },
  "breakdown": [
    { "key": "anthropic", "cost_cents": 3914, "input_tokens": 170100, "output_tokens": 47800 },
    { "key": "openai", "cost_cents": 298, "input_tokens": 14100, "output_tokens": 3300 }
  ],
  "from": "2026-03-11T00:00:00Z",
  "to": "2026-04-10T12:30:00Z",
  "group_by": "provider"
}
```

## Audit

### GET /api/audit

| Field | Value |
|---|---|
| Role | Viewer |
| Query | `session_id` (optional), `tool` (optional), `limit` (default 50) |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:496`.

Returns recent entries from the **[audit trail](../glossary.md#audit-trail)**.
With no filter, the handler returns the most recent `limit` rows across
all sessions. With `tool=...`, it returns rows for that tool name. With
`session_id=...`, it returns rows for that session. Requires an attached
`AuditTrail`; otherwise `404`.

```json
{
  "entries": [
    {
      "timestamp": "2026-04-10T12:30:00Z",
      "session_id": "ws:default",
      "tool_name": "read_file",
      "input_summary": "path=/etc/hosts",
      "outcome": "Success"
    }
  ]
}
```

### GET /api/audit/stats

| Field | Value |
|---|---|
| Role | Viewer |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:519`.

Returns audit aggregates: `total_entries`, `tool_breakdown` (array of
`{tool, count}`), `heartbeat_sessions`, and `viking_entries` (the size of
the root Viking directory when the Viking client is attached). This is
the rollup the Web UI shows on its audit overview tile.

```json
{
  "total_entries": 18432,
  "tool_breakdown": [
    { "tool": "read_file", "count": 4120 },
    { "tool": "bash", "count": 2981 },
    { "tool": "viking_search", "count": 1122 }
  ],
  "heartbeat_sessions": 96,
  "viking_entries": 312
}
```

## Viking memory

### GET /api/viking/list

| Field | Value |
|---|---|
| Role | Viewer |
| Query | `path` (default `viking://`) |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:591`.

Lists entries at a **[Viking](../glossary.md#viking)** directory. Requires
an attached `VikingClient`. The response body is the raw directory entry
array from the client; on error, the handler returns `200 OK` with
`{ "error": "..." }` so the UI can fall back gracefully. Each entry in
the array carries a `path`, an optional `summary` (the L0 text), and
boolean flags indicating whether it is a leaf or a subdirectory.

```bash
curl -H "Authorization: Bearer rk_view" \
  "http://127.0.0.1:18789/api/viking/list?path=viking://user/"
```

```json
[
  { "path": "viking://user/profile/", "summary": "User profile entries", "is_dir": true },
  { "path": "viking://user/preferences", "summary": "Preference notes", "is_dir": false }
]
```

### GET /api/viking/read

| Field | Value |
|---|---|
| Role | Viewer |
| Query | `path` (required), `level` (`L0`, `L1`, `L2`; default `L1`) |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:607`.

Reads a Viking memory entry at the requested detail level. `L0` is the
summary, `L1` is details, `L2` is the full content. Unknown `level`
values fall back to `L1`.

### GET /api/viking/search

| Field | Value |
|---|---|
| Role | Viewer |
| Query | `query` (required), `directory` (optional), `limit` (default 10) |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:628`.

Full-text search over Viking entries, optionally restricted to a
subdirectory. Returns the raw ranked result list from the client.

## Config editor

### GET /api/config

| Field | Value |
|---|---|
| Role | Admin |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:649`.

Returns the current contents of `ryvos.toml` as a string. Requires the
config path to have been attached to the gateway through
`GatewayServer::set_config_path`; otherwise `404`.

```json
{
  "path": "/home/user/.ryvos/ryvos.toml",
  "content": "[model]\nprovider = \"anthropic\"\n..."
}
```

### PUT /api/config

| Field | Value |
|---|---|
| Role | Admin |
| Query | — |
| Body | `{ "content": "<toml>" }` |

Source: `crates/ryvos-gateway/src/routes.rs:667`.

Writes the submitted TOML to `ryvos.toml` after running it through the
`AppConfig` parser. Malformed TOML returns `200 OK` with
`{ "error": "Invalid TOML config" }`; a successful write returns
`{ "ok": true }`. Changes do not hot-reload — the operator must restart
the daemon.

## Channels

### GET /api/channels

| Field | Value |
|---|---|
| Role | Viewer |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:692`.

Returns the configuration status of every known **[channel adapter](../glossary.md#channel-adapter)**
(Telegram, Discord, Slack, WhatsApp) plus the built-in Web UI and Gateway.
Status values are `active`, `configured`, or `not_configured`. The status
is inferred from the session manager's current session keys, so a channel
reports `active` only after it has received at least one message since
startup.

```json
{
  "channels": [
    { "name": "Telegram", "type": "telegram", "status": "active" },
    { "name": "Discord", "type": "discord", "status": "not_configured" },
    { "name": "Slack", "type": "slack", "status": "not_configured" },
    { "name": "WhatsApp", "type": "whatsapp", "status": "configured" },
    { "name": "Web UI", "type": "webui", "status": "active" },
    { "name": "Gateway", "type": "gateway", "status": "active" }
  ]
}
```

## Approvals

### GET /api/approvals

| Field | Value |
|---|---|
| Role | Viewer |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:723`.

Returns the pending requests held by the
**[approval broker](../glossary.md#approval-broker)**. Each entry carries
the request `id`, `tool_name`, `tier`, `input_summary`, and `session_id`.
The list is the same state the broker publishes on the EventBus via
`ApprovalRequested`; a REST client polling this endpoint and a
WebSocket client subscribed to events see identical data.

```json
{
  "approvals": [
    {
      "id": "apr_01HW3X4ABC",
      "tool_name": "bash",
      "tier": "T3",
      "input_summary": "rm -rf /tmp/old-cache",
      "session_id": "ws:default"
    }
  ]
}
```

### POST /api/approvals/{id}/approve

| Field | Value |
|---|---|
| Role | Operator |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:735`.

Releases the pending approval identified by `{id}` with an
`ApprovalDecision::Approved`. Returns `{ "approved": true }` when the
broker found a matching request, `{ "approved": false }` otherwise.

### POST /api/approvals/{id}/deny

| Field | Value |
|---|---|
| Role | Operator |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:751`.

Releases the pending approval with an `ApprovalDecision::Denied { reason:
"Denied via Web UI" }`. The reason string is fixed for the REST path; use
the WebSocket `approval.respond` method to supply a custom reason.

## Cron management

### GET /api/cron

| Field | Value |
|---|---|
| Role | Viewer |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:774`.

Parses `ryvos.toml` and returns the `[[cron.jobs]]` array. Requires
`set_config_path`; otherwise `404`.

### POST /api/cron

| Field | Value |
|---|---|
| Role | Operator |
| Query | — |
| Body | `{ "name", "schedule", "prompt", "channel"?, "goal"? }` |

Source: `crates/ryvos-gateway/src/routes.rs:806`.

Appends a new `[[cron.jobs]]` entry to `ryvos.toml`. The handler
validates the resulting TOML before writing and returns
`{ "ok": true, "note": "Restart required for changes to take effect" }`.
When validation fails, the response is `200 OK` with
`{ "error": "Invalid config after adding job" }` and the file is
left untouched.

```bash
curl -X POST -H "Authorization: Bearer rk_ops" \
  -H "Content-Type: application/json" \
  -d '{"name":"daily-report","schedule":"0 8 * * *","prompt":"summarize yesterday"}' \
  http://127.0.0.1:18789/api/cron
```

### DELETE /api/cron/{name}

| Field | Value |
|---|---|
| Role | Operator |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:847`.

Removes the matching job from `ryvos.toml` and rewrites the file. A
daemon restart is required for the change to take effect. Deleting a
job that does not exist is not an error — the handler walks the
config's `cron.jobs` array and filters by name, so a missing name is
a no-op that still returns `{ "ok": true }`.

## Budget

### GET /api/budget

| Field | Value |
|---|---|
| Role | Viewer |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:885`.

Returns `monthly_budget_cents` and `warn_pct` from the attached budget
config. When no `[budget]` section is configured, the response is
`{ "configured": false, "note": "..." }`.

### PUT /api/budget

| Field | Value |
|---|---|
| Role | Operator |
| Query | — |
| Body | `{ "monthly_budget_cents"?: number, "warn_pct"?: number }` |

Source: `crates/ryvos-gateway/src/routes.rs:911`.

Updates the `[budget]` section of `ryvos.toml` in place. A restart is
required.

## Model

### GET /api/model

| Field | Value |
|---|---|
| Role | Viewer |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:959`.

Returns the current `[model]` section from `ryvos.toml` with `api_key`
and `token` redacted so a Viewer cannot exfiltrate credentials through
the live config editor.

### PUT /api/model

| Field | Value |
|---|---|
| Role | Operator |
| Query | — |
| Body | `{ "model_id"?, "temperature"?, "max_tokens"?, "thinking"? }` |

Source: `crates/ryvos-gateway/src/routes.rs:1068`.

Updates fields under `[model]` in `ryvos.toml`. A restart is required.

### GET /api/models/available

| Field | Value |
|---|---|
| Role | Viewer |
| Query | `provider` (optional; defaults to the provider in `ryvos.toml`) |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:994`.

Returns a hardcoded catalog of models for the given provider. Supported
providers are `anthropic`, `openai`, `gemini`, and `groq`; unknown
providers return a single placeholder entry. This endpoint exists to
populate the Web UI model picker and is intentionally static — it is
cheaper to ship a known catalog than to hit each provider's list-models
endpoint at UI load time, and the catalog only needs to update when a
new model is added, which is a release-gated event.

```json
{
  "provider": "anthropic",
  "models": [
    { "id": "claude-opus-4-20250514", "name": "Claude Opus 4" },
    { "id": "claude-sonnet-4-20250514", "name": "Claude Sonnet 4" },
    { "id": "claude-haiku-4-20250506", "name": "Claude Haiku 4" },
    { "id": "claude-3-5-sonnet-20241022", "name": "Claude 3.5 Sonnet" }
  ]
}
```

## Integrations

### GET /api/integrations

| Field | Value |
|---|---|
| Role | Viewer |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:1125`.

Returns the list of the eight supported integration apps (`gmail`,
`calendar`, `drive`, `slack`, `notion`, `github`, `jira`, `linear`) with
their `id`, `name`, `provider`, `actions` (the tool count the integration
exposes), `icon`, plus two booleans: `configured` (credentials present in
`ryvos.toml`) and `connected` (an access token is present in
`integrations.db`). The three Google apps (`gmail`, `calendar`, `drive`)
share the same set of OAuth credentials because all three ride the
`gmail_provider` constructor; connecting any one of them also satisfies
the others.

```json
{
  "apps": [
    { "id": "gmail", "name": "Gmail", "provider": "google", "actions": 23, "icon": "mail", "configured": true, "connected": true },
    { "id": "calendar", "name": "Google Calendar", "provider": "google", "actions": 46, "icon": "calendar", "configured": true, "connected": true },
    { "id": "slack", "name": "Slack", "provider": "slack", "actions": 74, "icon": "message-square", "configured": false, "connected": false }
  ]
}
```

### POST /api/integrations/{app}/connect

| Field | Value |
|---|---|
| Role | Operator |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:1165`.

Starts the OAuth flow for `{app}`. For OAuth providers
(`gmail`/`calendar`/`drive`, `slack`, `github`, `jira`, `linear`) the
handler generates an authorization URL through
`oauth::generate_auth_url`, which appends `access_type=offline` and
`prompt=consent` so that every provider returns a refresh token on
first consent. The response carries the URL the caller should
redirect the user to:

```json
{
  "redirect_url": "https://accounts.google.com/o/oauth2/v2/auth?client_id=...&...",
  "app": "gmail"
}
```

For `notion` (which uses an API key instead of OAuth) the handler
writes the configured API key directly into `integrations.db` and
returns `{ "connected": true, "app": "notion" }`. For any other
`{app}` or for an OAuth provider whose credentials are not in
`ryvos.toml`, the handler returns `200 OK` with
`{ "error": "<app> not configured" }`.

### DELETE /api/integrations/{app}

| Field | Value |
|---|---|
| Role | Operator |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:1257`.

Removes the stored token for `{app}` from `integrations.db`. Returns
`{ "disconnected": true, "app": "<id>" }`.

### GET /api/integrations/callback

| Field | Value |
|---|---|
| Role | none |
| Query | `code`, `state` |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:1213`.

The OAuth redirect URI. Exchanges the `code` for a token pair through
`oauth::exchange_code`, stores the result in `integrations.db`, and
redirects the browser back to `/#/integrations?connected=<app>` on
success or `/#/integrations?error=<kind>&app=<app>` on failure. This
route is not protected by the role extractor because the upstream OAuth
provider calls it directly, and OAuth providers do not carry a Bearer
header on the redirect-back request. The `state` query parameter is
the `app_id` the flow was started for, so the handler can look up the
right provider config on return; Ryvos does not use `state` for CSRF
defense at this layer, which is a known simplification justified by
the single-tenant self-hosted deployment model.

## Goals and Director

### POST /api/goals/run

| Field | Value |
|---|---|
| Role | Operator |
| Query | — |
| Body | `{ "description": "...", "prompt"?: "...", "channel"?: "..." }` |

Source: `crates/ryvos-gateway/src/routes.rs:1290`.

Spawns a **[Director](../glossary.md#director)** run for the given
description. The handler constructs a `Goal` with a single `LlmJudge`
criterion, a weight of 1.0, and a success threshold of 0.7, then
spawns the runtime in a background `tokio::spawn` and returns
immediately with the synthetic session ID `goal:<timestamp>`. Events
stream over the WebSocket; this endpoint is fire-and-forget from
the REST client's perspective, so the REST response only confirms
that the goal is running. The terminal state — success, failure, or
escalation — is observable through the WebSocket event stream or by
polling `GET /api/goals/history`.

If a `channel` is specified, the handler publishes a
`CronJobComplete` event when the goal finishes so that the same
response routing used by the cron scheduler applies to goal runs.

```json
{ "session_id": "goal:20260410-123456", "status": "running", "goal": "..." }
```

### GET /api/goals/history

| Field | Value |
|---|---|
| Role | Viewer |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:1364`.

Returns the subset of `/api/runs` entries whose `session_id` starts with
`goal:` or `cron:`. When no cost store is attached the response is
`{ "runs": [] }`.

## Skills

### GET /api/skills

| Field | Value |
|---|---|
| Role | Viewer |
| Query | — |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:1397`.

Scans `$HOME/.ryvos/skills/` for `skill.toml` manifests and returns the
installed **[skills](../glossary.md#skill)**. Each entry carries `name`,
`description`, `command`, `timeout_secs`, `tier`, and `enabled`. The
handler does not load the skills — it only reads the manifests.

```json
{
  "skills": [
    {
      "name": "weather",
      "description": "Fetch current conditions",
      "command": "skills/weather.lua",
      "timeout_secs": 30,
      "tier": "t2",
      "enabled": true
    }
  ]
}
```

## Heartbeat history

### GET /api/heartbeat/history

| Field | Value |
|---|---|
| Role | Viewer |
| Query | `limit` (default 50, capped at 100) |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:1441`.

Filters recent audit entries to only those whose `session_id` starts with
`heartbeat:`, so a UI can render recent **[Heartbeat](../glossary.md#heartbeat)**
runs without walking the whole audit trail. Requires an attached audit
trail; otherwise `404`.

## Webhooks

### POST /api/hooks/wake

| Field | Value |
|---|---|
| Role | Bearer token in `Authorization` (config.webhooks.token) |
| Query | — |
| Body | `{ "prompt", "session_id"?, "channel"?, "metadata"?, "callback_url"? }` |

Source: `crates/ryvos-gateway/src/routes.rs:329`.

The generic inbound webhook. Runs the agent with `prompt` for the given
session (a new random session is created if omitted) and returns the
final response. Authentication for this endpoint uses a separate token
in `gateway.webhooks.token` rather than the standard API key chain; this
token is compared to a Bearer header. When `gateway.webhooks.enabled` is
false or unset, the endpoint returns `404`. When `gateway.webhooks.token`
is unset but `enabled` is true, the endpoint accepts requests with no
Bearer header — an intentional loophole for internal deployments where
the caller's IP is already trusted.

If `callback_url` is supplied, the handler fires an outbound POST with
`{ session_id, response, metadata }` after the run completes. The
outbound payload schema is documented in [webhook-format.md](webhook-format.md).
Callback delivery is fire-and-forget: the handler spawns a Tokio task,
does not retry on failure, and logs a warning on error.

```bash
curl -X POST -H "Authorization: Bearer my_webhook_token" \
  -H "Content-Type: application/json" \
  -d '{"prompt":"run build","callback_url":"https://ci.example.com/ryvos-done"}' \
  http://127.0.0.1:18789/api/hooks/wake
```

### GET /api/whatsapp/webhook

| Field | Value |
|---|---|
| Role | none |
| Query | `hub.mode`, `hub.verify_token`, `hub.challenge` |
| Body | — |

Source: `crates/ryvos-gateway/src/routes.rs:431`.

The Meta Cloud API verification handshake. Returns the `hub.challenge`
value as a plain-text body when the verify token matches the one
configured for the WhatsApp adapter. Returns `403 Forbidden` on token
mismatch and `404 Not Found` when the WhatsApp handle is not attached.

### POST /api/whatsapp/webhook

| Field | Value |
|---|---|
| Role | none |
| Query | — |
| Body | Meta Cloud API event payload (passed through) |

Source: `crates/ryvos-gateway/src/routes.rs:446`.

Forwards inbound WhatsApp events to the adapter's `process_webhook`
method. The response is always `200 OK` with no body, because Meta's
retry policy treats any non-2xx status as a delivery failure and
backs off aggressively. Authenticity is verified inside the adapter
through the webhook signature header.

## WebSocket

### GET /ws

| Field | Value |
|---|---|
| Role | Viewer (or anonymous-admin in single-user mode) |
| Query | `token`, `password` (same as REST) |
| Body | WebSocket upgrade |

Source: `crates/ryvos-gateway/src/routes.rs:457`.

Upgrades the connection to a WebSocket and hands it to
`connection::handle_connection`. The frame protocol, the RPC method
list, the LaneQueue semantics, and the full 23-event translation table
are documented in [gateway-websocket.md](gateway-websocket.md).

## Static UI

### GET /

Source: `crates/ryvos-gateway/src/static_files.rs`.

Returns the embedded `index.html` from the compiled Svelte 5 bundle.
No authentication — a browser must always be able to load the UI in
order to log in. See
[../adr/007-embedded-svelte-web-ui.md](../adr/007-embedded-svelte-web-ui.md)
for the decision to embed the UI in the binary.

### GET /assets/{*path}

Source: `crates/ryvos-gateway/src/static_files.rs`.

Serves static assets from the embedded bundle (JS, CSS, fonts, SVGs).
The MIME type is inferred from the extension through `mime_guess`. Any
path that does not resolve to an embedded file returns `404`.

## Cross-links

- [../crates/ryvos-gateway.md](../crates/ryvos-gateway.md) — the crate
  reference, including the `GatewayServer` builder and the `AppState`
  shape.
- [auth-and-rbac.md](auth-and-rbac.md) — the authoritative permission
  matrix and API-key configuration guide.
- [gateway-websocket.md](gateway-websocket.md) — the WebSocket frame
  schema and event translation table.
- [webhook-format.md](webhook-format.md) — the outbound payload schema
  used by `POST /api/hooks/wake`.
- [../operations/configuration.md](../operations/configuration.md) —
  every `ryvos.toml` key, including `[gateway]`, `[gateway.webhooks]`,
  and `[[gateway.api_keys]]`.
