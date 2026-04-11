# Wiring an MCP server

## When to use this guide

Three extension points connect Ryvos to new functionality: built-in
tools, **[skills](../glossary.md#skill)**, and
**[MCP](../glossary.md#mcp)** servers. Pick an MCP server when:

- The integration already exists as an MCP server — hundreds of them
  do, from GitHub and Filesystem to Notion, Slack, Gmail, and every
  database vendor.
- The tool needs to run in a separate process for isolation — an MCP
  server lives in its own address space, with its own dependencies,
  and can be restarted without touching the Ryvos daemon.
- You want the same tool available to any MCP-aware client (Claude
  Code, Claude Desktop, Cursor, Zed, Continue), not just Ryvos. ADR-008
  records MCP as Ryvos's single integration layer for exactly this
  reason: a tool that speaks MCP works everywhere.
- The tool is hot-reloadable. MCP servers emit `ToolsChanged`
  notifications when they add or remove tools; Ryvos refreshes its
  registry without restarting.

If the logic is tightly coupled to Ryvos types (session id,
`ToolContext`, the event bus), or if the tool needs to share the
daemon's memory for performance, choose a [built-in
tool](adding-a-tool.md) instead. If the tool is a one-off script in
another language that does not warrant an MCP server, a
[skill](adding-a-skill.md) is simpler.

The crate-level reference is [`ryvos-mcp`](../crates/ryvos-mcp.md); the
transport selection and capability negotiation details are in
[../internals/mcp-bridge.md](../internals/mcp-bridge.md).

## Configuration: `ryvos.toml`

The `[mcp]` section of `ryvos.toml` lists every external MCP server
Ryvos should connect to at startup. Each server is a named subtable
under `[mcp.servers]`:

```toml
[mcp.servers.filesystem]
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/me/workspace"]
auto_connect = true
timeout_secs = 30
tier_override = "T1"

[mcp.servers.github]
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "${GITHUB_TOKEN}" }
auto_connect = true
tier_override = "T2"

[mcp.servers.custom]
transport = "sse"
url = "https://mcp.example.com/sse"
headers = { Authorization = "Bearer ${EXAMPLE_API_TOKEN}" }
auto_connect = true
timeout_secs = 60
allow_sampling = false
```

Field semantics:

- `transport` — either `"stdio"` (the server is a local subprocess) or
  `"sse"` / `"streamable_http"` (the server is reachable over HTTP).
- `command` / `args` / `env` — for stdio transport only. The manager
  spawns the command as a child process and speaks MCP over its
  stdin/stdout. Environment variables are passed through exactly as
  written; `${VAR}` expansion is handled by `AppConfig::load`.
- `url` / `headers` — for HTTP transport only. Headers merge into
  `custom_headers` on the transport builder.
- `auto_connect` — if `true`, the daemon connects at startup. If
  `false`, the server is configured but idle until `/mcp connect
  <name>` is invoked from the REPL or the Web UI.
- `allow_sampling` — whether Ryvos should honor the MCP sampling
  request (the server asks the client to run an LLM completion on its
  behalf). Currently unimplemented; setting this to `true` has no
  effect today.
- `timeout_secs` — how long the registry waits for any single tool
  call from this server before aborting.
- `tier_override` — informational
  **[T0–T4](../glossary.md#t0t4)** label applied to every tool from
  this server. The **[security gate](../glossary.md#security-gate)**
  does not block on tier, but the audit trail uses it to categorize
  calls.

## Alternative: `.mcp.json`

Ryvos also reads an OpenClaw-compatible `.mcp.json` file from the
workspace root. The format matches what Claude Code, Cursor, and Zed
write, so a file prepared for another MCP client works without
modification:

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/me/workspace"]
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_..."
      }
    }
  }
}
```

The `ryvos.toml` and `.mcp.json` sources are merged at startup — the
TOML side wins on conflicts. Keep one source as the canonical list and
treat the other as a convenience for sharing configs with other
clients.

## What Ryvos does when it connects

1. The `McpClientManager` in
   `crates/ryvos-mcp/src/client.rs` resolves the transport: a
   `TokioChildProcess` for stdio, a `StreamableHttpClientTransport` for
   HTTP. For stdio, it spawns `command` with `args` and `env` set on
   the child; for HTTP, it sends the initial handshake to `url` with
   the configured `headers`.
2. The `rmcp` protocol handshake negotiates capabilities. Ryvos
   advertises itself as `ryvos` with the current `CARGO_PKG_VERSION`
   and requests tool and resource listings.
3. The manager calls `list_tools` and receives the server's tool
   catalog. Each entry has a name, a description, and a JSON Schema
   for its input.
4. `register_mcp_tools` wraps each advertised tool in an
   `McpBridgedTool` and inserts it into the
   **[tool registry](../glossary.md#tool-registry)** under the display
   name `mcp__{server}__{tool}`. The double-underscore prefix prevents
   collisions with built-ins: a `mcp__filesystem__read_file` tool never
   shadows the built-in `read` tool, and the audit trail can filter
   bridged calls by prefix.
5. The manager subscribes to the server's notification stream. When
   the server emits `ToolsChanged`, the handler publishes an
   `McpEvent::ToolsChanged` on Ryvos's broadcast channel; the
   bootstrap code calls `refresh_tools` to unregister the stale
   `mcp__{server}__*` entries and re-list. This is how a server that
   adds a new tool at runtime reaches the agent without a restart.
6. `ResourceUpdated`, `ResourcesChanged`, and `PromptsChanged`
   notifications fire similar events. The gateway's WebSocket
   broadcasts them to the Web UI so the tool and resource lists
   re-render live.

## REPL commands

Five slash commands in the REPL manage connected servers:

- `/mcp status` prints every configured server, whether it is
  connected, and the count of tools it advertises.
- `/mcp list` lists every bridged tool across every connected server
  with its description.
- `/mcp connect <name>` attaches to a server that was configured with
  `auto_connect = false` or that was disconnected at runtime.
- `/mcp disconnect <name>` tears down the connection and unregisters
  its tools.
- `/mcp tools <name>` lists tools from a specific server only.

The gateway exposes the same operations through `/api/mcp/*` REST
endpoints and through Web UI buttons.

## Example: filesystem server

The simplest end-to-end example uses the official Filesystem MCP
server, which is an npm package. Add to `ryvos.toml`:

```toml
[mcp.servers.filesystem]
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/me/workspace"]
auto_connect = true
```

Start the daemon. `/mcp list` should show tools like
`mcp__filesystem__read_file`, `mcp__filesystem__write_file`,
`mcp__filesystem__list_directory`, `mcp__filesystem__create_directory`,
and `mcp__filesystem__search_files`. Ask the agent:
`ryvos run "list the files in /tmp"`. The model picks
`mcp__filesystem__list_directory` out of the tool catalog and calls
it; the bridge forwards the call through the stdio transport to the
npx-spawned subprocess; the subprocess returns the listing; the
registry wraps it as a `ToolResult`; the audit trail records the
`mcp__filesystem__list_directory` entry; the agent formats the result
into a user-facing reply.

## Example: custom HTTP server

For a custom server that speaks streamable HTTP — for example a
corporate internal MCP proxy — point the config at its URL:

```toml
[mcp.servers.internal]
transport = "sse"
url = "https://mcp.internal.example.com/v1"
headers = { Authorization = "Bearer ${INTERNAL_MCP_TOKEN}" }
auto_connect = true
tier_override = "T2"
```

The manager dials the URL with the headers applied and holds the
connection open. Tool listings and invocations travel over the same
endpoint; the rmcp library handles both one-shot and long-polling
session modes transparently.

## Verification

1. `ryvos doctor` reports every configured MCP server and whether the
   initial handshake succeeded.
2. `/mcp list` shows bridged tools. Each one has the
   `mcp__{server}__{tool}` prefix.
3. `ryvos run "list files in /tmp"` uses
   `mcp__filesystem__list_directory`. Confirm with
   `ryvos audit query --tool mcp__filesystem__list_directory`.
4. `ryvos audit stats` groups tool counts so you can see the bridged
   calls alongside native ones.
5. If the server supports `ToolsChanged`, add or remove a tool on the
   server side and watch `/mcp list` update without a daemon restart.

For the internal wire-level story — transport negotiation, reconnect
behavior, and the `McpBridgedTool` shape — read
[../crates/ryvos-mcp.md](../crates/ryvos-mcp.md) and
[../internals/mcp-bridge.md](../internals/mcp-bridge.md). For the
rationale behind MCP as Ryvos's integration layer rather than a
bespoke plugin protocol, read
[../adr/008-mcp-integration-layer.md](../adr/008-mcp-integration-layer.md).
The nine tools Ryvos exposes when acting as an MCP server — so a Ryvos
daemon can be the server on the other end of someone else's client —
are documented in [../api/mcp-server.md](../api/mcp-server.md).
