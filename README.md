<div align="center">

<img src="docs/logo.png" alt="Ryvos" width="120">

# Ryvos

### Your autonomous AI assistant — secure, fast, and always on.

[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Release](https://img.shields.io/github/v/release/Ryvos/ryvos?color=F07030)](https://github.com/Ryvos/ryvos/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/Ryvos/ryvos/ci.yml?label=CI)](https://github.com/Ryvos/ryvos/actions)

**Goal-Driven Agents · Multi-Provider LLM · DAG Workflows · MCP-Native · Sandboxed by Default · Single Binary**

[Quick Start](#quick-start) · [Why Ryvos](#why-ryvos) · [Features](#features) · [Architecture](#architecture) · [Security](#security-model) · [Roadmap](#roadmap)

</div>

---

## What is Ryvos?

Ryvos is an open-source, autonomous personal AI assistant you run on your own hardware. It connects to 14 LLM providers (Anthropic, OpenAI, Gemini, Azure, Ollama, Groq, OpenRouter, Together, Fireworks, Cerebras, xAI, Mistral, Perplexity, DeepSeek), executes tasks through 62 sandboxed tools, and reaches you on the channels you already use — Telegram, Discord, Slack, Webhooks — plus a built-in Web UI and terminal interface.

Written in Rust. Ships as a single binary. Uses 15–30 MB of RAM.

```bash
cargo install --path .
ryvos init          # Pick your LLM provider, paste an API key
ryvos               # Start talking to your assistant
```

---

## Why Ryvos?

Autonomous AI assistants are exploding — but the current generation is built on TypeScript and Python runtimes that were never designed for always-on, autonomous operation:

- **No security model** — community skills run arbitrary code with full system access.
- **No goal awareness** — agents run until max_turns, not until the task is done.
- **High resource usage** — 200-500MB RAM idle, garbage collection pauses, slow cold starts.
- **Fragile deployment** — requires Node.js ≥22, npm ecosystem, container orchestration.

Ryvos is built from scratch in Rust with a different set of priorities:

| | Typical AI assistants | Ryvos |
|---|---|---|
| **Language** | TypeScript / Python | Rust |
| **Memory** | 200–500 MB | 15–30 MB |
| **Execution model** | Run until max_turns | Goal-driven with Judge verdict |
| **Tool security** | None (arbitrary code) | 5-tier classification + Docker sandboxing |
| **Dangerous command detection** | None | 9 built-in patterns (rm -rf, DROP TABLE, curl\|bash, etc.) |
| **Deployment** | npm/pip + runtime + Docker | Single static binary |
| **MCP support** | Plugin/community | Native (stdio + SSE/Streamable HTTP) |
| **Parallel tool execution** | Rare | Built-in |
| **Multi-agent workflows** | Separate orchestration layer | Built-in DAG engine + orchestrator |
| **Channel adapters** | Separate projects | Built-in (Telegram, Discord, Slack) |
| **HTTP Gateway** | Separate project | Built-in with Web UI + RBAC |

---

## Features

### Goal-Driven Execution
- **Goals with weighted success criteria** — define what "done" means with `OutputContains`, `OutputEquals`, `LlmJudge`, or `Custom` criteria, each with individual weights
- **Constraints** — hard and soft limits on time, cost, safety, scope, and quality
- **Two-level Judge** — Level 0 (deterministic fast-check) + LLM ConversationJudge that evaluates full conversation context
- **Verdicts** — `Accept(confidence)`, `Retry(reason, hint)`, `Escalate(reason)`, or `Continue` — the agent keeps going until the goal is met or turns run out

### Autonomous Agent
- **ReAct agent loop** with tool use, reflexion, and streaming responses
- **Parallel tool execution** — multiple tools run concurrently when independent
- **Multi-provider LLM** — 14 providers: Anthropic, OpenAI, Gemini, Azure, Cohere, Ollama, Groq, OpenRouter, Together, Fireworks, Cerebras, xAI, Mistral, Perplexity, DeepSeek
- **Session persistence** — SQLite-backed conversation history and memory across restarts
- **Sub-agent spawning** — delegate tasks to child agents with stricter security
- **Lifecycle hooks** — trigger shell commands on start, message, tool call, response, turn complete, tool error, session start/end
- **Checkpoint / resume** — agent state persisted to SQLite after each turn; crashed runs resume automatically
- **Decision tracking** — every tool call choice recorded with alternatives, confidence scores, and outcome (tokens, latency, success)
- **Structured output validation** — heuristic repair (strip code fences, balance JSON braces, enforce max length) + optional LLM repair against expected schema

### DAG Workflow Engine
- **Graph execution** — define multi-step workflows as directed acyclic graphs of agent nodes
- **Node types** — each node is an independent agent run with its own system prompt, tools, goal, and max turns
- **Edge conditions** — `Always`, `OnSuccess`, `OnFailure`, `Conditional(expression)`, `LlmDecide(prompt)`
- **Handoff context** — shared key-value store for passing data between nodes with JSON extraction
- **Multi-agent orchestrator** — capability-based routing with `Parallel`, `Relay`, and `Broadcast` dispatch modes

### Multi-Channel Inbox
- **Telegram, Discord, Slack** — talk to your assistant on the platforms you already use
- **Per-channel DM policies** — allowlist, open, or disabled access control per channel
- **HTTP/WebSocket Gateway** — Axum-based server with embedded Web UI for browser access
- **Terminal UI** — full ratatui-based TUI with adaptive banner and streaming output
- **Interactive REPL** — quick command-line usage
- **Daemon mode** — always-on background service with `--gateway` flag
- **Cron scheduler** — recurring tasks with cron expressions, persistent across restarts
- **Heartbeat** — periodic proactive agent checks with smart suppression and alert routing

### Security (Built-in, Not Bolted-on)
- **5-tier tool classification** (T0 safe → T4 critical) with automatic tier escalation
- **Dangerous pattern detection** — regex-based detection of destructive commands before execution
- **Docker sandboxing** — optional container isolation with memory limits, network isolation, and timeouts
- **Human-in-the-loop approval** — configurable approval flows for high-risk tool calls
- **Sandboxed skills** — user extensions run in Lua/Rhai, not arbitrary system code
- **Sub-agent restrictions** — spawned agents default to stricter security policies
- **Guardian watchdog** — detects stalls, doom loops (same tool called repeatedly), and budget overruns; injects corrective hints

### Observability
- **JSONL runtime logging** — three-level logging (L1 run summary, L2 per-turn detail, L3 tool execution) — crash-resilient append-only format
- **Decision journal** — SQLite-backed log of every tool call decision with alternatives and outcomes
- **Scoped EventBus** — subscribe to filtered events by type, session, or node for monitoring and integrations
- **Goal evaluation events** — stream `GoalEvaluated` and `JudgeVerdict` events to TUI, gateway, or custom subscribers
- **Token usage tracking** — per-turn and per-run input/output token counts

### Tools & Extensibility
- **62 built-in tools** — shell, file I/O, git, code analysis, network/HTTP, system, data transform, scheduling, database, sessions, memory, notifications
- **MCP-native** — connect to Model Context Protocol servers (stdio + SSE/Streamable HTTP transports)
- **Drop-in skills** — Lua/Rhai scripts in `~/.ryvos/skills/` with manifest-declared schemas and sandbox requirements
- **Tool registry** — built-in tools + custom tools via MCP or skills
- **Role-based API keys** — Viewer, Operator, Admin roles for gateway access
- **Phase-aware context compaction** — messages tagged by phase (planning, execution); protected messages survive compaction; phase-grouped summarization
- **Three-layer prompt composition** — Identity (SOUL.md) → Narrative (summaries, agents) → Focus (current goal + constraints)

---

## Quick Start

### Install

```bash
# Build from source (requires Rust 1.75+)
cargo install --path .
```

<details>
<summary>Other install methods</summary>

```bash
# One-line install (Linux / macOS)
curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh

# Pin a specific version
RYVOS_VERSION=v0.1.0 curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh

# Custom install directory
RYVOS_INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh
```
</details>

<div align="center">

<img src="docs/demo.gif" alt="Ryvos demo — security gate blocks rm -rf" width="720">

*Security gate auto-blocks dangerous commands (T4) — no confirmation needed.*

</div>

### Get Started

```bash
# Interactive setup — pick a provider, configure security, install service
ryvos init

# Non-interactive setup with defaults
ryvos init -y --provider ollama --model-id qwen2.5:7b

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

# Check system health
ryvos doctor
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
| `ryvos tui` | Terminal UI with streaming output |
| `ryvos serve` | Web UI + HTTP/WebSocket gateway |
| `ryvos daemon` | Always-on assistant (Telegram, Discord, Slack) |
| `ryvos daemon --gateway` | Always-on + Web UI in one process |
| `ryvos init` | Interactive setup wizard |
| `ryvos init -y` | Non-interactive setup with defaults |
| `ryvos config` | Print resolved configuration |
| `ryvos doctor` | System health checks (API, workspace, DB, channels, cron, MCP, security) |
| `ryvos health` | Tool health statistics |
| `ryvos mcp list` | List configured MCP servers |
| `ryvos mcp add <name>` | Add an MCP server |
| `ryvos completions <shell>` | Generate shell completions (bash, zsh, fish) |

---

## Architecture

Ryvos is a Cargo workspace with 10 crates. Together they form a complete autonomous assistant — goal-driven LLM reasoning, DAG workflow orchestration, tool execution, security enforcement, persistent memory, multi-channel inbox, and observability — all in one binary.

```
┌─────────────────────────────────────────────────────┐
│                     ryvos (CLI)                     │
├──────────┬──────────┬───────────┬───────────────────┤
│ ryvos-tui│  ryvos-  │  ryvos-   │  ryvos-channels   │
│  (TUI)   │ gateway  │  agent    │ (Telegram/Discord/ │
│          │(HTTP/WS) │           │      Slack)        │
├──────────┴──────────┤           ├───────────────────┤
│    ryvos-skills     │           │    ryvos-mcp      │
│  (Lua/Rhai loader)  │           │  (MCP client)     │
├─────────────────────┼───────────┼───────────────────┤
│    ryvos-tools      │ ryvos-llm │  ryvos-memory     │
│  (tool registry)    │(streaming │  (SQLite store)   │
│                     │  client)  │                   │
├─────────────────────┴───────────┴───────────────────┤
│                   ryvos-core                        │
│    (config, error types, event bus, security,       │
│     goal system, traits, types)                     │
└─────────────────────────────────────────────────────┘
```

| Crate | Purpose |
|-------|---------|
| `ryvos-core` | Config, error types, scoped event bus, security policy, goal system, traits |
| `ryvos-llm` | LLM client abstraction with streaming support (Anthropic, OpenAI, any compatible) |
| `ryvos-tools` | Tool registry, 62 built-in tools across 11 categories |
| `ryvos-agent` | ReAct loop, SecurityGate, ApprovalBroker, Guardian watchdog, Judge, GoalEvaluator, OutputValidator, CheckpointStore, RunLogger, CronScheduler, GraphExecutor, MultiAgentOrchestrator |
| `ryvos-memory` | SQLite-backed session and history storage |
| `ryvos-gateway` | Axum HTTP/WS server, Web UI, role-based auth middleware |
| `ryvos-channels` | Telegram, Discord, Slack adapters with DM policy enforcement |
| `ryvos-mcp` | MCP client (stdio + SSE transports) with sampling control |
| `ryvos-skills` | Drop-in skill loader (Lua/Rhai) with manifest validation |
| `ryvos-tui` | Terminal UI built on ratatui with adaptive banner |

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

```toml
[security]
auto_approve_up_to = "t1"    # t0-t1 run automatically
deny_above = "t3"            # t4 blocked outright (default)
approval_timeout_secs = 60   # Unapproved requests timeout
```

The gate is **fail-closed** — if a tool call's arguments can't be parsed, it escalates to T4 and denies execution rather than silently allowing it.

### Sub-Agent Restrictions

Agents that spawn child agents automatically apply a **stricter policy** — preventing privilege escalation through agent chains.

---

## Configuration

Configuration lives in `~/.ryvos/config.toml` (created by `ryvos init`). You can also place a `ryvos.toml` in the current directory. Environment variables expand with `${VAR}` syntax.

```toml
[agent]
max_turns = 25
max_duration_secs = 600
parallel_tools = true
enable_summarization = true

# Optional: goal-driven self-evaluation after each run
enable_self_eval = true

# Optional: checkpoint / resume crashed runs
[agent.checkpoint]
enabled = true

# Optional: JSONL runtime logging
[agent.log]
enabled = true
log_dir = "~/.ryvos/logs"

# Optional: Guardian watchdog
[agent.guardian]
stall_timeout_secs = 60
doom_loop_threshold = 5
budget_tokens = 100000

[model]
provider = "anthropic"
model_id = "claude-sonnet-4-20250514"
api_key = "${ANTHROPIC_API_KEY}"

# Ollama example:
# provider = "ollama"
# model_id = "qwen2.5:7b"
# base_url = "http://localhost:11434/v1/chat/completions"

[security]
auto_approve_up_to = "t1"
deny_above = "t4"
approval_timeout_secs = 60

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

---

## Roadmap

### Completed
- [x] Goal-driven execution with weighted success criteria
- [x] Two-level Judge system (deterministic + LLM)
- [x] Decision tracking and failure journal
- [x] Structured output validation and repair
- [x] JSONL runtime logging (L1/L2/L3)
- [x] Phase-aware context compaction
- [x] Three-layer prompt composition
- [x] Checkpoint / resume
- [x] DAG workflow engine (graph execution)
- [x] Multi-agent orchestrator with capability-based routing
- [x] Scoped EventBus with filtered subscriptions
- [x] Cron scheduler with persistent resume
- [x] Guardian watchdog (stall, doom loop, budget detection)
- [x] Multi-channel inbox (Telegram, Discord, Slack)
- [x] HTTP/WebSocket gateway with Web UI
- [x] Heartbeat system with smart suppression and alert routing

### Upcoming
- [ ] Pre-built binaries (Windows, macOS, Linux) via GitHub Releases
- [ ] `cargo install ryvos` from crates.io
- [ ] WhatsApp, Signal, iMessage, and Google Chat channel adapters
- [ ] Voice mode — wake word detection + speech-to-text + TTS
- [ ] Mobile companion apps (iOS, Android) via WebSocket
- [ ] Browser control — navigate, click, extract, screenshot
- [ ] Live Canvas — real-time document/artifact editing in Web UI
- [ ] Ryvos Cloud — hosted assistant with managed sessions
- [ ] SOC 2 compliance documentation
- [ ] Signed & verified skill marketplace
- [ ] MCP sampling support (server-initiated LLM calls)

---

## Contributing

We welcome contributions. See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## License

[MIT](LICENSE)
