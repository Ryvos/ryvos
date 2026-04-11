# Auth and role-based access control

Every HTTP and WebSocket request served by `ryvos-gateway` passes
through a single authentication extractor. The extractor resolves the
caller to one of three roles — `Viewer`, `Operator`, or `Admin` — and
each route handler then checks whether the resolved role has enough
privilege for the specific action. This document is the authoritative
reference for the auth chain, the role hierarchy, the per-route
permission matrix, and the `ryvos.toml` configuration that ties it all
together.

The implementation lives in `crates/ryvos-gateway/src/auth.rs` and the
axum extractor that plugs it into every handler lives in
`crates/ryvos-gateway/src/middleware.rs:11`. The three endpoints that
opt out of authentication entirely — `GET /api/health`, `GET /`, and
`GET /assets/*` — are wired with plain `get(...)` handlers that do not
extract `Authenticated`.

## Auth sources

The extractor reads two places: the `Authorization` header and the URL
query string. `validate_auth`
(`crates/ryvos-gateway/src/auth.rs:14`) then walks a fixed four-step
precedence chain. The first step that produces a decision is
authoritative; later steps are never consulted. The chain is:

1. **Bearer header matched against `api_keys`.** If the request carries
   `Authorization: Bearer <key>`, the value is compared byte-for-byte
   against every `ApiKeyConfig` entry in `config.gateway.api_keys`. A
   match returns `AuthResult { name, role }` with the configured name
   and role.
2. **Bearer header matched against legacy `token`.** If no API key
   matched, the Bearer value is compared against the deprecated
   `gateway.token` field. A match grants `Admin` under the name
   `legacy-token`. An unmatched Bearer value is a hard denial; the
   extractor does not fall through to query parameters when the client
   already supplied a Bearer header.
3. **Query `?token=...`.** When the Bearer header is absent and
   `gateway.token` is set, the `token` query parameter is compared to
   `gateway.token`. A match grants `Admin` under the name
   `legacy-token`. When `gateway.token` is set, the `?password=...`
   path is never consulted.
4. **Query `?password=...`.** When the Bearer header is absent,
   `gateway.token` is unset, and `gateway.password` is set, the
   `password` query parameter is compared to `gateway.password`. A
   match grants `Admin` under the name `legacy-password`.
5. **Anonymous Admin.** If none of `api_keys`, `token`, or `password`
   is configured, `validate_auth` returns `AuthResult { name:
   "anonymous-admin", role: Admin }`. This is the default experience
   for self-hosted single-user deployments: a fresh `ryvos serve` with
   no auth configured grants the owner full access to the Web UI,
   every REST endpoint, and the WebSocket out of the box.

The chain is deliberately strict about Bearer mismatches. If a caller
supplies a Bearer header, the extractor commits to that path and does
not fall through to query auth. Otherwise, a forgotten API key would
silently succeed through the query fallback, which is worse than a
clean `401`.

The anonymous-admin fallback only triggers when *all three* auth
mechanisms are empty. The moment an operator adds one `[[gateway.api_keys]]`
entry, the fallback is gone and every protected endpoint requires a
matching Bearer header (or the legacy `token` / `password` query
parameters, if those are also configured).

### Two query helpers

`extract_token_from_query` and `extract_password_from_query` are thin
URL parsers that split the query string on `&` and return the value
after `token=` or `password=` respectively. Both are used by the
extractor when there is no Bearer header; they do not handle URL
decoding of the value because the auth chain compares the raw string
directly to the configured secret, which is assumed to be in the same
form.

## Roles

`ApiKeyRole` in `crates/ryvos-core/src/config.rs:740` is the three-variant
enum that every authenticated request carries:

- `Viewer` — read-only access to sessions, history, metrics, audit,
  **[Viking](../glossary.md#viking)** memory, heartbeat history, cron
  listings, budget, model settings, integration listings, channel
  status, goal history, and skills.
- `Operator` — everything a `Viewer` can do, plus sending messages,
  responding to approvals, running goals, mutating cron/budget/model
  settings, triggering webhooks, and connecting or disconnecting
  integrations. This is the default role (`#[default]`), so an API key
  declared without an explicit `role = "..."` field becomes an
  `Operator`.
- `Admin` — everything an `Operator` can do, plus the live config
  editor (`GET /api/config`, `PUT /api/config`). The config editor is
  the only endpoint that requires `Admin` exactly; every other
  write-path endpoint accepts `Operator` and above.

The hierarchy is a total order: `Admin > Operator > Viewer`. The
`has_viewer_access` helper in `crates/ryvos-gateway/src/auth.rs:94`
returns `true` for all three variants; `has_operator_access` returns
`true` for `Operator` and `Admin`. Route handlers call these helpers
directly rather than matching on the enum, which keeps the role check
one line long and consistent across every handler.

There is no "write-only" or "send-only" role, no per-channel
scoping, and no per-session ACL. The three-tier model is the product
of a decision that a single-binary agent runtime should have an auth
story that fits on a sticky note, not a full RBAC engine with policy
documents. Operators needing finer-grained control typically front
Ryvos with an HTTP proxy and do role-mapping at the proxy.

## Role by endpoint

The table below lists every protected endpoint with the minimum role
it requires. "None" means the endpoint is deliberately unprotected,
"Viewer" means the role check uses `has_viewer_access`, "Operator"
means `has_operator_access`, and "Admin" means the handler matches on
`ApiKeyRole::Admin` exactly.

| Endpoint | Method | Required role |
|---|---|---|
| `/api/health` | GET | None |
| `/` and `/assets/*` | GET | None |
| `/ws` | GET (upgrade) | Viewer |
| `/api/sessions` | GET | Viewer |
| `/api/sessions/{id}/history` | GET | Viewer |
| `/api/sessions/{id}/messages` | POST | Operator |
| `/api/metrics` | GET | Viewer |
| `/api/runs` | GET | Viewer |
| `/api/costs` | GET | Viewer |
| `/api/audit` | GET | Viewer |
| `/api/audit/stats` | GET | Viewer |
| `/api/viking/list` | GET | Viewer |
| `/api/viking/read` | GET | Viewer |
| `/api/viking/search` | GET | Viewer |
| `/api/config` | GET | Admin |
| `/api/config` | PUT | Admin |
| `/api/channels` | GET | Viewer |
| `/api/approvals` | GET | Viewer |
| `/api/approvals/{id}/approve` | POST | Operator |
| `/api/approvals/{id}/deny` | POST | Operator |
| `/api/cron` | GET | Viewer |
| `/api/cron` | POST | Operator |
| `/api/cron/{name}` | DELETE | Operator |
| `/api/budget` | GET | Viewer |
| `/api/budget` | PUT | Operator |
| `/api/model` | GET | Viewer |
| `/api/model` | PUT | Operator |
| `/api/models/available` | GET | Viewer |
| `/api/integrations` | GET | Viewer |
| `/api/integrations/{app}/connect` | POST | Operator |
| `/api/integrations/{app}` | DELETE | Operator |
| `/api/integrations/callback` | GET | None (called by OAuth provider) |
| `/api/goals/run` | POST | Operator |
| `/api/goals/history` | GET | Viewer |
| `/api/skills` | GET | Viewer |
| `/api/heartbeat/history` | GET | Viewer |
| `/api/hooks/wake` | POST | webhook-specific Bearer |
| `/api/whatsapp/webhook` | GET/POST | None (Meta verifies upstream) |

Two endpoints have non-standard auth. `POST /api/hooks/wake` uses the
separate token in `gateway.webhooks.token`, compared against a Bearer
header; it does not consult `api_keys` at all, and the webhook config
must be explicitly enabled with `enabled = true`. The two
`/api/whatsapp/webhook` routes are unauthenticated on the HTTP layer
because Meta's verification handshake and inbound delivery cannot
carry a Bearer header; the adapter verifies the payload signature
inside `process_webhook`. See [gateway-rest.md](gateway-rest.md) for
the full handler descriptions.

## Configuring API keys

API keys are declared in `ryvos.toml` under `[[gateway.api_keys]]`.
Each entry has three fields: `name` (a human-readable label that
shows up in logs), `key` (the Bearer token the caller must supply),
and `role` (one of `viewer`, `operator`, or `admin`; defaults to
`operator` if omitted).

```toml
[gateway]
bind = "0.0.0.0:18789"

[[gateway.api_keys]]
name = "web-ui"
key  = "rk_ui_01HW3...abc"
role = "operator"

[[gateway.api_keys]]
name = "dashboard-readonly"
key  = "rk_view_01HW4...def"
role = "viewer"

[[gateway.api_keys]]
name = "admin-cli"
key  = "rk_admin_01HW5...ghi"
role = "admin"
```

The `rk_...` prefix is a convention for Ryvos-generated keys — it
stands for "Ryvos key" — and makes it easy to spot a Ryvos credential
in environment dumps, log output, and git diffs. The prefix has no
meaning to `validate_auth`; keys without the prefix work identically.
A prefix-only scheme is not a substitute for rotation, but it does
make accidental commits of a key to a public repo easier to detect
with static analysis.

Keys are compared byte-for-byte and must be supplied exactly. There is
no hashing, no HMAC construction, and no JWT parsing. This is
deliberate: Ryvos is a single-tenant daemon and the auth model is
"shared secret over TLS", not a distributed identity system. Put a
reverse proxy in front of Ryvos and terminate TLS there; never expose
`gateway.bind` on `0.0.0.0` without TLS.

### Rotating keys

Rotation is a two-step edit of `ryvos.toml`: add a new entry with a
fresh key, wait for clients to pick up the new key, then remove the
old entry. Because the API key list is read once at startup, a
rotation also requires a daemon restart (or a config reload once the
restart-free path lands). The config editor endpoint
(`PUT /api/config`) writes the updated file, so a running Admin can
rotate through the Web UI.

The deprecated `gateway.token` and `gateway.password` fields predate
the `api_keys` list and are retained so existing single-operator
deployments keep working across upgrades. Both grant `Admin` because
the older config format did not have a role concept. New deployments
should use `api_keys` exclusively; the legacy paths are a migration
aid, not a recommended pattern.

## Security considerations

- **HTTPS is mandatory in production.** Every authentication path
  carries a shared secret — an API key, a token, or a password — in
  plain text inside the HTTP request. Without TLS, a network observer
  can replay the secret. Ryvos itself does not terminate TLS; run a
  reverse proxy (Caddy, nginx, Cloudflare, Fly.io's built-in TLS) in
  front of the gateway.
- **Do not expose `gateway.bind` on a public interface.** The default
  bind is `127.0.0.1:18789`, which is only reachable from the same
  host. Change this only when you have a reverse proxy terminating
  TLS and forwarding to the loopback address.
- **Rotate on compromise.** If an API key leaks, remove the
  corresponding `[[gateway.api_keys]]` entry from `ryvos.toml` and
  restart the daemon. There is no in-memory revocation list.
- **Segregate roles.** A dashboard that only reads metrics should use
  a `Viewer` key, not an `Operator` or `Admin` key. The principle of
  least privilege applies here even though the three roles are coarse.
- **Keep the webhook token separate from the API key list.** The
  `gateway.webhooks.token` is a distinct credential; a compromised
  webhook token can only trigger `POST /api/hooks/wake`, not the full
  API surface. Treat it as a separate secret with its own rotation
  schedule.
- **The Web UI is served anonymously.** A browser must always be able
  to load `/` and `/assets/*` in order to render the login page, so
  those routes skip the auth extractor. The first authenticated
  action the browser takes — typically a `session.list` WebSocket
  call or a `GET /api/metrics` — is where auth is enforced. Do not
  put sensitive content in the static bundle and expect it to be
  hidden; anyone who can reach the gateway can fetch it.

## Cross-links

- [../crates/ryvos-gateway.md](../crates/ryvos-gateway.md) — crate
  reference including the `GatewayServer` builder and `AppState`.
- [gateway-rest.md](gateway-rest.md) — per-endpoint auth documented
  alongside the full request/response shapes.
- [gateway-websocket.md](gateway-websocket.md) — the WebSocket
  upgrade uses the same extractor; per-method role checks are handled
  in-band by the RPC handlers.
- [../operations/configuration.md](../operations/configuration.md) —
  every `ryvos.toml` key including `[gateway]`, `[[gateway.api_keys]]`,
  and `[gateway.webhooks]`.
