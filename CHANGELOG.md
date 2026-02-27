# Changelog

All notable changes to Ryvos will be documented in this file.

## [0.2.2] — 2026-02-27

### Soul Interview
- New `ryvos soul` command — 5-question personality interview
- Generates ~/.ryvos/SOUL.md loaded into every conversation
- Available via CLI (`ryvos soul`), REPL (`/soul`), and during `ryvos init`
- Shapes communication style, personality, proactivity, and operator context

### Interactive Approval
- REPL now prompts Y/N inline via `dialoguer::Confirm` when approval is needed
- No more blocked-thread deadlock — approval works without needing `/approve` while agent is running
- Eased security defaults: `auto_approve_up_to = "t3"` (normal dev workflow auto-approved), `deny_above = "t4"`
- Only the 9 dangerous-pattern commands (rm -rf, DROP TABLE, force push, etc.) trigger an interactive prompt

### Heartbeat Fix
- Add `broadcast()` method to `ChannelAdapter` trait — sends to all known users without session mapping
- Telegram adapter implements `broadcast()` using `allowed_users` list
- Heartbeat alert router now uses `broadcast()` instead of `send()`, fixing the missing ChatId error
- Default `[heartbeat]` section added to config template

### Fixes
- Fix Telegram adapter TLS (enable rustls for teloxide)
- Fix TUI /approve and /deny commands
- Add bot token validation at Telegram startup
- Fix `cargo fmt` CI check (soul interview code)

## [0.2.0] — 2026-02-26

### 50 New Built-in Tools (62 total)
- **Sessions (5)**: session_list, session_history, session_send, session_spawn, session_status
- **Memory (3)**: memory_get, daily_log_write, memory_delete
- **File System (9)**: file_info, file_copy, file_move, file_delete, dir_list, dir_create, file_watch, archive_create, archive_extract
- **Git (6)**: git_status, git_diff, git_log, git_commit, git_branch, git_clone
- **Code/Dev (4)**: code_format, code_lint, test_run, code_outline
- **Network/HTTP (4)**: http_request, http_download, dns_lookup, network_check
- **System (5)**: process_list, process_kill, env_get, system_info, disk_usage
- **Data/Transform (8)**: json_query, csv_parse, yaml_convert, toml_convert, base64_codec, hash_compute, regex_replace, text_diff
- **Scheduling (3)**: cron_list, cron_add, cron_remove
- **Database (2)**: sqlite_query, sqlite_schema
- **Communication (1)**: notification_send

### 12 New LLM Providers (14 total)
- **4 dedicated implementations**: Google Gemini (native API), Azure OpenAI, Cohere v2, AWS Bedrock (stub)
- **10 OpenAI-compatible presets**: Ollama, Groq, OpenRouter, Together, Fireworks, Cerebras, xAI, Mistral, Perplexity, DeepSeek
- Automatic preset defaults for base_url and extra headers
- Per-agent model routing with `model_overrides` config

### Memory Flush Before Compaction
- Agent automatically persists durable information via memory_write and daily_log_write before context window compaction
- Prevents loss of important context during long-running sessions
- Configurable via `disable_memory_flush` in agent config

### Daily Append-Only Logs
- Timestamped daily log files at `~/.ryvos/memory/YYYY-MM-DD.md`
- Last 2 days' logs injected into agent context via ContextBuilder
- `daily_log_write` tool for structured journaling
- Configurable retention (default 30 days)

### Webhooks
- `POST /api/hooks/wake` endpoint for external integrations
- Bearer token authentication from gateway config
- Creates/resumes sessions from webhook payloads

### Skill Registry
- `ryvos skill install/list/search/remove` CLI commands
- Remote registry index (GitHub-hosted JSON)
- SHA-256 verification for downloaded skill packages
- Integration with existing drop-in skill system

### Non-Interactive Onboarding
- New flags: `--base-url`, `--security-level`, `--channels`, `--from-env`
- `--from-env` reads RYVOS_PROVIDER, RYVOS_MODEL_ID, RYVOS_API_KEY, etc. from environment
- Default model IDs for all 14 providers
- Connection test support

### Config Extensions
- `daily_logs` section (enabled, retention_days, log_dir)
- `registry` section (url, cache_dir) for skill registry
- `webhooks` section under gateway (enabled, token)
- Azure fields on ModelConfig (azure_resource, azure_deployment, azure_api_version)
- AWS region field on ModelConfig
- Extra headers map on ModelConfig
- `config_path` on ToolContext for cron tools

## [0.1.1] — 2026-02-26

### Heartbeat System
- Periodic proactive agent checks (configurable interval, default 30min)
- Smart suppression — ack responses silenced, actionable alerts routed to channels
- Active hours — restrict to time windows with timezone offset
- HEARTBEAT.md workspace file for custom check instructions
- Three new EventBus events: HeartbeatFired, HeartbeatOk, HeartbeatAlert

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
