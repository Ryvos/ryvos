# ryvos-gateway

`ryvos-gateway` is the HTTP and WebSocket surface of the Ryvos daemon. It wraps
the agent runtime, the session store, the **[EventBus](../glossary.md#eventbus)**,
and the **[approval broker](../glossary.md#approval-broker)** in an Axum server
that exposes more than forty REST endpoints, a full-duplex WebSocket protocol,
a five-provider OAuth flow, and an embedded Svelte 5 single-page application.
The crate is the only way a browser, a cloud dashboard, or a scripted HTTP
client talks to Ryvos.

The design goal is "one binary, one port". When the daemon boots, the Svelte
UI — built at compile time and baked into the binary via `rust_embed` — is
served from the same listener as the REST API, the WebSocket upgrade handler,
and the OAuth callback endpoint. There is no separate static file server, no
reverse proxy, and no second process. See ADR-007 for the rationale behind
embedding the UI instead of shipping it as a sibling asset.

This document focuses on what lives in the crate: the `GatewayServer` type, the
shared `AppState`, the authentication model, the WebSocket protocol, the OAuth
bridge, and the router topology. The full list of REST endpoints and their
request/response shapes is the subject of [../api/gateway-rest.md](../api/gateway-rest.md);
the WebSocket frame format is the subject of
[../api/gateway-websocket.md](../api/gateway-websocket.md). The role-based
access-control contract is documented in
[../api/auth-and-rbac.md](../api/auth-and-rbac.md).

## Position in the stack

`ryvos-gateway` is an integration-layer crate. Its direct workspace
dependencies are `ryvos-core` (config, traits, event bus), `ryvos-agent`
(`AgentRuntime`, `ApprovalBroker`, `AuditTrail`, `SessionManager`),
`ryvos-channels` (for the `WhatsAppWebhookHandle` that routes inbound Meta
Cloud API calls through the gateway), `ryvos-tools`, and `ryvos-memory`
(`CostStore`, `IntegrationStore`, `SessionMetaStore`, `VikingClient`). The
binary at `crates/ryvos/src/main.rs` constructs every one of those collaborators
and hands them to the gateway through the builder methods on
`GatewayServer`.

External dependencies are kept to the minimum needed for a production HTTP
stack: `axum` for routing and extractors, `tower` plus `tower-http` for the
permissive CORS layer, `rust-embed` and `mime_guess` for the embedded UI,
`reqwest` for the outbound OAuth token exchanges, `tokio-util` for the
cancellation token that drives graceful shutdown, and `toml` for the live
config editor endpoint. See `crates/ryvos-gateway/Cargo.toml` for the exact
set.

## GatewayServer

The public entry point is `GatewayServer` in
`crates/ryvos-gateway/src/server.rs:23`. It holds the full set of collaborators
that the Axum handlers need at request time, plus a `start_time: Instant`
captured when the server is constructed so that the `/api/health` endpoint can
report uptime.

`GatewayServer::new` takes the six mandatory collaborators — the
`GatewayConfig`, the `Arc<AgentRuntime>`, the `Arc<EventBus>`, the
`Arc<dyn SessionStore>`, the `Arc<SessionManager>`, and the
`Arc<ApprovalBroker>` — and initializes every optional collaborator to
`None`. The rest of the collaborators are attached through builder-style
setters so that the caller can opt out of subsystems the current deployment
does not use:

- `set_whatsapp_handle(WhatsAppWebhookHandle)` — registers the WhatsApp
  adapter's webhook sink so that `/api/whatsapp/webhook` can forward inbound
  messages into the dispatcher.
- `set_cost_store(Arc<CostStore>, Option<BudgetConfig>)` — enables the
  `/api/costs`, `/api/metrics`, and `/api/budget` routes for the monitoring
  dashboard.
- `set_audit_trail(Arc<AuditTrail>)` — enables the `/api/audit` and
  `/api/audit/stats` routes.
- `set_viking_client(Arc<VikingClient>)` — enables the `/api/viking/*` proxy
  to the local or remote **[Viking](../glossary.md#viking)** store.
- `set_config_path(PathBuf)` — enables the live config editor at
  `/api/config`.
- `set_session_meta(Arc<SessionMetaStore>)` — attaches the per-session
  metadata store used by the channel dispatcher for resuming CLI provider
  sessions.
- `set_integration_store(Arc<IntegrationStore>)` — persistence layer for OAuth
  tokens obtained through the integration callback.
- `set_integrations_config(IntegrationsConfig)` — client IDs and client
  secrets for the five one-click OAuth providers.

`GatewayServer::run(shutdown: CancellationToken)` builds the Axum router,
binds a `TcpListener` to `config.bind`, and calls `axum::serve` with
`with_graceful_shutdown`. The future completes when the cancellation token
fires, at which point the daemon's main shutdown sequence proceeds to drain
the other subsystems.

## AppState

Every Axum handler in the crate takes `State<Arc<AppState>>` as an extractor.
`AppState` in `crates/ryvos-gateway/src/state.rs:14` is the concrete shared
state built once inside `GatewayServer::run` and handed to the router via
`with_state`. It is structurally identical to `GatewayServer`'s field set
minus the builder methods: the mandatory collaborators are plain `Arc`s, the
optional collaborators are `Option<Arc<_>>`, and the fields the router does
not touch (for example, nothing) are omitted.

Wrapping the whole state in a single `Arc<AppState>` keeps the router
ergonomic — handlers never need multiple extractors for different
collaborators — and cheap, because Axum clones the outer `Arc` on every
request rather than cloning each field individually. The optional fields are
checked at handler entry; routes whose collaborators are not attached return
`503 Service Unavailable` rather than panicking.

## Authentication

Authentication lives in `crates/ryvos-gateway/src/auth.rs`. Every protected
route extracts an `Authenticated` from the request via the extractor defined
in `crates/ryvos-gateway/src/middleware.rs`, which in turn calls
`validate_auth`. There are three roles:

- `Viewer` — read-only access to sessions, metrics, audit, costs, Viking, and
  heartbeat history.
- `Operator` — everything a Viewer can do, plus sending messages, running
  goals, responding to approvals, and triggering webhooks.
- `Admin` — everything an Operator can do, plus editing the config file,
  changing the model, changing the budget, managing cron jobs, and
  connecting or disconnecting integrations.

`validate_auth` walks a fixed precedence chain:

1. **Bearer header.** If the request carries an `Authorization: Bearer <key>`
   header, the value is compared against every `ApiKeyConfig` in
   `config.api_keys`. A match returns `AuthResult { name, role }` with the
   configured role. If the header does not match any API key, it is then
   compared against the deprecated `config.token` field, which always grants
   `Admin` under the name `legacy-token`. An unmatched bearer value is a hard
   denial — no fall-through to query parameters.
2. **Query `?token=...`.** If `config.token` is set and the Bearer header is
   absent, the `token` query parameter is checked; a match grants `Admin`.
3. **Query `?password=...`.** If `config.token` is not set but
   `config.password` is, the `password` query parameter is checked; a match
   grants `Admin`.
4. **Anonymous Admin.** If none of `config.api_keys`, `config.token`, or
   `config.password` is configured, `validate_auth` returns
   `AuthResult { name: "anonymous-admin", role: Admin }`. This is the
   default experience for self-hosted single-user deployments: a fresh
   `ryvos serve` with no auth configured grants the owner full access to the
   UI out of the box. To lock it down, the operator adds one or more
   `[[gateway.api_keys]]` entries to `ryvos.toml`.

Two helpers in the same file — `has_viewer_access` and `has_operator_access` —
compile the role hierarchy: Admin ≥ Operator ≥ Viewer. Route handlers call
these directly rather than reasoning about individual variants.

The `/api/health` endpoint and the embedded UI at `/` and `/assets/*` are
deliberately not protected. A fully anonymous client can always confirm the
daemon is up and load the login-capable UI; every other endpoint enforces at
least `Viewer`. See [../api/auth-and-rbac.md](../api/auth-and-rbac.md) for
the authoritative permission matrix.

Requests that the extractor rejects return `401 Unauthorized` with no
body; requests that the extractor accepts but whose role is insufficient
for the specific handler return `403 Forbidden`. Handlers never panic on
an auth decision, and the extractor's work is O(1) in the number of
configured API keys because the list is walked exactly once per request.
The legacy `token` and `password` fields predate the `api_keys` list and
are retained so that existing single-operator deployments keep working
across upgrades; both paths grant `Admin` because the older config
format did not have a role concept.

## REST router

The router is constructed in a single builder chain inside
`GatewayServer::run` and then handed to `axum::serve` along with the
shared state. Routes are grouped by subsystem:

- **Health** — `GET /api/health`. The only unauthenticated API route.
- **Sessions** — `GET /api/sessions`, `GET /api/sessions/{id}/history`,
  `POST /api/sessions/{id}/messages`.
- **Monitoring dashboard** — `GET /api/metrics`, `GET /api/runs`,
  `GET /api/costs`.
- **Audit** — `GET /api/audit`, `GET /api/audit/stats`.
- **Viking browser** — `GET /api/viking/list`, `GET /api/viking/read`,
  `GET /api/viking/search`. The handlers proxy to the attached `VikingClient`,
  which can be backed by `viking.db` or by a remote
  `ryvos viking-server` process.
- **Config editor** — `GET /api/config` and `PUT /api/config`. The PUT
  handler validates incoming TOML by running it through `AppConfig`'s parser
  before writing the file.
- **Channel status** — `GET /api/channels`.
- **Approvals** — `GET /api/approvals`, `POST /api/approvals/{id}/approve`,
  `POST /api/approvals/{id}/deny`. These are the REST equivalent of the
  `approval.respond` WebSocket method; both routes ultimately call
  `ApprovalBroker::respond`.
- **Cron** — `GET /api/cron`, `POST /api/cron`,
  `DELETE /api/cron/{name}`.
- **Budget** — `GET /api/budget`, `PUT /api/budget`.
- **Model** — `GET /api/model`, `PUT /api/model`,
  `GET /api/models/available`.
- **Integrations** — `GET /api/integrations`,
  `POST /api/integrations/{app}/connect`,
  `DELETE /api/integrations/{app}`,
  `GET /api/integrations/callback`.
- **Skills** — `GET /api/skills`.
- **Heartbeat history** — `GET /api/heartbeat/history`.
- **Goals** (Director) — `POST /api/goals/run`, `GET /api/goals/history`.
- **Webhooks** — `POST /api/hooks/wake` for the generic inbound webhook that
  wakes a session, `GET|POST /api/whatsapp/webhook` for Meta Cloud API
  verification and incoming messages.
- **WebSocket** — `GET /ws`.
- **Static UI** — `GET /` serves the embedded `index.html`, `GET /assets/*`
  serves the Svelte bundle.

The top of the router chain attaches a permissive `CorsLayer` so that a
browser served from a different origin (the Ryvos Cloud dashboard, for
example) can call the API directly. The same `CorsLayer` applies to the
WebSocket upgrade response.

All thirty-eight handler functions live in `crates/ryvos-gateway/src/routes.rs`
and share the same pattern: extract `State<Arc<AppState>>` and
`Authenticated`, check the role with `has_viewer_access` or
`has_operator_access`, dispatch to the relevant collaborator through
`Arc`-wrapped fields, and return `Json<serde_json::Value>` or a typed struct.
Handler bodies never hold locks across `.await` points and never touch any
subsystem that is not carried on `AppState`. Handlers that depend on an
optional collaborator (for example, `/api/viking/list` requires the
Viking client) check for the `None` case explicitly and return
`503 Service Unavailable` with a short JSON body so the Web UI can gray
out the corresponding page.

The approvals routes are worth calling out because they exist in both
REST and WebSocket form. `GET /api/approvals` lists the pending requests
held by the `ApprovalBroker`; `POST /api/approvals/{id}/approve` and
`POST /api/approvals/{id}/deny` dispatch an `ApprovalDecision`. The
three routes are the canonical way a dashboard, a scripted client, or a
mobile app responds to a **[soft checkpoint](../glossary.md#soft-checkpoint)**
without opening a WebSocket. The WebSocket `approval.respond` method is
functionally identical and exists so that a connected browser does not
need to make a separate HTTP round trip.

The config editor and the live model picker both delegate to the
in-process collaborators that own the canonical state — `AppConfig` for
the editor, the `AgentRuntime` for the model — so that a write through
the API is indistinguishable from a write made by editing the TOML file
directly. The editor's `PUT` handler runs the submitted text through the
`AppConfig` parser before touching the file so that a malformed config
cannot corrupt the on-disk copy.

See [../api/gateway-rest.md](../api/gateway-rest.md) for the
endpoint-by-endpoint contract, the full request and response shapes, and
the exact error codes each route can return.

## WebSocket protocol

The WebSocket surface is intentionally thin. `handle_connection` in
`crates/ryvos-gateway/src/connection.rs:38` is the handler attached to
`GET /ws`. Each connection accepts JSON frames, validates them against the
`ClientFrame` shape defined in `crates/ryvos-gateway/src/protocol.rs:4`, and
processes them through a per-connection **[lane](../glossary.md#lane)** queue.

The frame vocabulary has three kinds of messages, tagged by a `type` field:

- `request` — a client RPC call. Carries an `id`, a `method`, and arbitrary
  `params`. The server replies with a `response` frame carrying the same
  `id`.
- `response` — the reply to a specific `request`. Carries either a `result`
  or an `error` with a numeric code and a message, modeled after JSON-RPC.
- `event` — a push frame from the server, generated from an `AgentEvent` on
  the EventBus. Carries a `session_id` and an `EventPayload` with a `kind`
  string and optional `text`, `tool`, and `data` fields.

Five RPC methods are handled in `process_request`:

- `agent.send` — runs the agent for a session. If `session_id` is empty, the
  session manager creates or fetches one keyed `ws:default`. The method
  auto-subscribes the connection to the session so that subsequent
  `TextDelta` events flow through. The reply carries the final text and the
  resolved session ID.
- `agent.cancel` — triggers the runtime's cancellation token.
- `session.list` — returns the keys tracked by `SessionManager`.
- `session.history` — loads the last `limit` (default 50) messages for a
  session from the session store.
- `approval.respond` — calls `ApprovalBroker::respond` with an
  `ApprovalDecision::Approved` or `ApprovalDecision::Denied { reason }`
  based on the `approved` boolean. The broker matches on the exact request
  ID, so the caller must already know the full ID.

All five run through a `LaneQueue` (`crates/ryvos-gateway/src/lane.rs`), a
bounded mpsc channel with a buffer of 32. A single background task per
connection drains the queue and dispatches one request at a time. The queue
is what the glossary calls a "lane": requests from the same client are
serialized so that a fast `agent.send` cannot race with a slow
`session.history` from the same browser tab, while requests from different
connections are free to run in parallel. If a client floods the queue the
backpressure surfaces as a blocked `send` on the client side, not as a
starved server. The lane is also what makes cancellation safe: a pending
`agent.cancel` waits behind whatever request is currently in flight and
then runs, which means the runtime's cancellation token never races with
a new `agent.send` from the same tab.

Each `LaneItem` carries the method, the params, and a oneshot sender for
the result. When the processing task finishes handling the item, it
writes the `serde_json::Value` result into the oneshot; the main
WebSocket read loop `await`s that oneshot and writes a `ServerResponse`
frame back to the client. A client that disconnects while a request is
in flight drops its half of the oneshot and the processing task silently
discards the result — there is no orphaned work left running against a
dead connection.

In parallel with the lane, a second background task subscribes to the
EventBus and translates around twenty-three `AgentEvent` variants into
outbound `ServerEvent` frames. The interesting translations are:

- `TextDelta`, `ToolStart`, and `ToolEnd` become `text_delta`, `tool_start`,
  and `tool_end` events tagged with the last session the connection touched.
  `ToolStart` carries the raw input JSON in `data`; `ToolEnd` carries the
  tool's `content` and `is_error` flag.
- `RunStarted`, `RunComplete`, and `RunError` map to `run_started`,
  `run_complete` (with `total_turns`, `input_tokens`, `output_tokens`), and
  `run_error`.
- `ApprovalRequested` maps to `approval_requested` with the pending
  request's `id`, `tool_name`, `tier`, `input_summary`, and `session_id`
  embedded in `data`.
- `BudgetWarning` and `BudgetExceeded` map to `budget_warning` and
  `budget_exceeded`, each carrying `spent_cents` and `budget_cents`.
- `HeartbeatFired`, `HeartbeatOk`, and `HeartbeatAlert` map to
  `heartbeat_fired`, `heartbeat_ok`, and `heartbeat_alert`. The first is
  tagged with the synthetic session ID `system` so that UI subscribers
  always receive it.
- `CronFired` and `CronJobComplete` map to `cron_fired` and `cron_complete`.
- `GuardianStall`, `GuardianDoomLoop`, and `GuardianBudgetAlert` surface as
  the corresponding `guardian_*` events.
- `GraphGenerated`, `NodeComplete`, `EvolutionTriggered`, and
  `SemanticFailureCaptured` expose the **[Director](../glossary.md#director)**'s
  graph state, per-node progress, plan evolution, and semantic failure
  diagnostics for the Goals UI.

Several `AgentEvent` variants — `TurnComplete`, `ApprovalResolved`,
`GuardianHint`, `GoalEvaluated`, `DecisionMade`, `JudgeVerdict` — are
intentionally dropped because the browser does not need them. The full
translation table is documented in
[../api/gateway-websocket.md](../api/gateway-websocket.md).

Every WebSocket connection auto-subscribes to the synthetic session ID
`*` at connect time. System-wide events that do not belong to any one
session — `HeartbeatFired`, `CronFired`, `CronJobComplete` — are tagged
with `system` as their session field and still forwarded, so the UI
shows heartbeat and cron activity even before the user has sent a
message. When a user does send a message through `agent.send`, the
resolved session ID is appended to the connection's subscribed list,
and subsequent `TextDelta`, `ToolStart`, and `ToolEnd` events addressed
to that session are forwarded with the correct tag.

## OAuth bridge

`crates/ryvos-gateway/src/oauth.rs` implements a generic OAuth 2.0
authorization-code flow with `generate_auth_url`, `exchange_code`, and
`refresh_token` helpers, plus five pre-configured provider constructors:
`gmail_provider`, `slack_provider`, `github_provider`, `jira_provider`, and
`linear_provider`. Each constructor fills in the provider's authorization
endpoint, token endpoint, and a scope list tailored to the built-in tools
that need access — for Gmail, that is Gmail read/send/modify, Google
Calendar, and Drive read-only; for Slack, it is `channels:read`,
`channels:history`, `chat:write`, and `users:read`.

The user-facing flow is driven from the Integrations page in the Web UI:

1. The browser calls `POST /api/integrations/{app}/connect`. The handler
   resolves the `OAuthProviderConfig` for `{app}` by looking up the
   corresponding `IntegrationsConfig` section on `AppState`, generates a
   random state token, and returns an authorization URL built by
   `generate_auth_url`.
2. The UI redirects the user to the provider's consent screen.
3. The provider redirects back to `GET /api/integrations/callback` with a
   `code` and the original `state`. The handler calls `exchange_code`, which
   POSTs a form-encoded token request to the provider and returns a
   `TokenResponse { access_token, refresh_token, expires_in, token_type,
   scope }`.
4. The handler writes the tokens into `IntegrationStore` (SQLite, persisted
   in `integrations.db`) keyed by the app ID.
5. Tools that need the integration read their tokens from `IntegrationStore`
   at execution time. When an access token expires, the gateway's
   `refresh_token` helper is called with the stored refresh token to obtain
   a new pair.

`get_provider(app_id, &IntegrationsConfig)` resolves an `app_id` to its
provider constructor; the Gmail constructor is reused for the aliases
`google`, `calendar`, and `drive` because all three ride the same Google
OAuth credentials. The authorization URL builder appends
`access_type=offline` and `prompt=consent` so that every provider returns
a refresh token the first time through, even when the user has already
consented to a narrower scope set in the past.

The error-handling story is deliberately plain: `exchange_code` and
`refresh_token` return `Err(String)` rather than a structured error type,
because the handler's job is to hand the string back to the Web UI so a
human can read it. Network failures, non-2xx responses, and deserialization
errors all produce a descriptive message. The tokens themselves are never
logged; only their presence is recorded via `tracing::info!`.

## Embedded Web UI

The Svelte 5 single-page application lives in a sibling `ui/` directory at
the repo root, built by `npm run build` into `ui/dist/`. At compile time, the
`#[derive(Embed)]` macro from `rust_embed` walks the output directory and
embeds every file into the binary as a `UiAssets` type
(`crates/ryvos-gateway/src/static_files.rs`). Two handlers serve the
embedded tree:

- `GET /` calls `UiAssets::get("index.html")` and returns the payload with
  `content-type: text/html; charset=utf-8`.
- `GET /assets/*path` calls `UiAssets::get("assets/{path}")` and sets the
  MIME type via `mime_guess::from_path`. Any path that does not resolve to an
  embedded file returns `404`.

Because the UI is baked in, the daemon ships as a single self-contained
binary — the same executable that runs the agent loop also serves the
dashboard. The build cost is paid at `cargo build --release` time and the
runtime memory footprint of the UI assets is roughly 376 KB. See ADR-007 for
the alternative approaches considered (external static server, CDN, dynamic
asset directory) and why embedding won.

The practical consequence is that there is no way for the UI and the API
to drift out of sync at runtime: upgrading the binary atomically upgrades
both halves, and operators never have to coordinate a browser cache
invalidation with a daemon restart. The downside — longer build times and
a bigger binary — is accepted as the cost of a single-file deployment.
For a development workflow where the Svelte side changes often, the UI
can be served from a separate `vite dev` process on a different port;
the Axum router's permissive CORS layer makes that setup transparent.

## Where to go next

For the REST endpoint contracts, read
[../api/gateway-rest.md](../api/gateway-rest.md). For the WebSocket frame
schema and the full event translation table, read
[../api/gateway-websocket.md](../api/gateway-websocket.md). The role
hierarchy and how to configure API keys are described in
[../api/auth-and-rbac.md](../api/auth-and-rbac.md). For the decision to embed
the Svelte UI rather than ship it separately, read
[ADR-007](../adr/007-embedded-svelte-web-ui.md).
