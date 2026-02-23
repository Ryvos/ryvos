# Changelog

All notable changes to Ryvos will be documented in this file.

## [0.1.0] — 2026-02-23

### Initial Release

Ryvos v0.1.0 — an autonomous AI assistant runtime written in Rust. Single binary, 15-30 MB RAM, multi-provider LLM, multi-channel inbox.

### Agent Runtime
- **ReAct agent loop** with streaming responses, parallel tool execution, and configurable turn/duration limits
- **Multi-provider LLM support** — Anthropic, OpenAI, Ollama, or any OpenAI-compatible API
- **Retry & fallback chains** — automatic retries with exponential backoff and model fallback
- **Extended thinking** — support for Anthropic and OpenAI reasoning tokens (off/low/medium/high)
- **Context management** — automatic pruning with optional LLM-powered summarization
- **Session persistence** — SQLite-backed conversation history across restarts
- **Sub-agent spawning** — delegate tasks to child agents with stricter security
- **Self-healing** — FailureJournal tracks tool failures, reflexion hints guide recovery
- **Guardian Watchdog** — event-driven background monitor detecting doom loops, stalls, and token budget overruns with automatic corrective hints
- **Self-evaluation** — optional LLM-as-judge scoring after each run

### Security
- **5-tier tool classification** (T0 safe → T4 critical)
- **SecurityGate middleware** — every tool call intercepted, policy enforced
- **Dangerous pattern detection** — 9 built-in regex patterns (rm -rf, DROP TABLE, curl|bash, chmod 777, etc.) with automatic T4 escalation
- **Human-in-the-loop approval** — configurable approval flows via REPL, TUI, Telegram, Discord, or WebSocket
- **Docker sandboxing** — optional container isolation with memory limits and network control
- **Sub-agent restrictions** — spawned agents inherit stricter security policies

### Tools & Extensibility
- **Built-in tools** — shell, file read/write/edit, web search
- **MCP-native** — Model Context Protocol client (stdio + SSE/Streamable HTTP transports)
- **Drop-in skills** — executable scripts in `~/.ryvos/skills/` with manifest-declared schemas and tiers
- **Dynamic tool refresh** — MCP tools auto-update when servers notify of changes
- **Tool health statistics** — `ryvos health` shows per-tool success/failure rates

### Channels & Interfaces
- **Interactive REPL** — command-line interface with slash commands
- **Terminal UI** — full ratatui-based TUI
- **HTTP/WebSocket Gateway** — Axum server with embedded Web UI
- **Role-based API keys** — Viewer, Operator, Admin access levels
- **Telegram adapter** — with DM policy enforcement and approval buttons
- **Discord adapter** — with allowlist/open/disabled DM policies
- **Slack adapter** — Socket Mode with bot + app token auth
- **Daemon mode** — always-on background service (`ryvos daemon --gateway`)

### Configuration
- **TOML config** — `ryvos.toml` or `~/.ryvos/config.toml`
- **Environment variable expansion** — `${VAR}` syntax in config values
- **Interactive setup wizard** — `ryvos init` with guided provider/channel configuration
- **Lifecycle hooks** — shell commands on start, message, tool call, response, turn complete, session events
- **Cron scheduler** — recurring tasks with cron expressions
- **Shell completions** — `ryvos completions bash/zsh/fish`

### Internals
- 10-crate Cargo workspace (core, llm, tools, agent, memory, gateway, tui, mcp, channels, skills)
- 95 tests across all crates
- Release profile: LTO, stripped binaries, opt-level 3
