# MCP bridge

Ryvos is both an **[MCP](../glossary.md#mcp)** server and an MCP client. As a
server, it exposes nine of its own tools (Viking memory, audit queries,
memory writes, daily logs) over the same protocol that external assistants
like Claude Code and Copilot use to discover tools. As a client, it connects
to any external MCP server — filesystem, GitHub, Notion, Gmail, a bespoke
internal server — and bridges every tool that server exposes into Ryvos's
own **[tool registry](../glossary.md#tool-registry)**, so the LLM sees an
external tool as indistinguishable from a built-in one.

This document covers the client side: the bridge. It walks the
`ryvos-mcp` crate at `crates/ryvos-mcp/` module by module, describes the
two supported transports, the connection manager, the event handler, the
bridged-tool shim, and the dynamic refresh pipeline. The server side is
documented in [../api/mcp-server.md](../api/mcp-server.md). For the
architectural motivation, see
[../adr/008-mcp-integration-layer.md](../adr/008-mcp-integration-layer.md).
For configuration, see [../guides/wiring-an-mcp-server.md](../guides/wiring-an-mcp-server.md).

## Why a bridge

The alternative to bridging is to teach every LLM provider about MCP
directly. That is a losing proposition: each provider has a different
tool-use format, some (like the legacy OpenAI function-calling API) do
not even carry enough metadata to express MCP's capabilities, and the
set of providers is growing. Bridging instead unifies the inputs: every
MCP tool is wrapped as an `McpBridgedTool` that implements Ryvos's
`Tool` trait, and the registry feeds `ToolDefinition` entries to the
LLM just like any other tool. The provider does not need to know MCP
exists.

The bridge also lets Ryvos apply uniform policy to MCP tools. Audit,
**[SafetyMemory](../glossary.md#safetymemory)**, soft checkpoints,
timeouts, and **[passthrough](../glossary.md#passthrough-security)**
execution all apply to MCP tools automatically — because every tool
call goes through the same `SecurityGate` dispatcher documented in
[tool-registry.md](tool-registry.md). A destructive tool from a third-
party MCP server is treated the same as a destructive built-in: not
blocked by tier, but audited and learned from.

## Transports

The MCP specification supports two transports that matter in practice:
stdio (parent process talks to a child over pipes) and Streamable HTTP
(an HTTP connection that upgrades to a streaming event channel). Ryvos
supports both through the `rmcp` crate. See
`crates/ryvos-mcp/src/client.rs:56`:

```rust
let client = match &config.transport {
    McpTransport::Stdio { command, args, env } => {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let transport = rmcp::transport::TokioChildProcess::new(cmd)
            .map_err(|e| RyvosError::Mcp(format!("Failed to spawn {}: {}", command, e)))?;
        handler.serve(transport).await
            .map_err(|e| RyvosError::Mcp(format!("MCP init for '{}' failed: {}", name, e)))?
    }
    McpTransport::Sse { url } => {
        let custom_headers = config.headers.iter()
            .filter_map(|(k, v)| {
                let name = HeaderName::from_bytes(k.as_bytes()).ok()?;
                let value = HeaderValue::from_str(v).ok()?;
                Some((name, value))
            })
            .collect();
        let transport_config = StreamableHttpClientTransportConfig {
            uri: url.as_str().into(),
            custom_headers,
            ..Default::default()
        };
        let transport = StreamableHttpClientTransport::from_config(transport_config);
        <RyvosClientHandler as ServiceExt<RoleClient>>::serve(handler, transport).await
            .map_err(|e| RyvosError::Mcp(format!("MCP init for '{}' failed: {}", name, e)))?
    }
};
```

Stdio is the common case. Most MCP servers ship as CLI binaries that
read newline-delimited JSON-RPC on stdin and write on stdout. Ryvos
spawns the command with the configured args and environment, hands
`rmcp` the child's stdio handles through `TokioChildProcess`, and
`rmcp` does the framing. Environment variables are how stdio servers
receive secrets (API tokens, workspace paths), because there is no
other place to put them.

Streamable HTTP is the right transport when the server is not local:
an MCP server running behind an HTTPS endpoint, a hosted Notion
gateway, an internal service. The `McpTransport::Sse` variant is
misleadingly named — it was originally just SSE, but Ryvos now uses
`StreamableHttpClientTransport`, which is the MCP specification's
current designation for the HTTP transport. Custom headers flow from
`config.headers` into the transport config, which is how OAuth bearer
tokens and custom API keys reach the server.

Either way, the client is a `RunningService<RoleClient, RyvosClientHandler>`.
This is `rmcp`'s handle for a live MCP session: it owns the transport,
the message-routing task, and the handler callbacks. It exposes the
client-side RPC methods (`list_all_tools`, `call_tool`, `read_resource`,
and so on) and it closes the transport when dropped.

## McpClientManager

`McpClientManager` at `crates/ryvos-mcp/src/client.rs:25` is the owner
of every live connection:

```rust
pub struct McpClientManager {
    connections: Mutex<HashMap<String, McpConnection>>,
    server_configs: Mutex<HashMap<String, McpServerConfig>>,
    event_tx: broadcast::Sender<McpEvent>,
}
```

Three fields. `connections` is the map of server name to running
service; `server_configs` is the stored config for each named server
so `reconnect` can redial without the daemon re-parsing `ryvos.toml`;
`event_tx` is a broadcast channel of `McpEvent` values produced by the
handler callbacks on every connected server.

`connect` at `crates/ryvos-mcp/src/client.rs:53` is the entry point.
It builds a `RyvosClientHandler` tagged with the server name, spins up
the transport, runs `rmcp`'s initialization handshake through
`handler.serve`, and inserts the resulting service and config into the
two maps. The handshake is where capability negotiation happens:
`rmcp` sends the client's `ClientInfo` (including the Ryvos version
from `CARGO_PKG_VERSION`) and receives the server's capabilities and
tool list metadata.

`reconnect` at `crates/ryvos-mcp/src/client.rs:115` is the recovery
path. It pulls the stored config, removes the old entry (calling
`close` on the zombie service if it is still alive), and calls
`connect` again under the same server name. This is idempotent: if the
connection is already healthy, `reconnect` still replaces it. The
method is triggered automatically inside `call_tool` on transport
errors and manually from operator tooling.

`is_connected`, `connected_servers`, `configured_servers`, and
`get_config` are read-only accessors used by the Web UI and the
operator CLI to present the connection state. `disconnect` and
`disconnect_all` close services cleanly during shutdown.

## Tools, resources, and prompts

The client manager exposes the three MCP concepts the agent cares
about. Tools are methods the LLM can invoke. Resources are read-only
data the agent can fetch by URI (like filesystem files or Notion
pages). Prompts are pre-composed user messages the server suggests for
specific workflows.

`list_tools`, `list_resources`, and `list_prompts` all delegate to
`rmcp`'s corresponding paginating methods (`list_all_tools`, and so
on) and translate errors into `RyvosError::Mcp`. The implementations
are all a handful of lines; the interesting one is `call_tool` at
`crates/ryvos-mcp/src/client.rs:182`:

```rust
pub async fn call_tool(
    &self,
    server_name: &str,
    tool_name: &str,
    arguments: Option<serde_json::Map<String, serde_json::Value>>,
) -> Result<String, RyvosError> {
    let result = self.call_tool_inner(server_name, tool_name, arguments.clone()).await;

    if let Err(ref e) = result {
        let err_str = e.to_string();
        if err_str.contains("closed") || err_str.contains("Transport") {
            warn!(server = %server_name, "MCP transport closed, attempting reconnect");
            if self.reconnect(server_name).await.is_ok() {
                return self.call_tool_inner(server_name, tool_name, arguments).await;
            }
        }
    }

    result
}
```

The outer function wraps `call_tool_inner` with a single automatic
retry. If the inner call fails with an error string that mentions
"closed" or "Transport", the manager assumes the underlying transport
has dropped (stdio child died, HTTP connection reset, process
crashed) and tries a single reconnect. If the reconnect succeeds, the
original call is retried; otherwise the original error bubbles up.

The retry is limited to one attempt on purpose. MCP tool calls are
not guaranteed idempotent — a `send_email` or a `delete_file` should
not silently retry if the first attempt might have partially
succeeded. One retry covers the transport-crash case (which is pre-
execution, so a retry is safe) without opening the door to double-
execution during a flaky network.

`call_tool_inner` at `crates/ryvos-mcp/src/client.rs:208` is the
straight-through path: look up the running service, build a
`CallToolRequestParams`, call `client.call_tool(params)`, and
flatten the content array into a string. Non-text content (images,
blobs) is stringified via `Debug` as a fallback; most MCP tools
return text, so this is rarely hit. Resource reads at
`crates/ryvos-mcp/src/client.rs:267` follow the same shape: build
`ReadResourceRequestParams`, call `read_resource`, flatten the
contents, and preserve a `[blob: N bytes]` placeholder for binary
entries.

## The client handler

Every connected server gets a `RyvosClientHandler` instance, which
is the `ClientHandler` implementation Ryvos hands to `rmcp`. See
`crates/ryvos-mcp/src/handler.rs:37`:

```rust
pub struct RyvosClientHandler {
    server_name: String,
    event_tx: broadcast::Sender<McpEvent>,
}
```

Two fields. The name is used to tag every emitted event so the
subscriber can tell which server fired the notification. The
`event_tx` is cloned from the manager's own broadcast channel, so
every handler on every server publishes into the same stream.

The handler implements six `ClientHandler` trait methods, each of
which is called by `rmcp` when the server sends a notification or
a request of the matching type:

- `on_tool_list_changed` fires an `McpEvent::ToolsChanged` — the
  server is telling the client that its tool list has changed and
  the client should re-list.
- `on_resource_list_changed` fires an `McpEvent::ResourcesChanged`.
- `on_prompt_list_changed` fires an `McpEvent::PromptsChanged`.
- `on_resource_updated(uri)` fires an `McpEvent::ResourceUpdated`
  with the specific URI that changed — used by servers that
  implement resource subscription.
- `on_logging_message(params)` fires an `McpEvent::LogMessage` with
  the level and body — used by servers that emit diagnostic logs
  over the protocol.
- `on_progress(params)` logs the progress details but does not emit
  an event; progress updates are currently informational only.

The handler also implements `create_message`, which is the
server-to-client sampling request (an MCP server asking the client
to run an LLM prompt on its behalf). Ryvos does not support
sampling yet and returns `method_not_found`. This is a TODO in the
protocol sense — supporting sampling would let MCP servers use the
daemon's LLM, which is a powerful but security-sensitive feature.

`get_info` returns a `ClientInfo` struct with name `"ryvos"`, the
crate version from `env!("CARGO_PKG_VERSION")`, and default
capabilities. `rmcp` sends this during initialization so the
server can optionally present different behavior to different
clients.

## McpEvent

`McpEvent` at `crates/ryvos-mcp/src/handler.rs:14` is a simple enum
with five variants:

```rust
pub enum McpEvent {
    ToolsChanged { server: String },
    ResourcesChanged { server: String },
    PromptsChanged { server: String },
    ResourceUpdated { server: String, uri: String },
    LogMessage { server: String, level: String, message: String },
}
```

These are *not* `AgentEvent`s — they ride their own broadcast
channel inside the MCP manager, not the main `EventBus`. The
separation keeps MCP-specific traffic from polluting the main bus
(every connected server produces at least log messages), and it
lets consumers subscribe to MCP events without also subscribing to
the full agent event stream.

## connect_and_register

`connect_and_register` at `crates/ryvos-mcp/src/lib.rs:39` is the
high-level entry point used at daemon startup. It glues the
connection manager to the tool registry:

```rust
pub async fn connect_and_register(
    manager: &Arc<McpClientManager>,
    server_name: &str,
    config: &McpServerConfig,
    registry: &mut ToolRegistry,
) -> Result<usize, RyvosError> {
    manager.connect(server_name, config).await?;

    let tools = manager.list_tools(server_name).await?;
    let count = tools.len();

    bridge::register_mcp_tools(
        registry,
        manager,
        server_name,
        &tools,
        config.timeout_secs,
        config.tier_override.as_deref(),
    );

    Ok(count)
}
```

Three steps. First, connect and negotiate capabilities. Second,
list the server's tools. Third, wrap each one as an
`McpBridgedTool` and insert it into the registry. The return value
is the tool count so the caller can log `"Connected to notion (37
tools)"`.

The daemon calls `connect_and_register` for every entry in the
`[mcp.servers]` table of `ryvos.toml` during bootstrap. Failures
are caught and logged; a single broken server does not prevent the
rest of the daemon from starting.

## McpBridgedTool

`McpBridgedTool` at `crates/ryvos-mcp/src/bridge.rs:32` is the
`Tool` implementation that every MCP tool appears as inside the
registry:

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

`display_name` is the full Ryvos-facing name (`mcp__notion__search_pages`).
`server_name` and `tool_name` are the components used to route the
`call_tool` invocation back through the manager. `description` and
`schema` are the LLM-facing metadata, copied once from the MCP
listing and reused for every call. `manager` is the shared
`Arc<McpClientManager>` that owns the connection. `timeout` and
`security_tier` are per-server config knobs.

The `Tool` impl at `crates/ryvos-mcp/src/bridge.rs:43` is almost
trivial:

```rust
impl Tool for McpBridgedTool {
    fn name(&self) -> &str { &self.display_name }
    fn description(&self) -> &str { &self.description }
    fn input_schema(&self) -> serde_json::Value { self.schema.clone() }

    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        let server = self.server_name.clone();
        let tool = self.tool_name.clone();
        let manager = self.manager.clone();

        Box::pin(async move {
            let arguments = input.as_object().cloned();
            match manager.call_tool(&server, &tool, arguments).await {
                Ok(content) => Ok(ToolResult::success(content)),
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        })
    }

    fn timeout_secs(&self) -> u64 { self.timeout }
    fn tier(&self) -> SecurityTier { self.security_tier }
}
```

The `ToolContext` is dropped — MCP tools do not receive the
session id, working directory, or other runtime state, because
their execution is entirely delegated to the remote server and the
remote has no way to use that context anyway. If an MCP tool needs
session scoping, it has to carry the session id in its arguments.

Errors from `call_tool` are translated to
`ToolResult::error(string)` rather than bubbled up as
`RyvosError`. This matters because the registry dispatcher treats
a `ToolResult { is_error: true }` as "the tool ran but produced an
error," which the LLM can see and react to; it treats a
`Result::Err` as "the tool failed to run," which the agent loop
surfaces differently. MCP-level failures — a server that rejected
an argument, a remote tool that returned an error — are almost
always the kind the LLM should learn from, so presenting them as
tool-level errors is the right default.

## register_mcp_tools

`register_mcp_tools` at `crates/ryvos-mcp/src/bridge.rs:87` is the
loop that turns a list of `rmcp::model::Tool` entries into
`McpBridgedTool` entries in the registry:

```rust
pub fn register_mcp_tools(
    registry: &mut ToolRegistry,
    manager: &Arc<McpClientManager>,
    server_name: &str,
    tools: &[McpTool],
    timeout_secs: u64,
    tier_override: Option<&str>,
) {
    let security_tier = tier_override
        .and_then(|t| match t.to_uppercase().as_str() {
            "T0" => Some(SecurityTier::T0),
            "T1" => Some(SecurityTier::T1),
            "T2" => Some(SecurityTier::T2),
            "T3" => Some(SecurityTier::T3),
            "T4" => Some(SecurityTier::T4),
            _ => None,
        })
        .unwrap_or(SecurityTier::T1);

    for tool in tools {
        let display_name = format!("mcp__{}__{}", server_name, tool.name);
        // ... build bridged, registry.register(bridged);
    }
}
```

The tier override is a per-server config knob: it lets an operator
tag a server's tools uniformly (e.g., `T0` for a read-only Notion
server, `T3` for an admin server). Invalid strings fall back to
`T1`. The tier is informational under passthrough security but is
still shown in the Web UI and stored in the audit trail, so
setting it correctly makes post-hoc analysis cleaner.

The display name format `mcp__{server}__{tool}` is deliberate.
Double underscores avoid collisions with real tool names (no
built-in uses double underscores), the `mcp__` prefix makes MCP
tools filterable in logs and dashboards, and the server segment
prevents two servers from colliding when they expose tools with
the same upstream name.

Empty descriptions fall back to `"MCP tool: {name}"` so the LLM
always receives *something* for `ToolDefinition::description`.
Some MCP servers do not populate descriptions for every tool, and
an empty string would confuse the model.

## refresh_tools

`refresh_tools` at `crates/ryvos-mcp/src/lib.rs:65` is the dynamic
update path. When a server fires `ToolsChanged`, the daemon's MCP
event listener calls `refresh_tools` for that server, which:

1. Lists every currently-registered tool name.
2. Filters to the `mcp__{server_name}__` prefix.
3. Calls `registry.unregister` on each match.
4. Re-fetches the tool list from the server via `manager.list_tools`.
5. Re-registers each tool as an `McpBridgedTool`.

```rust
pub async fn refresh_tools(
    manager: &Arc<McpClientManager>,
    server_name: &str,
    registry: &mut ToolRegistry,
) -> Result<usize, RyvosError> {
    let prefix = format!("mcp__{}__", server_name);
    let to_remove: Vec<String> = registry
        .list()
        .into_iter()
        .filter(|name| name.starts_with(&prefix))
        .map(|s| s.to_string())
        .collect();

    for name in &to_remove {
        registry.unregister(name);
    }

    let tools = manager.list_tools(server_name).await?;
    let count = tools.len();

    let config = manager.get_config(server_name).await;
    let (timeout, tier) = config
        .as_ref()
        .map(|c| (c.timeout_secs, c.tier_override.as_deref()))
        .unwrap_or((120, None));

    bridge::register_mcp_tools(registry, manager, server_name, &tools, timeout, tier);
    Ok(count)
}
```

The unregister-then-reregister pattern handles the general case:
tools may have been added, removed, or had their descriptions or
schemas updated. A diff-based update would be more efficient but
also more fragile; the full replacement is 50 lines of code and
only runs on explicit notifications, so the cost is negligible.

## Wiring the event listener

The MCP event listener lives in the daemon bootstrap at
`src/main.rs:473`:

```rust
if let Some(ref manager) = mcp_manager {
    let mut event_rx = manager.subscribe_events();
    let tools_for_events = tools.clone();
    let manager_for_events = manager.clone();
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            match event {
                ryvos_mcp::McpEvent::ToolsChanged { server } => {
                    let mut registry = tools_for_events.write().await;
                    match ryvos_mcp::refresh_tools(&manager_for_events, &server, &mut registry).await {
                        Ok(count) => info!(server = %server, tools = count, "MCP tools refreshed"),
                        Err(e) => error!(server = %server, error = %e, "Failed to refresh"),
                    }
                }
                ryvos_mcp::McpEvent::ResourcesChanged { server } => { /* log only */ }
                // ...
            }
        }
    });
}
```

A single background task owns the subscription for the lifetime of
the daemon. On a `ToolsChanged`, it takes the write lock on the
registry and runs `refresh_tools`. On the other variants, it
currently logs and moves on: resource and prompt lists are not
reflected into the registry (they are not tools), and log messages
are informational. The design leaves room for future subscribers
that care about resource updates (e.g., invalidating a cache of
`mcp_read_resource` results).

## McpReadResourceTool

Resources are not tools, but they need to be callable by the LLM,
so the bridge exposes one built-in tool — `mcp_read_resource` —
that takes a server name and a URI and delegates to
`McpClientManager::read_resource`. See
`crates/ryvos-mcp/src/resource_tool.rs:13`:

```rust
pub struct McpReadResourceTool {
    manager: Arc<McpClientManager>,
}
```

Its input schema requires `server` and `uri`:

```rust
fn input_schema(&self) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "server": { "type": "string" },
            "uri":    { "type": "string" }
        },
        "required": ["server", "uri"]
    })
}
```

Its `execute` pulls the two fields out of the JSON, validates they
are non-empty, and calls `manager.read_resource`. `tier()` returns
`SecurityTier::T0` (read-only) and `timeout_secs` is 120 — long
enough for a slow HTTP MCP server but short enough to avoid
hanging the turn forever. The tool is registered once at daemon
startup, not per-server, so the LLM has one consistent way to
fetch resources regardless of how many MCP servers are connected.

## End-to-end example

A full trace through the bridge looks like this. The user has
configured a Notion MCP server in `ryvos.toml`:

```toml
[mcp.servers.notion]
transport = "stdio"
command = "npx"
args = ["@modelcontextprotocol/server-notion"]
env = { NOTION_TOKEN = "${NOTION_TOKEN}" }
```

At daemon startup, `connect_and_register` spawns `npx
@modelcontextprotocol/server-notion` with `NOTION_TOKEN` in its
environment, negotiates capabilities, and lists the server's tools
— say, `search_pages`, `read_page`, `create_page`, and
`query_database`. Each is wrapped as an `McpBridgedTool` with
display names `mcp__notion__search_pages`, `mcp__notion__read_page`,
and so on, and inserted into the registry.

On a later user turn, the LLM receives the full tool list (built-ins
plus the four Notion tools) and decides to call
`mcp__notion__search_pages` with `{"query": "Q3 roadmap"}`. The
agent runtime dispatches the call through `SecurityGate::execute`,
which logs the invocation, checks SafetyMemory for relevant
lessons, and then calls the bridged tool's `execute`. The bridge
translates the name back to `("notion", "search_pages")`, calls
`manager.call_tool`, which forwards a JSON-RPC `tools/call` to the
stdio child, receives the response, and returns the flattened text.
The dispatch finishes, the result flows back into the next turn's
message list, and the loop continues.

If the Notion server later notifies `tools/list_changed` because a
new tool was added, the handler fires `McpEvent::ToolsChanged`,
the daemon's event listener picks it up, `refresh_tools` runs, the
new tool appears in the registry, and the next turn's tool list
includes it.

If the server crashes, the next call to `call_tool` returns a
transport error, the outer wrapper reconnects, the retried call
succeeds (or fails permanently if the crash was non-transient),
and the LLM sees either a normal result or a clean error — never a
zombie hang.

## Where to go next

- [tool-registry.md](tool-registry.md) — the registry that holds
  `McpBridgedTool` entries alongside built-ins and skills.
- [../crates/ryvos-mcp.md](../crates/ryvos-mcp.md) — the full
  crate reference, including the MCP server side.
- [../api/mcp-server.md](../api/mcp-server.md) — the nine tools
  Ryvos exposes when running its own MCP server.
- [../guides/wiring-an-mcp-server.md](../guides/wiring-an-mcp-server.md)
  — configuration recipes for common MCP servers (filesystem,
  GitHub, Notion, Slack).
- [../adr/008-mcp-integration-layer.md](../adr/008-mcp-integration-layer.md)
  — why MCP is the integration layer and not a bespoke plugin API.
