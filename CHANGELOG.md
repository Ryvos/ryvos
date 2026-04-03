# Changelog

All notable changes to Ryvos will be documented in this file.

## [0.8.2] — 2026-04-03

### Director Orchestration (fully wired)
- Director OODA loop now fires in production for goal-driven runs
- New Goals page in Web UI for creating, tracking, and evaluating goals
- Cron jobs with `goal` field route through Director instead of plain ReAct
- Improved output formatting across all channels

### Fixes
- `cargo fmt` and `cargo clippy` clean across entire workspace

## [0.8.1] — 2026-04-02

### Heartbeat Auto-Bootstrap
- HEARTBEAT.md is auto-created on `ryvos init` and on first heartbeat fire
- New users no longer need to manually create the heartbeat prompt file

## [0.8.0] — 2026-03-31

### Constitutional AI Safety Pipeline (fully wired)
- SafetyMemory, Reflexion, and corrective rules are now active in production
- Self-learning safety: destructive patterns detected, lessons recorded, context injected
- 2,500+ audit entries processed with 100% Harmless classification
- Case-insensitive tool name matching for CLI provider side-effects check

## [0.7.2] — 2026-03-29

### Native OAuth Integrations
- One-click OAuth for Gmail, Slack, GitHub, Jira, and Linear
- Browser-based OAuth flow via Web UI Integration Settings page
- Tokens stored securely in `~/.ryvos/integrations.db` and refreshed automatically
- 5 pre-configured OAuth providers with `generate_auth_url()` and `exchange_code()`

### Fixes
- Cron job results now correctly route to Telegram via CronJobComplete event handler
- Fixed cron schedule timezone (08:00 IST was firing at 13:30 IST, now correct)

## [0.7.1] — 2026-03-28

### Fixes
- Fixed UTF-8 panic caused by emoji truncation in `&s[..120]` byte slicing
- Changed 7 locations to use `chars().take(N)` instead of byte slicing
- This bug was killing heartbeat cycles silently for ~10 hours

## [0.7.0] — 2026-03-27

### Dormant Systems Activated
- Director orchestration, Reflexion self-healing, Guardian watchdog, and CostStore now activate on every daemon start (previously required explicit config)
- CostStore always created (was gated on `[budget]` config existing)

### Web UI Management Pages
- Cron job editor (add, remove, list scheduled tasks)
- Budget controls (view spend, set monthly limit, warn/hard-stop thresholds)
- Model switcher (change provider and model from browser)
- Integration settings (configure OAuth apps, view token status)
- Audit trail with tool breakdown statistics

### Admin Auth Fix
- Anonymous access defaults to Admin role for self-hosted single-user mode
- Previously defaulted to Viewer, blocking all writes from Web UI

## [0.6.11] — 2026-03-25

### Neo-Brutalist Theme Overhaul
- Complete Web UI rewrite: 13 Svelte pages in Neo-Brutalist design
- Light background (#FEFCF9), 2px solid borders, brutal shadows (4px 4px 0px)
- DM Serif Display headings, Plus Jakarta Sans body, JetBrains Mono code
- Dashboard with metrics strip, tool usage chart, activity feed, Guardian alerts
- New pages: VikingBrowser, ConfigEditor, CommandPalette (Cmd+K)
- Responsive sidebar with mobile hamburger menu

### MCP Server Capabilities Fix
- Server now declares `"capabilities": {"tools": {}}` so Claude Code discovers all 9 tools
- Previously returned empty capabilities object

## [0.6.0] — 2026-03-18

### Security Overhaul
- Deprecated tier-based blocking in favor of passthrough security
- SecurityGate: logs all tool calls, never blocks, learns from outcomes
- SafetyMemory (SQLite): detects destructive patterns, records lessons, injects context
- 7 Constitutional AI principles in every system prompt

### Viking Server
- Standalone HTTP/REST memory server (`ryvos viking-server`, port 1933)
- Viking context injection into agent system prompt before every run
- Dual-write pattern: tools write to both SQLite and Viking

### Global Audit Trail
- Every tool execution logged to audit.db with safety reasoning
- Audit stats API with tool breakdown and session counts

### Web Sessions Fix
- Web chat sessions now properly register with session manager

## [0.5.0] — 2026-03-13

### Browser Automation (5 new tools, 67 total)
- `browser_navigate` (T3) — Navigate to a URL
- `browser_screenshot` (T3) — Screenshot the current page
- `browser_click` (T3) — Click an element by CSS selector
- `browser_type` (T3) — Type text into an input field
- `browser_extract` (T3) — Extract text content from the page
- Powered by chromiumoxide (Chromium DevTools Protocol)

### WhatsApp Cloud API Channel (8 channels total)
- New channel adapter: `[channels.whatsapp]`
- Meta Business webhook integration with automatic verification
- Interactive approval buttons for tool calls
- E.164 phone number format, 4096-char auto-split
- DM policy (allowlist/open/disabled)

### 19 LLM Providers (+3)
- **AWS Bedrock** — full SigV4 authentication
- **Claude Code** — CLI delegation with subscription billing
- **GitHub Copilot** — CLI delegation with Copilot license

### Budget System
- `[budget]` config section: `monthly_budget_cents`, `warn_pct`, `hard_stop_pct`
- Per-model pricing overrides via `[budget.pricing."provider/model"]`
- Guardian monitors token usage; publishes `BudgetWarning` / `BudgetExceeded` events
- Budget events routed to configured channels

### Semantic Memory (Embeddings)
- `[embedding]` config section: `provider`, `model`, `dimensions`
- Supports OpenAI, Ollama, and custom (OpenAI-compatible) providers
- Hybrid BM25 + vector cosine similarity ranking for `memory_search`
- Daily logs auto-embedded on write

### Director Orchestration (enabled by default)
- Director now enabled by default for all installations
- Goal-driven multi-agent OODA loop: Generate → Execute → Evaluate → Evolve
- Auto-evolving execution graphs on semantic failure
- Configurable via `[agent.director]`: `max_evolution_cycles`, `failure_threshold`, `model`

### 10-Phase Onboarding
- `ryvos init` walks through: provider, model, security, channels, sandbox, guardian, heartbeat, budget, embedding, summary

### 6 Bundled Skills
- web-scraper, code-review, git-summary, docker-deploy, db-migrate, api-test

### Webhook Enhancements
- `metadata` and `callback_url` fields in webhook payloads
- `CronJobComplete` event with response routing to channels

### New Dependencies
- sha2 (replaces DefaultHasher for skills registry integrity)
- chromiumoxide (browser automation)
- base64

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
