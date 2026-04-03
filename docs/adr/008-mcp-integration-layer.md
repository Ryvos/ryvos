# ADR-008: MCP as the Integration Layer

## Status

Accepted

## Context

Ryvos needs to integrate in two directions. First, external AI assistants like
Claude Code need to access Ryvos capabilities (memory, audit, daily logs).
Second, Ryvos needs to access external tool servers (filesystem operations,
GitHub, database queries, and whatever else the ecosystem produces).

We could build custom integrations for each direction. A bespoke Claude Code
plugin, a custom GitHub integration, hand-rolled API adapters for every service.
That works, but it scales linearly with the number of integrations and creates
a maintenance burden.

The Model Context Protocol (MCP) is an emerging standard for exactly this
problem. It defines a JSON-RPC protocol for tool discovery, invocation, and
resource access. Claude Code already supports MCP servers natively. The
ecosystem has thousands of MCP servers covering common services.

The protocol supports multiple transport layers: stdio (for local subprocess
servers), Streamable HTTP, and Server-Sent Events (SSE). The Rust ecosystem
has the rmcp crate for building both servers and clients.

## Decision

Ryvos implements both sides of the MCP protocol:

**As an MCP server** (so Claude Code can use Ryvos tools):

We expose 9 tools through the MCP server interface:
- `viking_read`, `viking_write`, `viking_search`, `viking_list` for memory
- `audit_query`, `audit_stats` for audit log access
- `memory_get`, `memory_write` for quick key-value memory
- `daily_log_write` for structured daily logging

The MCP server runs over stdio transport. Claude Code connects to it by adding
Ryvos to its MCP configuration (`.mcp.json` or the global settings). From
Claude Code's perspective, Ryvos memory and audit tools appear as native tools
alongside its built-in file editing and bash tools.

**As an MCP client** (so Ryvos can use external tool servers):

Ryvos can connect to external MCP servers and make their tools available to the
agent. The user configures external servers in the Ryvos config file, specifying
the transport (stdio command, HTTP URL, or SSE endpoint). On startup, Ryvos
connects to each configured server, discovers its tools, and registers them in
the tool catalog.

When the LLM wants to use an external tool, Ryvos routes the call to the
appropriate MCP client connection, executes it, and returns the result.

## Consequences

**What went well:**

- Claude Code integration is seamless. Users add Ryvos as an MCP server once,
  and from then on, Claude Code can read and write Viking memory, query the
  audit log, and use all Ryvos tools naturally. No custom plugin needed.
- The external MCP client gives Ryvos access to a huge ecosystem. Filesystem
  servers, GitHub servers, Notion servers, database servers, and more. Each one
  is a few lines of configuration, not a custom integration.
- MCP's tool discovery protocol means Ryvos automatically knows what tools are
  available from each connected server. No hardcoded tool definitions needed.
- The JSON-RPC protocol is simple and well-defined. Implementing both client
  and server was straightforward with the rmcp crate.

**What is harder:**

- The MCP protocol is still evolving. The spec has gone through several
  revisions, and transport details (especially around Streamable HTTP and
  authentication) are not fully stabilized. We may need to update our
  implementation as the standard matures.
- stdio transport requires spawning child processes. Each external MCP server
  is a separate process that Ryvos manages. This works fine for a few servers,
  but could become resource-intensive if someone configures dozens.
- Authentication for MCP servers varies widely. Some use OAuth 2.1, some use
  API keys, some have no auth at all. Ryvos needs to handle each case, which
  adds complexity to the configuration surface.
- Error handling across the MCP boundary is lossy. If an external server
  returns a cryptic error, Ryvos can only pass it through. There is limited
  ability to provide helpful diagnostics for third-party tool failures.
- Testing MCP integrations requires running actual server processes. Our test
  suite includes mock MCP servers, but integration testing with real external
  servers is manual.

MCP is the right bet for the integration layer. It is the closest thing to a
universal standard for AI tool interop, and its adoption is accelerating. The
protocol instability is a manageable risk given the alternative of building
everything custom.
