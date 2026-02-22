<div align="center">

# Ryvos

### The secure, high-performance AI agent runtime — written in Rust.

[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

**Multi-provider LLM · Parallel tool execution · MCP-native · Sandboxed by default**

[Quick Start](#quick-start) · [Why Ryvos](#why-ryvos) · [Features](#features) · [Architecture](#architecture) · [Security](#security-model) · [Roadmap](#roadmap)

</div>

---

## What is Ryvos?

Ryvos is an open-source AI agent runtime that lets developers build, deploy, and operate autonomous AI agents. It connects to any LLM provider (Anthropic, OpenAI, Ollama, or any OpenAI-compatible API), executes tools in a sandboxed environment, and deploys anywhere as a single binary — no Node.js, no Python, no Docker required.

```bash
cargo install --path .
ryvos init          # Pick your LLM provider, paste an API key
ryvos               # Start talking to your agent
```

Three commands. Under 30MB of RAM. Running.

---

## Why Ryvos?

The AI agent ecosystem today is built on Python and TypeScript runtimes that were never designed for autonomous tool execution at scale. They ship with:

- **No security model** — community plugins run arbitrary code with full system access
- **High resource usage** — 200-500MB RAM idle, GC pauses, slow cold starts
- **No deployment story** — require container orchestration just to run a single agent

Ryvos takes a different approach:

| | Typical agent runtimes | Ryvos |
|---|---|---|
| **Language** | Python / TypeScript | Rust |
| **Memory** | 200–500 MB | 15–30 MB |
| **Tool security** | None (arbitrary code) | 5-tier classification + sandboxing |
| **Dangerous command detection** | None | 9 built-in patterns (rm -rf, DROP TABLE, curl\|bash, etc.) |
| **Deployment** | npm/pip + runtime + Docker | Single static binary |
| **MCP support** | Plugin/community | Native (stdio + SSE/Streamable HTTP) |
| **Parallel tool execution** | Rare | Built-in |
| **Channel adapters** | Separate projects | Built-in (Telegram, Discord, Slack) |
| **HTTP Gateway** | Separate project | Built-in with Web UI + RBAC |

---

## Features

### Agent Runtime
- **ReAct agent loop** with tool use, reflexion, and streaming responses
- **Parallel tool execution** — multiple tools run concurrently when independent
- **Multi-provider LLM** — Anthropic, OpenAI, Ollama, or any OpenAI-compatible endpoint
- **Session persistence** — SQLite-backed conversation history and memory
- **Sub-agent spawning** — agents can spawn child agents with stricter security policies

### Security (Built-in, Not Bolted-on)
- **5-tier tool classification** (T0 safe → T4 critical) with automatic tier escalation
- **Dangerous pattern detection** — regex-based detection of destructive commands before execution
- **Docker sandboxing** — optional container isolation with memory limits, network isolation, and timeouts
- **Human-in-the-loop approval** — configurable approval flows for high-risk tool calls
- **Sandboxed skills** — user extensions run in Lua/Rhai, not arbitrary system code
- **Sub-agent restrictions** — spawned agents default to stricter security policies

### Deployment
- **Gateway** — Axum HTTP/WebSocket server with embedded Web UI
- **Role-based API keys** — Viewer, Operator, Admin roles for gateway access
- **Channel adapters** — Telegram, Discord, Slack with per-channel DM policies (allowlist/open/disabled)
- **Daemon mode** — run as a background service with `--gateway` flag
- **TUI** — full terminal UI built on ratatui
- **Interactive REPL** — for quick command-line usage
- **Lifecycle hooks** — shell commands on start, message, tool call, and response events

### Extensibility
- **MCP-native** — connect to Model Context Protocol servers (stdio + SSE/Streamable HTTP transports)
- **Drop-in skills** — Lua/Rhai scripts in `~/.ryvos/skills/` with manifest-declared schemas and sandbox requirements
- **Tool registry** — built-in tools (shell, file I/O, web search) + custom tools via MCP or skills
- **Event bus** — subscribe to agent events for monitoring, logging, or custom integrations

---

## Quick Start

```bash
# Install from source
cargo install --path .

# Interactive setup — pick a provider, paste an API key
ryvos init

# Start the REPL
ryvos

# Or run a single prompt
ryvos run "Summarize the files in this directory"

# Launch the terminal UI
ryvos tui

# Start the HTTP/WebSocket gateway with Web UI
ryvos serve

# Run as a daemon with Telegram + Discord + Slack
ryvos daemon --gateway
```

### Commands

| Command | Description |
|---------|-------------|
| `ryvos` | Interactive REPL (default) |
| `ryvos run <prompt>` | Single prompt, then exit |
| `ryvos tui` | Terminal UI |
| `ryvos serve` | HTTP/WebSocket gateway with Web UI |
| `ryvos daemon` | Channel adapters (Telegram, Discord, Slack) |
| `ryvos daemon --gateway` | Channels + gateway in one process |
| `ryvos init` | Interactive setup wizard |
| `ryvos init -y` | Non-interactive setup with defaults |
| `ryvos config` | Print resolved configuration |
| `ryvos doctor` | Run system health checks |
| `ryvos health` | Tool health statistics |
| `ryvos mcp list` | List configured MCP servers |
| `ryvos mcp add <name>` | Add an MCP server |
| `ryvos completions <shell>` | Generate shell completions |

---

## Architecture

Ryvos is a Cargo workspace with 10 crates, each with a single responsibility:

```
┌─────────────────────────────────────────────────────┐
│                     ryvos (CLI)                     │
├──────────┬──────────┬───────────┬───────────────────┤
│ ryvos-tui│  ryvos-  │  ryvos-   │  ryvos-channels   │
│  (TUI)   │ gateway  │  agent    │ (Telegram/Discord/ │
│          │(HTTP/WS) │(ReAct loop│      Slack)        │
├──────────┴──────────┤ + security├───────────────────┤
│    ryvos-skills     │   gate)   │    ryvos-mcp      │
│  (Lua/Rhai loader)  │          │  (MCP client)      │
├─────────────────────┼──────────┼───────────────────┤
│    ryvos-tools      │ ryvos-llm│  ryvos-memory     │
│  (tool registry)    │(streaming│  (SQLite store)    │
│                     │  client) │                    │
├─────────────────────┴──────────┴───────────────────┤
│                   ryvos-core                        │
│    (config, error types, event bus, security,       │
│     traits, types)                                  │
└─────────────────────────────────────────────────────┘
```

| Crate | Purpose |
|-------|---------|
| `ryvos-core` | Config, error types, event bus, security policy, traits |
| `ryvos-llm` | LLM client abstraction with streaming support |
| `ryvos-tools` | Tool registry, built-in tools (shell, file I/O, web search) |
| `ryvos-agent` | ReAct agent loop, SecurityGate, ApprovalBroker, session management |
| `ryvos-memory` | SQLite-backed session and history storage |
| `ryvos-gateway` | Axum HTTP/WS server, Web UI, role-based auth middleware |
| `ryvos-channels` | Telegram, Discord, Slack adapters with DM policy enforcement |
| `ryvos-mcp` | MCP client (stdio + SSE transports) with sampling control |
| `ryvos-skills` | Drop-in skill loader (Lua/Rhai) with manifest validation |
| `ryvos-tui` | Terminal UI built on ratatui |

---

## Security Model

Security is enforced at the **SecurityGate middleware** — every tool call passes through it before execution.

### Tool Tier System

Every tool declares a security tier. The SecurityGate compares the effective tier against your policy to decide: **Allow**, **Deny**, or **NeedsApproval**.

| Tier | Risk Level | Example | Default Policy |
|------|-----------|---------|----------------|
| T0 | Safe | Read file, list directory | Auto-approve |
| T1 | Low | Web search, read URL | Auto-approve |
| T2 | Medium | Write file, edit file | Needs approval |
| T3 | High | Shell command, spawn agent | Needs approval |
| T4 | Critical | rm -rf, DROP TABLE, curl\|bash | Deny |

### Dangerous Pattern Detection

The SecurityGate inspects tool inputs with regex patterns and **automatically escalates** to T4:

```
rm -rf    git --force    DROP TABLE    chmod 777
mkfs      dd             >/dev/*       curl|bash    wget|sh
```

### Docker Sandboxing

Shell commands can optionally run inside an isolated Docker container:

```toml
[agent.sandbox]
enabled = true
memory_mb = 512
timeout_secs = 120
network = "none"        # No network access
mount_workspace = true  # Only mount the agent workspace
```

### Human-in-the-Loop Approval

High-risk tool calls pause and wait for explicit approval — via REPL prompt, TUI dialog, Discord button, Telegram message, or gateway WebSocket:

```
[security]
auto_approve_up_to = "T1"    # T0-T1 run automatically
deny_above = "T4"            # T4 blocked outright
approval_timeout_secs = 60   # Unapproved requests timeout
```

### Sub-Agent Restrictions

Agents that spawn child agents automatically apply a **stricter policy** — preventing privilege escalation through agent chains.

---

## Configuration

Configuration lives in `ryvos.toml` or `~/.ryvos/config.toml`. Environment variables expand with `${VAR}` syntax.

```toml
[agent]
max_turns = 25
max_duration_secs = 600

[model]
provider = "anthropic"
model_id = "claude-sonnet-4-20250514"
api_key = "${ANTHROPIC_API_KEY}"

[gateway]
bind = "127.0.0.1:18789"

[[gateway.api_keys]]
name = "web-ui"
key = "rk_..."
role = "operator"    # viewer | operator | admin

[channels.telegram]
bot_token = "${TELEGRAM_BOT_TOKEN}"
dm_policy = "allowlist"
allowed_users = [123456789]

[mcp.servers.filesystem]
transport = { type = "stdio", command = "npx", args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"] }
```

See [`ryvos.toml.example`](ryvos.toml.example) for the full reference.

---

## Roadmap

- [ ] Pre-built binaries (Windows, macOS, Linux) via GitHub Releases
- [ ] `cargo install ryvos` from crates.io
- [ ] Ryvos Cloud — hosted gateway with managed sessions
- [ ] SOC 2 compliance documentation
- [ ] Plugin marketplace with signed, verified skills
- [ ] Multi-agent collaboration protocols
- [ ] Observability dashboard (token usage, tool latency, security events)
- [ ] WhatsApp and Signal channel adapters

---

## Contributing

We welcome contributions. See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## License

[MIT](LICENSE)
