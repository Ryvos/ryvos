<div align="center">

# Ryvos

### Your autonomous AI assistant — secure, fast, and always on.

[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

**Multi-provider LLM · Multi-channel inbox · MCP-native · Sandboxed by default · Single binary**

[Quick Start](#quick-start) · [Why Ryvos](#why-ryvos) · [Features](#features) · [Architecture](#architecture) · [Security](#security-model) · [Roadmap](#roadmap)

</div>

---

## What is Ryvos?

Ryvos is an open-source, autonomous personal AI assistant you run on your own hardware. It connects to any LLM (Anthropic, OpenAI, Ollama, or any OpenAI-compatible API), executes tasks through sandboxed tools, and reaches you on the channels you already use — Telegram, Discord, Slack — plus a built-in Web UI and terminal interface. Written in Rust. Ships as a single binary. Uses 15–30 MB of RAM.

```bash
cargo install --path .
ryvos init          # Pick your LLM provider, paste an API key
ryvos               # Start talking to your assistant
```

Three commands. Under 30MB of RAM. Your assistant is running.

---

## Why Ryvos?

Autonomous AI assistants are exploding — OpenClaw hit 140K GitHub stars in six weeks. But the current generation is built on TypeScript and Python runtimes that were never designed for always-on, autonomous operation:

- **No security model** — community skills run arbitrary code with full system access. Cisco's security team [found OpenClaw skills performing data exfiltration](https://blogs.cisco.com/ai/personal-ai-agents-like-openclaw-are-a-security-nightmare) without user awareness.
- **High resource usage** — 200-500MB RAM idle, garbage collection pauses, slow cold starts. Not what you want from an always-on assistant.
- **Fragile deployment** — requires Node.js ≥22, npm ecosystem, container orchestration. Breaks on updates.

Ryvos is built from scratch in Rust with a different set of priorities:

| | Typical AI assistants | Ryvos |
|---|---|---|
| **Language** | TypeScript / Python | Rust |
| **Memory** | 200–500 MB | 15–30 MB |
| **Tool security** | None (arbitrary code) | 5-tier classification + Docker sandboxing |
| **Dangerous command detection** | None | 9 built-in patterns (rm -rf, DROP TABLE, curl\|bash, etc.) |
| **Deployment** | npm/pip + runtime + Docker | Single static binary |
| **MCP support** | Plugin/community | Native (stdio + SSE/Streamable HTTP) |
| **Parallel tool execution** | Rare | Built-in |
| **Channel adapters** | Separate projects | Built-in (Telegram, Discord, Slack) |
| **HTTP Gateway** | Separate project | Built-in with Web UI + RBAC |

---

## Features

### Autonomous Assistant
- **ReAct agent loop** with tool use, reflexion, and streaming responses
- **Parallel tool execution** — multiple tools run concurrently when independent
- **Multi-provider LLM** — Anthropic, OpenAI, Ollama, or any OpenAI-compatible endpoint
- **Session persistence** — SQLite-backed conversation history and memory across restarts
- **Sub-agent spawning** — your assistant can delegate tasks to child agents with stricter security
- **Lifecycle hooks** — trigger shell commands on start, message, tool call, and response events

### Multi-Channel Inbox
- **Telegram, Discord, Slack** — talk to your assistant on the platforms you already use
- **Per-channel DM policies** — allowlist, open, or disabled access control per channel
- **HTTP/WebSocket Gateway** — Axum-based server with embedded Web UI for browser access
- **Terminal UI** — full ratatui-based TUI for power users
- **Interactive REPL** — quick command-line usage
- **Daemon mode** — always-on background service with `--gateway` flag

### Security (Built-in, Not Bolted-on)
- **5-tier tool classification** (T0 safe → T4 critical) with automatic tier escalation
- **Dangerous pattern detection** — regex-based detection of destructive commands before execution
- **Docker sandboxing** — optional container isolation with memory limits, network isolation, and timeouts
- **Human-in-the-loop approval** — configurable approval flows for high-risk tool calls
- **Sandboxed skills** — user extensions run in Lua/Rhai, not arbitrary system code
- **Sub-agent restrictions** — spawned agents default to stricter security policies

### Tools & Extensibility
- **Built-in tools** — shell, file I/O, web search, and more out of the box
- **MCP-native** — connect to Model Context Protocol servers (stdio + SSE/Streamable HTTP transports)
- **Drop-in skills** — Lua/Rhai scripts in `~/.ryvos/skills/` with manifest-declared schemas and sandbox requirements
- **Tool registry** — built-in tools + custom tools via MCP or skills
- **Role-based API keys** — Viewer, Operator, Admin roles for gateway access
- **Event bus** — subscribe to assistant events for monitoring, logging, or custom integrations

---

## Quick Start

### Install

```bash
# One-line install (Linux / macOS) — no Rust required
curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh
```

Or download a binary directly from [GitHub Releases](https://github.com/Ryvos/ryvos/releases).

<details>
<summary>Other install methods</summary>

```bash
# Pin a specific version
RYVOS_VERSION=v0.1.0 curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh

# Custom install directory
RYVOS_INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh

# Build from source (requires Rust 1.75+)
cargo install --path .
```
</details>

### Get Started

```bash
# Interactive setup — pick a provider, paste an API key
ryvos init

# Start talking to your assistant
ryvos

# Or ask a quick question
ryvos run "What meetings do I have tomorrow?"

# Launch the terminal UI
ryvos tui

# Start the Web UI + HTTP/WebSocket gateway
ryvos serve

# Always-on assistant: Telegram + Discord + Slack + gateway
ryvos daemon --gateway
```

### Uninstall

```bash
rm ~/.local/bin/ryvos
rm -rf ~/.ryvos   # optional: remove config and data
```

### Commands

| Command | Description |
|---------|-------------|
| `ryvos` | Interactive conversation (default) |
| `ryvos run <prompt>` | Ask a question, get an answer, exit |
| `ryvos tui` | Terminal UI |
| `ryvos serve` | Web UI + HTTP/WebSocket gateway |
| `ryvos daemon` | Always-on assistant (Telegram, Discord, Slack) |
| `ryvos daemon --gateway` | Always-on + Web UI in one process |
| `ryvos init` | Interactive setup wizard |
| `ryvos init -y` | Non-interactive setup with defaults |
| `ryvos config` | Print resolved configuration |
| `ryvos doctor` | System health checks |
| `ryvos health` | Tool health statistics |
| `ryvos mcp list` | List configured MCP servers |
| `ryvos mcp add <name>` | Add an MCP server |
| `ryvos completions <shell>` | Generate shell completions |

---

## Architecture

Ryvos is a Cargo workspace with 10 crates. Together they form a complete autonomous assistant — LLM reasoning, tool execution, security enforcement, persistent memory, multi-channel inbox, and a web dashboard — all in one binary.

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
| `ryvos-agent` | Autonomous ReAct loop, SecurityGate, ApprovalBroker, session management |
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
- [ ] WhatsApp, Signal, iMessage, and Google Chat channel adapters
- [ ] Voice mode — wake word detection + speech-to-text + TTS
- [ ] Mobile companion apps (iOS, Android) via WebSocket
- [ ] Browser control — navigate, click, extract, screenshot
- [ ] Cron scheduler — recurring tasks and automated workflows
- [ ] Live Canvas — real-time document/artifact editing in Web UI
- [ ] Ryvos Cloud — hosted assistant with managed sessions
- [ ] SOC 2 compliance documentation
- [ ] Signed & verified skill marketplace
- [ ] Multi-agent collaboration protocols
- [ ] Observability dashboard (token usage, tool latency, security events)

---

## Contributing

We welcome contributions. See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## License

[MIT](LICENSE)
