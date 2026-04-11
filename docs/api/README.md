# API reference

This section documents every external surface Ryvos exposes over the wire.
The five files below are the authoritative contract for anything that talks
to a running Ryvos daemon from the outside: a browser, a mobile app, a shell
script, a **[channel adapter](../glossary.md#channel-adapter)**, another agent,
or a general-purpose MCP client like Claude Code. They describe the HTTP
surface served by `ryvos-gateway`, the WebSocket surface on `/ws`, the nine
tools exposed by `ryvos-mcp` when it is acting as a server, the role-based
access model that protects every endpoint, and the payload schema for
outbound webhooks.

The docs here deliberately stop at the wire format. Why a given endpoint
exists, how it is implemented inside the process, and how it interacts with
the **[EventBus](../glossary.md#eventbus)** and the **[agent runtime](../glossary.md#agent-runtime)**
are covered by [../crates/ryvos-gateway.md](../crates/ryvos-gateway.md) and
[../crates/ryvos-mcp.md](../crates/ryvos-mcp.md). Internals that cross
multiple crates — the event bus itself, the session manager, the approval
broker — live under [../internals/](../internals/). Read those when the API
reference is not enough to understand the runtime behavior behind a call.

Every endpoint documented here is pinned to Ryvos v0.8.3. The WebSocket
frame schema and the REST URL paths are stable within a minor release; the
event translation table and the integration provider list may grow in
later minor releases, and such additions are called out in
[../../CHANGELOG.md](../../CHANGELOG.md).

## Files in this section

| File | Purpose |
|---|---|
| [gateway-rest.md](gateway-rest.md) | The 40+ REST endpoints served by `ryvos-gateway`: sessions, metrics, audit, Viking, config, cron, budget, model, integrations, goals, skills, heartbeat, approvals, webhooks. |
| [gateway-websocket.md](gateway-websocket.md) | The `/ws` WebSocket protocol: `ClientFrame`/`ServerResponse`/`ServerEvent` shapes, the five RPC methods, the per-connection **[lane](../glossary.md#lane)** queue, and the full 23-variant `AgentEvent` to `ServerEvent` translation table. |
| [mcp-server.md](mcp-server.md) | The nine tools Ryvos exposes when run as `ryvos mcp-server`: four Viking tools, three file-memory tools, and two audit-trail tools, with parameter schemas. |
| [auth-and-rbac.md](auth-and-rbac.md) | The four-step authentication precedence chain, the three roles (Viewer, Operator, Admin), and how to configure `[[gateway.api_keys]]` entries in `ryvos.toml`. |
| [webhook-format.md](webhook-format.md) | The outbound payload schema that Ryvos sends to a `callback_url` after an inbound `POST /api/hooks/wake` completes. |

## What is not here

Two categories of API documentation live outside this section because the
audience is different.

The **Rust type documentation** — every public struct, trait, and function
signature in the workspace — is produced by `cargo doc --workspace --open`
and is the primary reference for contributors writing Rust code against
`ryvos-core` or any other workspace crate. The files in this section refer
to those types by name (for example, `ClientFrame`, `ApprovalBroker`,
`RyvosServerHandler`) and point at specific source locations like
`crates/ryvos-gateway/src/protocol.rs:4`, but they do not attempt to
reproduce rustdoc output.

The **channel adapter protocols** — Telegram's Bot API, Discord's Gateway,
Slack's Events API, the Meta WhatsApp Cloud API — are not Ryvos APIs at
all; they are vendor contracts that `ryvos-channels` consumes. See
[../crates/ryvos-channels.md](../crates/ryvos-channels.md) for how each
adapter bridges its upstream protocol to the agent runtime.

## Reading path

If you are building a Web UI, a dashboard, or a CLI client that talks
HTTP, start at [gateway-rest.md](gateway-rest.md) and treat
[auth-and-rbac.md](auth-and-rbac.md) as the companion contract. Every
protected REST endpoint requires one of the three roles, and the exact
role is listed in the endpoint table.

If you are building a live dashboard that streams token deltas and
tool-call progress, the WebSocket surface on `/ws` is the right choice;
start at [gateway-websocket.md](gateway-websocket.md). The REST API is
a strict subset of what the WebSocket exposes — every RPC method has a
REST equivalent — so HTTP-only clients can still do everything, just
without streaming.

If you are wiring Ryvos into an MCP-aware client such as Claude Code,
Claude Desktop, or Cursor, read [mcp-server.md](mcp-server.md) and the
walkthrough at [../guides/wiring-an-mcp-server.md](../guides/wiring-an-mcp-server.md).
That path uses stdio, not HTTP, and does not go through the gateway at
all.

If you are integrating Ryvos into an external pipeline that should be
notified when a triggered run completes, read
[webhook-format.md](webhook-format.md) alongside the `POST /api/hooks/wake`
entry in [gateway-rest.md](gateway-rest.md). The two documents together
describe the full inbound-and-outbound loop.

## Cross-links

- [../crates/ryvos-gateway.md](../crates/ryvos-gateway.md) — crate-level
  reference for the HTTP server, including the `GatewayServer` builder,
  `AppState`, the OAuth bridge, and the embedded Svelte UI.
- [../crates/ryvos-mcp.md](../crates/ryvos-mcp.md) — crate-level reference
  for both the MCP client and the MCP server.
- [../adr/008-mcp-integration-layer.md](../adr/008-mcp-integration-layer.md) —
  the decision record for using MCP as the integration layer.
- [../adr/007-embedded-svelte-web-ui.md](../adr/007-embedded-svelte-web-ui.md) —
  the decision record for embedding the Web UI in the binary.
