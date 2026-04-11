# ryvos-mcp

The integration crate for the **[MCP](../glossary.md#mcp)** protocol.
`ryvos-mcp` has two complementary jobs. As a **client** it connects to
external MCP servers and bridges every tool those servers advertise into the
Ryvos **[tool registry](../glossary.md#tool-registry)**, so a tool from
GitHub's MCP server or a local filesystem server looks identical to a
built-in from the agent's perspective. As a **server** it exposes nine of
Ryvos's own capabilities — Viking memory, file-based memory, and the audit
trail — over stdio so that any MCP-aware client (Claude Code, Claude
Desktop, Cursor, Zed, and so on) can read and write Ryvos state without
speaking a Ryvos-specific protocol.

The dual role is deliberate. MCP is Ryvos's single integration layer; the
alternative would have been a bespoke plugin format for built-in
integrations and a second bespoke protocol for outbound automation. The
decision and its tradeoffs are recorded in
[ADR-008](../adr/008-mcp-integration-layer.md).

## Position in the workspace

`ryvos-mcp` depends on `ryvos-core` (for traits, types, config, and errors),
`ryvos-tools` (to register bridged tools into the registry), and
`ryvos-memory` (to expose the Viking client through the server handler).
The protocol implementation itself comes from the `rmcp` crate (version
0.16), which provides both the client and server sides of MCP, the
`#[tool_router]` and `#[tool_handler]` macros used by the server handler,
and the stdio and streamable-HTTP transports used by the client.

The crate re-exports a small surface from `lib.rs`:

```rust
pub use bridge::register_mcp_tools;
pub use client::McpClientManager;
pub use handler::{McpEvent, RyvosClientHandler};
pub use resource_tool::McpReadResourceTool;

pub use rmcp::model::{PromptMessage, PromptMessageContent, PromptMessageRole};
```

Two top-level helpers round out the public API:
`connect_and_register(manager, server_name, config, registry)` and
`refresh_tools(manager, server_name, registry)`. The first connects to a
server over its configured transport and registers every advertised tool;
the second unregisters the current `mcp__{server}__*` entries and re-lists
tools from the server. Both return the number of tools registered.

## Client side

The client is the path every Ryvos run takes when calling an external
tool. It lives in four files: `client.rs` (the connection manager),
`handler.rs` (the notification handler), `bridge.rs` (the `Tool` trait
impl that wraps a remote tool), and `resource_tool.rs` (a standalone
tool for reading MCP resources).

### `McpClientManager`

`McpClientManager` in `crates/ryvos-mcp/src/client.rs` owns every active
MCP connection. Its state is three fields:

```rust
pub struct McpClientManager {
    connections: Mutex<HashMap<String, McpConnection>>,
    server_configs: Mutex<HashMap<String, McpServerConfig>>,
    event_tx: broadcast::Sender<McpEvent>,
}
```

`connections` maps a server name to a live `RunningService<RoleClient,
RyvosClientHandler>` (the `rmcp` type that wraps an established session);
`server_configs` remembers the original `McpServerConfig` so a reconnect
can replay the transport parameters; and `event_tx` is a
`tokio::sync::broadcast` channel (capacity 64) that `RyvosClientHandler`
uses to emit notification events that the rest of Ryvos can subscribe to.

`connect(name, config)` supports two `McpTransport` variants:

- **Stdio.** The manager spawns the configured command as a child
  process (`tokio::process::Command`), wraps it in an
  `rmcp::transport::TokioChildProcess`, and attaches a
  `RyvosClientHandler` to the resulting service. Command, argv, and
  environment variables come from the `command`, `args`, and `env`
  fields of `McpServerConfig`.
- **Streamable HTTP.** The manager constructs an
  `rmcp::transport::streamable_http_client::StreamableHttpClientTransport`
  with the configured URL and any `headers` from the config merged into
  `custom_headers`. Streamable HTTP is MCP's successor to the older SSE
  transport and handles both one-shot and long-polling sessions over a
  single endpoint.

After either transport is initialized, the handler's `serve(transport)`
call returns a `RunningService`, which is inserted into `connections`
alongside the stored config. The manager also exposes `reconnect`,
`is_connected`, `connected_servers`, and `configured_servers` for
maintenance and health checks.

Tool dispatch goes through `call_tool(server_name, tool_name, arguments)`.
The method tries `call_tool_inner` once; if the result is an error whose
message contains `"closed"` or `"Transport"`, the manager calls
`reconnect(server_name)` and retries exactly once. One retry is the
intentional compromise — it covers the common case of a long-lived stdio
child exiting cleanly between calls without opening the door to infinite
reconnect loops under a persistently broken server. Resource reads go
through `read_resource(server_name, uri)` and follow the same retry
shape.

### `RyvosClientHandler`

`crates/ryvos-mcp/src/handler.rs` implements `rmcp::ClientHandler` for
`RyvosClientHandler`. The handler is the MCP-side callback surface:
every time an upstream server sends a notification (tools changed,
resources changed, prompts changed, resource updated, log message,
progress), one of the trait methods fires. Each notification is
translated into an `McpEvent` and published on the shared broadcast
channel:

```rust
pub enum McpEvent {
    ToolsChanged { server: String },
    ResourcesChanged { server: String },
    PromptsChanged { server: String },
    ResourceUpdated { server: String, uri: String },
    LogMessage { server: String, level: String, message: String },
}
```

The gateway subscribes to these events so the Web UI can reload the tool
list without a page refresh when an upstream server adds, removes, or
renames a tool. The agent itself subscribes indirectly: when
`ToolsChanged` fires for a server, the bootstrap code calls
`refresh_tools` to unregister the old `mcp__{server}__*` entries and
re-register the new set.

`RyvosClientHandler` also handles two request-shaped RPCs. `create_message`
(the MCP "sampling" feature, which lets a server ask its client to run
an LLM completion on its behalf) is deliberately unimplemented and
returns `method_not_found`; sampling is a future extension point but is
not wired to a Ryvos-side LLM client today. `get_info` returns a
minimal `Implementation` block advertising the client as `ryvos` with
the current `CARGO_PKG_VERSION`.

### `McpBridgedTool`

The bridge between an external tool and Ryvos's registry lives in
`crates/ryvos-mcp/src/bridge.rs`. `McpBridgedTool` is a simple `Tool`
impl that holds a pre-computed display name, the original server and
tool names, the cached description and schema, a clone of the
`McpClientManager`, a timeout, and a security tier:

```rust
pub struct McpBridgedTool {
    display_name: String,
    server_name: String,
    tool_name: String,
    description: String,
    schema: serde_json::Value,
    manager: Arc<McpClientManager>,
    timeout: u64,
    security_tier: SecurityTier,
}
```

`display_name` is always `mcp__{server}__{tool}`. The double-underscore
prefix is the single convention Ryvos follows to keep bridged tools from
colliding with built-ins: no built-in starts with `mcp__`, so a
`mcp__filesystem__read_file` never shadows the built-in `read`, and the
audit trail can filter bridged calls by prefix.

`execute` forwards the input JSON object through
`manager.call_tool(server, tool, arguments)` and translates the outcome
into a `ToolResult`. Errors become `ToolResult::error(…)` rather than
propagating as `RyvosError`, so an upstream MCP failure becomes part of
the agent's conversation (where the model can react to it) instead of
aborting the turn.

`register_mcp_tools(registry, manager, server_name, tools, timeout_secs,
tier_override)` is the bulk registration entry point called from
`connect_and_register` and `refresh_tools`. It parses the tier override
(`"T0"` through `"T4"`, case-insensitive, falling back to `T1` when
missing or invalid), and for each advertised tool it constructs a
`McpBridgedTool` and inserts it into the registry. The description
falls back to `"MCP tool: {name}"` when the server does not provide one;
the schema falls back to `{"type":"object"}` when serialization of the
original schema fails.

Tier defaults and overrides matter less than they used to, since the
**[security gate](../glossary.md#security-gate)** no longer blocks on
tier. They still affect how the tool is reported in the audit trail and
in operator tooling, so servers that advertise destructive operations
should be registered with `tier_override = "T3"` to preserve the
informational signal.

### `McpReadResourceTool`

MCP resources are a second surface alongside tools — servers can
advertise named data blobs (files, database rows, document pages) that
the client can fetch but not invoke. Ryvos exposes a single built-in to
reach them: `mcp_read_resource`, defined in
`crates/ryvos-mcp/src/resource_tool.rs`. Its input schema takes a
`server` name and a resource `uri`, and its `execute` method calls
`McpClientManager::read_resource(server, uri)`. The tool is registered
once per daemon and reports `SecurityTier::T0` because resource reads
are strictly read-only.

The gateway also includes resource URIs from every connected server in
the narrative layer of the **[onion context](../glossary.md#onion-context)**,
using `ContextBuilder::with_mcp_resources`. The agent therefore sees a
list of available resources up front and can call
`mcp_read_resource` to fetch the ones it needs without asking the user.

## Server side

The server side lets external MCP clients reach Ryvos. It lives under
`crates/ryvos-mcp/src/server/` in three files: `handler.rs` (the
`RyvosServerHandler` with the nine exposed tools), `audit_reader.rs`
(a read-only SQLite handle on `audit.db`), and `tools/` (the parameter
structs and the thin wrappers that implement each tool's body).

### `RyvosServerHandler`

`RyvosServerHandler` in `crates/ryvos-mcp/src/server/handler.rs` is built
on `rmcp`'s attribute macros:

```rust
#[derive(Clone)]
pub struct RyvosServerHandler {
    pub(crate) viking: Option<Arc<VikingClient>>,
    pub(crate) audit: Option<Arc<AuditReader>>,
    pub(crate) workspace: std::path::PathBuf,
    tool_router: ToolRouter<Self>,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RyvosServerHandler { /* ... */ }

#[tool_router(router = tool_router)]
impl RyvosServerHandler { /* nine #[tool(...)] methods */ }
```

The `#[tool_router]` macro generates a `ToolRouter<Self>` from the
`#[tool(name = "…", description = "…")]` attributes on each method, and
`#[tool_handler]` wires that router into the `ServerHandler` trait. The
net effect is that each `async fn ...(&self, params: Parameters<Foo>)
-> String` method in the `#[tool_router]` impl becomes a registered MCP
tool whose JSON schema is derived from the parameter struct's
`schemars::JsonSchema` derive.

`get_info` advertises the server under `ProtocolVersion::V_2024_11_05`
with tool capabilities enabled, names it `ryvos`, and sets an
instructions string that appears in any MCP client's server list:
`"Ryvos agent memory & audit tools. Use viking_* to read/write
persistent memory, audit_* to inspect tool history."`

### The nine exposed tools

The router exposes exactly nine tools grouped in three families. Each
tool falls back to a human-readable "not available" string when its
dependency is `None`, so the server can be brought up in a minimal mode
before Viking or the audit reader are wired in.

| Tool | Family | Depends on |
|---|---|---|
| `viking_search` | Viking | `Some(viking)` |
| `viking_read` | Viking | `Some(viking)` |
| `viking_write` | Viking | `Some(viking)` |
| `viking_list` | Viking | `Some(viking)` |
| `memory_get` | File memory | workspace |
| `memory_write` | File memory | workspace |
| `daily_log_write` | File memory | workspace |
| `audit_query` | Audit | `Some(audit)` |
| `audit_stats` | Audit | `Some(audit)` |

Parameter structs live in `crates/ryvos-mcp/src/server/tools/mod.rs` and
derive `Deserialize`, `Serialize`, and `JsonSchema`. For example:

```rust
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct VikingSearchParams {
    /// Natural language search query
    pub query: String,
    /// Restrict search to a viking:// directory (optional)
    pub directory: Option<String>,
    /// Max results (default 10)
    pub limit: Option<usize>,
}
```

The doc comments on each field become the tool schema's `description`
text that the calling client sees. The actual tool bodies are thin
adapters in `crates/ryvos-mcp/src/server/tools/viking.rs`,
`memory.rs`, and `audit.rs`: they unwrap the `Parameters<_>` wrapper,
call the appropriate method on `VikingClient`, `AuditReader`, or the
workspace file helpers, and return a plain `String`. The macros handle
schema generation, error translation, and the MCP response framing.

### `AuditReader`

The server needs a live view of `audit.db` while the main daemon is
still writing to it. `AuditReader` in
`crates/ryvos-mcp/src/server/audit_reader.rs` opens a SQLite connection
with `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX`:

```rust
pub struct AuditReader {
    conn: Mutex<rusqlite::Connection>,
}
```

Because `audit.db` is opened in WAL mode by the writer (see
`crates/ryvos-agent/src/audit.rs`), concurrent readers are safe without
any extra coordination — readers see a consistent snapshot and never
block writers. `AuditReader` exposes two methods: `recent_entries(limit)`
(rows ordered by `timestamp DESC`) and `tool_counts()` (group-by over
`tool_name`, ordered by count). These are the bodies behind
`audit_query` and `audit_stats`.

### Running the server

The daemon runs the MCP server as a subcommand:

```bash
ryvos mcp-server
```

This launches an stdio MCP server bound to the same Viking and audit
stores as the main daemon, so an external CLI client can read and
write Ryvos memory while the daemon continues to serve its other
channels. The typical deployment attaches this subcommand to Claude
Code, Claude Desktop, or Cursor through that client's MCP settings
file; see [../guides/wiring-an-mcp-server.md](../guides/wiring-an-mcp-server.md)
for the config snippets.

The REST-level API reference — the exact schema of each exposed tool,
the response shapes, and the common error cases — is in
[../api/mcp-server.md](../api/mcp-server.md). The broader MCP wire
format is described in the `rmcp` crate's documentation and in the MCP
specification linked from the ADR.

## Where to go next

For the agent-side view of how bridged tools are dispatched alongside
built-ins, read [../internals/tool-registry.md](../internals/tool-registry.md).
For the exact transport selection logic, capability negotiation, and
proxying story, read [../internals/mcp-bridge.md](../internals/mcp-bridge.md).
For the list of tools Ryvos exposes when acting as a server and their
response schemas, read [../api/mcp-server.md](../api/mcp-server.md).
For the rationale behind MCP as the integration layer rather than a
bespoke plugin format, read [ADR-008](../adr/008-mcp-integration-layer.md).
