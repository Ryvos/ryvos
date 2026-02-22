# Ryvos

[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

Blazingly fast AI agent runtime written in Rust. Multi-provider, multi-channel, extensible.

## Features

- **Multi-provider LLM** -- Anthropic, OpenAI, Ollama, or any OpenAI-compatible API
- **Agentic loop** -- ReAct agent with tool use, reflexion, and parallel tool execution
- **Built-in tools** -- Shell, file I/O, web search, and more
- **MCP support** -- Connect to Model Context Protocol servers (stdio + SSE/Streamable HTTP)
- **Gateway** -- HTTP/WebSocket server with embedded Web UI, role-based API keys
- **Channels** -- Telegram, Discord, and Slack bot adapters
- **TUI** -- Terminal UI built on ratatui
- **Drop-in skills** -- Extend with Lua/Rhai scripts in `~/.ryvos/skills/`
- **Lifecycle hooks** -- Shell commands on start, message, tool call, and response events
- **Daemon mode** -- Run as a background service with optional gateway (`--gateway`)

## Quick Start

```bash
# Install
cargo install --path .

# Interactive setup wizard
ryvos init

# Start REPL
ryvos

# Or run a single prompt
ryvos run "Summarize this project"
```

## Commands

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
| `ryvos config` | Print current configuration |
| `ryvos completions <shell>` | Generate shell completions |

## Architecture

Ryvos is organized as a Cargo workspace with 10 crates:

| Crate | Purpose |
|-------|---------|
| `ryvos-core` | Config, error types, event bus, traits |
| `ryvos-llm` | LLM client abstraction (streaming) |
| `ryvos-tools` | Tool registry and built-in tools |
| `ryvos-agent` | ReAct agent loop, session management |
| `ryvos-memory` | SQLite session/history storage |
| `ryvos-gateway` | Axum HTTP/WS server, Web UI, auth |
| `ryvos-channels` | Telegram, Discord, Slack adapters |
| `ryvos-mcp` | MCP client (stdio + SSE transports) |
| `ryvos-skills` | Drop-in skill loader |
| `ryvos-tui` | Terminal UI (ratatui) |

## Configuration

Configuration lives in `ryvos.toml` or `~/.ryvos/config.toml`. Environment variables are expanded with `${VAR}` syntax.

See [`ryvos.toml.example`](ryvos.toml.example) for a full reference.

## License

[MIT](LICENSE)
