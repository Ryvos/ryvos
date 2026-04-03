<div align="center">

<img src="docs/logo.png" alt="Ryvos — Open-Source AI Agent Runtime" width="120">

# Ryvos

### Open-source AI agent runtime built in Rust. Self-hosted. 15–30 MB RAM. 18+ LLM providers.

[![GitHub Stars](https://img.shields.io/github/stars/Ryvos/ryvos?style=flat&color=yellow)](https://github.com/Ryvos/ryvos/stargazers)
[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 1.75+](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Release](https://img.shields.io/github/v/release/Ryvos/ryvos?color=F07030)](https://github.com/Ryvos/ryvos/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/Ryvos/ryvos/ci.yml?label=CI)](https://github.com/Ryvos/ryvos/actions)
[![Platform: Linux macOS](https://img.shields.io/badge/platform-linux%20%7C%20macos-lightgrey.svg)](#quick-start)

**Goal-Driven Agents · 18+ LLM Providers · 86+ Tools · DAG Workflows · MCP-Native · Constitutional AI Safety · Single Binary**

[Quick Start](#quick-start) · [Why Ryvos](#why-ryvos) · [Features](#features) · [Architecture](#architecture) · [Security](#security) · [Roadmap](#roadmap)

[Website](https://ryvos.dev) · [Cloud](https://cloud.ryvos.dev) · [Docs](https://ryvos.dev/docs)

</div>

---

```bash
# One-line install (Linux / macOS)
curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh
ryvos init    # pick your LLM provider, paste an API key
ryvos         # start your AI coding assistant
```

---

## Why Ryvos?

Most AI coding assistants are built on TypeScript or Python runtimes that were never designed for autonomous, always-on operation. They're heavy, insecure, and fragile to deploy.

Ryvos is a complete reimagination — built in Rust from scratch as a true **autonomous AI agent runtime**:

| | Typical AI assistants | **Ryvos** |
|---|---|---|
| **Language** | TypeScript / Python | **Rust** |
| **Memory (idle)** | 200–500 MB | **15–30 MB** |
| **Execution model** | Run until max_turns | **Goal-driven with Judge verdict** |
| **Tool security** | None (arbitrary code) | **Constitutional AI safety + Docker sandboxing** |
| **Dangerous command detection** | None | **9 built-in patterns (rm -rf, DROP TABLE, curl\|bash…)** |
| **Deployment** | npm/pip + runtime + Docker | **Single static binary** |
| **MCP support** | Plugin/community | **Native (stdio + SSE/Streamable HTTP)** |
| **Parallel tool execution** | Rare | **Built-in** |
| **Multi-agent workflows** | Separate orchestration layer | **Built-in DAG engine + orchestrator** |
| **Channel adapters** | Separate projects | **Built-in (Telegram, Discord, Slack, WhatsApp)** |
| **HTTP Gateway** | Separate project | **Built-in with Web UI + RBAC** |

If you've used Claude Code, Aider, or Cursor and wanted something lighter, self-hosted, or with a proper security model — Ryvos is built for you.

---

## What is Ryvos?

Ryvos is an open-source, autonomous AI coding assistant and agent runtime you run on your own hardware. It connects to **18+ LLM providers** (Anthropic, OpenAI, Gemini, Azure, Cohere, Ollama, Groq, OpenRouter, Together, Fireworks, Cerebras, xAI, Mistral, Perplexity, DeepSeek, Bedrock, Claude Code, GitHub Copilot), executes tasks through **86+ sandboxed tools**, and reaches you on the channels you already use — Telegram, Discord, Slack, WhatsApp, Webhooks — plus a built-in Web UI and terminal interface.

Written in **Rust**. Ships as a **single binary**. Uses **15–30 MB of RAM**.

---

## Performance

Benchmarks from a live v0.8.2 instance on Linux x86_64 (Intel i3-10105, 12 GB RAM).

| Metric | Value |
|--------|-------|
| Binary size | 45 MB (stripped, thin LTO) |
| CLI startup | < 6 ms |
| Daemon RSS | 57 MB (9 threads, all subsystems active) |
| Heartbeat cycle | ~38 s average |
| Telegram response | ~11 s |
| Data on disk | ~9 MB (all databases combined) |
| Rust LOC | 39,863 |
| Dependencies | 467 transitive, 31 direct |

---

## Features

### Goal-Driven Execution
- **Goals with weighted success criteria** — define what "done" means with `OutputContains`, `OutputEquals`, `LlmJudge`, or `Custom` criteria, each with individual weights
- **Constraints** — hard and soft limits on time, cost, safety, scope, and quality
- **Two-level Judge** — Level 0 (deterministic fast-check) + LLM ConversationJudge that evaluates full conversation context
- **Verdicts** — `Accept(confidence)`, `Retry(reason, hint)`, `Escalate(reason)`, or `Continue` — the agent keeps going until the goal is met or turns run out

### Autonomous AI Agent
- **ReAct agent loop** with tool use, reflexion, and streaming responses
- **Parallel tool execution** — multiple tools run concurrently when independent
- **18+ LLM providers** — Anthropic, OpenAI, Gemini, Azure, Cohere, Ollama, Groq, OpenRouter, Together, Fireworks, Cerebras, xAI, Mistral, Perplexity, DeepSeek, Bedrock, Claude Code, GitHub Copilot
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
- **Telegram, Discord, Slack, WhatsApp** — talk to your AI assistant on the platforms you already use
- **Per-channel DM policies** — allowlist, open, or disabled access control per channel
- **HTTP/WebSocket Gateway** — Axum-based server with embedded Web UI for browser access
- **Terminal UI** — full ratatui-based TUI with adaptive banner and streaming output
- **Interactive REPL** — quick command-line usage
- **Daemon mode** — always-on background service with `--gateway` flag
- **Cron scheduler** — recurring tasks with cron expressions, persistent across restarts
- **Heartbeat** — periodic proactive agent checks with smart suppression and alert routing

### Security (Constitutional AI Safety)
- **Constitutional self-learning safety** — the agent reasons about every action using 7 built-in principles
- **No tool is ever blocked** — safety comes from understanding, not prohibition
- **Safety Memory** — the agent learns from past mistakes via SafetyMemory corrective rules
- **Dangerous pattern detection** — 9 built-in patterns trigger explicit constitutional reasoning
- **Docker sandboxing** — optional container isolation with memory limits, network isolation, and timeouts
- **Optional checkpoints** — configure `pause_before` for tools that should wait for human acknowledgment
- **Budget enforcement** — monthly dollar limits with soft warnings and hard stops
- **Guardian watchdog** — detects stalls, doom loops (same tool called repeatedly), and budget overruns; injects corrective hints

### Observability
- **JSONL runtime logging** — three-level logging (L1 run summary, L2 per-turn detail, L3 tool execution) — crash-resilient append-only format
- **Decision journal** — SQLite-backed log of every tool call decision with alternatives and outcomes
- **Scoped EventBus** — subscribe to filtered events by type, session, or node for monitoring and integrations
- **Goal evaluation events** — stream `GoalEvaluated` and `JudgeVerdict` events to TUI, gateway, or custom subscribers
- **Token usage tracking** — per-turn and per-run input/output token counts

### Tools & Extensibility (86+ Built-in Tools)
- **86+ built-in tools** — shell, file I/O, git, code analysis, network/HTTP, system, data transform, scheduling, database, sessions, memory, notifications, browser, and more across 18 categories
- **MCP-native** — connect to any Model Context Protocol server (stdio + SSE/Streamable HTTP transports)
- **Drop-in skills** — Lua/Rhai scripts in `~/.ryvos/skills/` with manifest-declared schemas and sandbox requirements
- **Tool registry** — built-in tools + custom tools via MCP or skills
- **Role-based API keys** — Viewer, Operator, Admin roles for gateway access
- **Soul interview** — `ryvos soul` runs a 5-question personality interview that generates SOUL.md, shaping agent tone, proactivity, and operator context

### Viking Memory
- **Hierarchical context database** — L0/L1/L2 tiered loading with FTS search for fast, relevant context retrieval

### Browser Automation
- **5 browser tools** — navigate, screenshot, click, type, extract (powered by Chromium)

### WhatsApp Channel
- **Cloud API adapter** for WhatsApp Business — full bidirectional messaging

### Budget System
- **Monthly dollar limits** with configurable warn/hard-stop thresholds

### Semantic Memory
- **Embedding-based search** for long-term context retrieval across sessions

### Constitutional AI Safety
- **Self-learning safety** with 7 principles and SafetyMemory — the agent improves its safety behavior over time

---

## Quick Start

### Install

```bash
# One-line install (Linux / macOS) — recommended
curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh

# Pin a specific version
RYVOS_VERSION=v0.8.2 curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh

# Custom install directory
RYVOS_INSTALL_DIR=/usr/local/bin curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh
```

<details>
<summary>Build from source (Rust 1.75+)</summary>

```bash
cargo install --path .
```
</details>

<div align="center">

<img src="docs/demo.gif" alt="Ryvos demo — constitutional safety evaluates every action" width="720">

*Constitutional safety evaluates every action with full audit logging*

</div>

### Get Started

```bash
# Interactive setup — pick a provider, configure security, install service
ryvos init

# Non-interactive setup (defaults to Ollama / qwen2.5:7b for local inference)
ryvos init -y --provider ollama --model-id qwen2.5:7b

# Start your AI coding assistant
ryvos

# Ask a quick question and exit
ryvos run "Summarize the last 5 git commits in this repo"

# Launch the terminal UI
ryvos tui

# Start the Web UI + HTTP/WebSocket gateway
ryvos serve

# Always-on: Telegram + Discord + Slack + WhatsApp + gateway in one process
ryvos daemon --gateway

# Check system health
ryvos doctor
```

### Uninstall

```bash
rm ~/.local/bin/ryvos
rm -rf ~/.ryvos   # optional: remove config and data
```

### Shell Completions

```bash
ryvos completions bash > ~/.local/share/bash-completion/completions/ryvos  # bash
ryvos completions zsh > ~/.zfunc/_ryvos                                    # zsh
ryvos completions fish > ~/.config/fish/completions/ryvos.fish             # fish
```

### Commands

| Command | Description |
|---------|-------------|
| `ryvos` | Interactive conversation (default) |
| `ryvos run <prompt>` | Ask a question, get an answer, exit |
| `ryvos tui` | Terminal UI with streaming output |
| `ryvos serve` | Web UI + HTTP/WebSocket gateway |
| `ryvos daemon` | Always-on assistant (Telegram, Discord, Slack, WhatsApp) |
| `ryvos daemon --gateway` | Always-on + Web UI in one process |
| `ryvos init` | Interactive setup wizard |
| `ryvos init -y` | Non-interactive setup with defaults |
| `ryvos soul` | Personalize your agent (5-question interview → SOUL.md) |
| `ryvos config` | Print resolved configuration |
| `ryvos doctor` | System health checks (API, workspace, DB, channels, cron, MCP, security) |
| `ryvos health` | Tool health statistics |
| `ryvos mcp list` | List configured MCP servers |
| `ryvos mcp add <name>` | Add an MCP server |
| `ryvos completions <shell>` | Generate shell completions (bash, zsh, fish) |

---

## Architecture

Ryvos is a Cargo workspace with 10 crates. Together they form a complete autonomous AI agent runtime — goal-driven LLM reasoning, DAG workflow orchestration, tool execution, security enforcement, persistent memory, multi-channel inbox, and observability — all in one binary.

```
┌─────────────────────────────────────────────────────┐
│                     ryvos (CLI)                     │
├──────────┬──────────┬───────────┬───────────────────┤
│ ryvos-tui│  ryvos-  │  ryvos-   │  ryvos-channels   │
│  (TUI)   │ gateway  │  agent    │(Telegram/Discord/  │
│          │(HTTP/WS) │           │ Slack/WhatsApp)    │
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
| `ryvos-llm` | LLM client abstraction with streaming support (18+ providers) |
| `ryvos-tools` | Tool registry, 86+ built-in tools across 18 categories |
| `ryvos-agent` | ReAct loop, SecurityGate, ApprovalBroker, Guardian watchdog, Judge, GoalEvaluator, OutputValidator, CheckpointStore, RunLogger, CronScheduler, GraphExecutor, MultiAgentOrchestrator |
| `ryvos-memory` | SQLite-backed session and history storage |
| `ryvos-gateway` | Axum HTTP/WS server, Web UI, role-based auth middleware |
| `ryvos-channels` | Telegram, Discord, Slack, WhatsApp adapters with DM policy enforcement |
| `ryvos-mcp` | MCP client (stdio + SSE transports) with sampling control |
| `ryvos-skills` | Drop-in skill loader (Lua/Rhai) with manifest validation |
| `ryvos-tui` | Terminal UI built on ratatui with adaptive banner |

---

## Security

Ryvos uses a **constitutional self-learning safety model** — the agent reasons about the appropriateness of every action using 7 built-in principles.

**No tool is ever blocked.** Safety comes from understanding, not prohibition.

### How It Works

1. **Tool classification** — every tool has a security tier (T0 safe → T4 critical) for audit and context
2. **Constitutional reasoning** — the agent evaluates each action against 7 principles: Preservation, Intent Match, Proportionality, Transparency, Boundaries, Secrets, Learning
3. **Safety Memory** — the agent learns from past mistakes. SafetyMemory stores lessons as corrective rules that improve behavior over time
4. **Full audit trail** — every tool call is logged with input, output, safety reasoning, and outcome

### Additional Safety Layers

- **Dangerous pattern detection** — 9 built-in patterns (rm -rf, DROP TABLE, curl|bash, etc.) trigger explicit constitutional reasoning
- **Docker sandboxing** — optional isolated execution for file system and network operations
- **Optional checkpoints** — configure `pause_before` for tools that should wait for human acknowledgment
- **Budget enforcement** — monthly dollar limits with soft warnings and hard stops

> The old tier-based blocking system has been replaced. Tiers are retained for classification and backward compatibility, but they do not gate execution. See the [security documentation](https://ryvos.dev/docs/security/overview) for details.

---

## Configuration

Configuration lives in `~/.ryvos/config.toml` (created by `ryvos init`). You can also place a `ryvos.toml` in the current directory.

```toml
[agent]
max_turns = 25
parallel_tools = true
enable_self_eval = true

[agent.checkpoint]
enabled = true

[agent.log]
enabled = true
log_dir = "~/.ryvos/logs"

[agent.guardian]
stall_timeout_secs = 60
doom_loop_threshold = 5
budget_tokens = 100000

[model]
provider = "anthropic"
model_id = "claude-sonnet-4-20250514"
api_key = "${ANTHROPIC_API_KEY}"

# Local / self-hosted (no API key required):
# provider = "ollama"
# model_id = "qwen2.5:7b"

[security]
mode = "constitutional"       # constitutional | legacy-tier
pause_before = ["shell_exec"] # optional human checkpoints
budget_monthly_usd = 50.0
budget_warn_pct = 80

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
- [x] WhatsApp channel adapter (shipped in v0.5.0)
- [x] Browser control — navigate, click, extract, screenshot (shipped in v0.5.0)
- [x] Ryvos Cloud — hosted assistant with managed sessions (in preview at [cloud.ryvos.dev](https://cloud.ryvos.dev))

### Upcoming
- [ ] Pre-built binaries (Windows, macOS, Linux) via GitHub Releases
- [ ] `cargo install ryvos` from crates.io
- [ ] Signal, iMessage, and Google Chat channel adapters
- [ ] Voice mode — wake word detection + speech-to-text + TTS
- [ ] Mobile companion apps (iOS, Android) via WebSocket
- [ ] Live Canvas — real-time document/artifact editing in Web UI
- [ ] SOC 2 compliance documentation
- [ ] Signed & verified skill marketplace
- [ ] MCP sampling support (server-initiated LLM calls)

---

## Acknowledgments & Inspirations

Ryvos stands on the shoulders of great projects:

- [Claude Code](https://claude.ai/code) — Developer-first CLI patterns and ReAct loop design
- [Aider](https://github.com/paul-gauthier/aider) — Lightweight coding assistant philosophy
- [Aden Hive](https://github.com/aden-hive/hive) — Goal-driven graph execution and evolution loops
- [OpenClaw](https://github.com/openclaw/openclaw) — Channel adapter architecture and skills marketplace model
- [OpenViking](https://github.com/volcengine/OpenViking) — Hierarchical context database with L0/L1/L2 tiered loading
- [Paperclip](https://github.com/paperclipai/paperclip) — Multi-agent fleet orchestration patterns
- [Model Context Protocol](https://modelcontextprotocol.io) — Open standard for LLM tool integration

## Contributing

We welcome contributions. See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

## Get Help

- [GitHub Issues](https://github.com/Ryvos/ryvos/issues) — bug reports and feature requests
- [GitHub Discussions](https://github.com/Ryvos/ryvos/discussions) — questions and community

---

## License

[MIT](LICENSE)
