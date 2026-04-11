# Configuration

Ryvos reads a single TOML file at startup. The default path is
`./ryvos.toml` in the current working directory, falling back to
`~/.ryvos/config.toml` if the first file is missing. The file is parsed by
`AppConfig::load` in `crates/ryvos-core/src/config.rs:1124`, which first runs
every string value through **[environment variable expansion](environment-variables.md)**
and then passes the result to `toml::from_str`. Every field not listed in the
file falls back to the `#[serde(default)]` value declared alongside the
struct.

This document is the field-by-field reference. It is organized by the TOML
section each field lives under. Defaults listed in the tables match the
constants in `config.rs`; when the source changes, this document is out of
date. `ryvos config` prints the parsed config as normalized TOML and is the
authoritative runtime view.

## Top-level structure

The top-level `AppConfig` struct (`config.rs:141`) nests the following
optional sections. `[model]` is the only required section — Ryvos refuses to
start without at least a provider and a `model_id`.

| Section | Required | Purpose |
|---|---|---|
| `[agent]` | No | Agent loop limits, workspace, guardian, checkpoint, logs, director. |
| `[model]` | Yes | Primary LLM provider and credentials. |
| `[[fallback_models]]` | No | Ordered fallback chain of alternate `ModelConfig` entries. |
| `[gateway]` | No | HTTP/WebSocket gateway bind address and auth. |
| `[channels.telegram]` / `[channels.discord]` / `[channels.slack]` / `[channels.whatsapp]` | No | Channel adapter credentials and DM policy. |
| `[mcp.servers.<name>]` | No | External MCP server definitions. |
| `[hooks]` | No | Shell commands fired at lifecycle points. |
| `[cron]` | No | Persistent cron jobs. |
| `[heartbeat]` | No | Timer-driven self-check. |
| `[web_search]` | No | Brave or Tavily API credentials for the `web_search` tool. |
| `[security]` | No | Soft checkpoints and deprecated tier knobs. |
| `[embedding]` | No | Embedder provider for semantic memory. |
| `[daily_logs]` | No | Daily markdown log retention. |
| `[registry]` | No | Skill registry URL and cache. |
| `[budget]` | No | Monthly dollar budget and pricing overrides. |
| `[openviking]` | No | Standalone Viking server URL and user. |
| `[google]` / `[notion]` / `[jira]` / `[linear]` | No | Per-provider integration credentials. |
| `[integrations]` | No | One-click OAuth app registrations. |

## `[agent]`

Fields in `AgentConfig` (`config.rs:194`). Every field has a default; omitting
the whole `[agent]` section is equivalent to accepting every value in the
table below.

| Field | Type | Default | Description |
|---|---|---|---|
| `max_turns` | integer | `25` | Hard cap on **[turns](../glossary.md#turn)** per run. |
| `max_duration_secs` | integer | `600` | Wall-clock limit per run. |
| `workspace` | string | `"~/.ryvos"` | Workspace directory; `~` expands to `$HOME`. |
| `system_prompt` | string | `null` | Overrides the built-in system prompt. |
| `max_context_tokens` | integer | `80000` | Token budget for the **[onion context](../glossary.md#onion-context)** before compaction fires. |
| `max_tool_output_tokens` | integer | `4000` | Per-tool-call output cap; longer outputs are truncated. |
| `reflexion_failure_threshold` | integer | `3` | Consecutive failures of the same tool before **[Reflexion](../glossary.md#reflexion)** hints inject. |
| `parallel_tools` | bool | `true` | Dispatch independent tool calls concurrently. |
| `enable_summarization` | bool | `true` | Use an LLM pass to compact context on overflow. |
| `enable_self_eval` | bool | `false` | Run LLM-as-judge scoring after each run. |
| `disable_memory_flush` | bool | `null` | Opt out of the pre-compaction memory flush. |
| `model_overrides` | table | `{}` | Per-agent-id model routing (`agent_id → ModelConfig`). |

Four nested sections live under `[agent]`.

### `[agent.guardian]`

Controls the **[Guardian](../glossary.md#guardian)** watchdog (see
[../internals/guardian.md](../internals/guardian.md)).

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `true` | Spawn the guardian task on daemon start. |
| `doom_loop_threshold` | integer | `3` | Consecutive identical tool calls that trigger a **[doom loop](../glossary.md#doom-loop)** event. |
| `stall_timeout_secs` | integer | `120` | Seconds of no activity before a stall event fires. |
| `token_budget` | integer | `0` | Total token ceiling for a run. `0` means unlimited. |
| `token_warn_pct` | integer | `80` | Soft warning at this percentage of `token_budget`. |

### `[agent.director]`

Controls the **[Director](../glossary.md#director)** (see
[../internals/director-ooda.md](../internals/director-ooda.md)). Always
constructed with defaults if missing.

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `true` | Route runs with attached goals through the Director. |
| `max_evolution_cycles` | integer | `3` | Plan evolution cycles before escalation. |
| `failure_threshold` | integer | `3` | Semantic failures before evolution triggers. |
| `model` | table | inherit | Optional `ModelConfig` override for the Director's planning LLM. |

### `[agent.log]`

`RunLogger` writes JSONL runtime logs to disk. See
[../internals/agent-loop.md](../internals/agent-loop.md).

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `true` | Start the logger task. |
| `log_dir` | string | `<workspace>/logs` | Output directory for `runs.jsonl`. |
| `level` | integer | `2` | `1` run summary only, `2` per-turn detail, `3` per-step detail. |

### `[agent.checkpoint]`

Controls per-turn checkpointing (see
[../internals/checkpoint-resume.md](../internals/checkpoint-resume.md)).

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `true` | Persist turn snapshots to `sessions.db`. |
| `checkpoint_dir` | string | `<workspace>/checkpoints` | Directory for auxiliary checkpoint files. |

### `[agent.sandbox]`

Optional Docker sandbox for the `bash` tool.

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Route bash tool calls through a container. |
| `mode` | string | `"docker"` | Backend driver. Only `docker` is supported. |
| `image` | string | `"ubuntu:24.04"` | Container image. |
| `memory_mb` | integer | `512` | Memory limit per container. |
| `timeout_secs` | integer | `120` | Per-command wall-clock limit. |
| `mount_workspace` | bool | `true` | Bind-mount the workspace into the container. |

## `[model]`

The primary `ModelConfig` (`config.rs:430`). `provider` and `model_id` are
the only required fields; the rest default to the provider preset applied by
`ryvos_llm::apply_preset_defaults`.

| Field | Type | Default | Description |
|---|---|---|---|
| `provider` | string | `"anthropic"` | Provider name (anthropic, openai, gemini, azure, cohere, ollama, groq, openrouter, together, fireworks, cerebras, xai, mistral, perplexity, deepseek, bedrock, claude-code, copilot). |
| `model_id` | string | — | Model identifier. Required. |
| `api_key` | string | `null` | Credential. Accepts `${ENV_VAR}` expansion. |
| `base_url` | string | preset | Override the default base URL. |
| `max_tokens` | integer | `8192` | Output token cap per LLM call. |
| `temperature` | float | `0.0` | Sampling temperature. |
| `thinking` | enum | `off` | `off`/`low`/`medium`/`high` reasoning tokens. |
| `retry` | table | `null` | `RetryConfig` (see below). |
| `azure_resource` | string | `null` | Azure OpenAI resource name. |
| `azure_deployment` | string | `null` | Azure OpenAI deployment name. |
| `azure_api_version` | string | `null` | Azure OpenAI API version. |
| `aws_region` | string | `null` | AWS region for Bedrock. |
| `extra_headers` | table | `{}` | Extra HTTP headers per LLM request. |
| `claude_command` | string | `null` | Path to `claude` CLI (claude-code provider). |
| `cli_allowed_tools` | array | `[]` | Tool allowlist for Claude CLI subprocess. |
| `cli_permission_mode` | string | `null` | `default`, `plan`, `dontAsk`, or `bypassPermissions`. |
| `copilot_command` | string | `null` | Path to `gh copilot` CLI (copilot provider). |

### `RetryConfig`

Embedded under `[model.retry]` or any entry in `[[fallback_models]]`.

| Field | Type | Default | Description |
|---|---|---|---|
| `max_retries` | integer | `3` | Attempts before falling back to the next model. |
| `initial_backoff_ms` | integer | `1000` | First backoff interval. |
| `max_backoff_ms` | integer | `30000` | Backoff ceiling; doubles until this cap. |

## `[[fallback_models]]`

Zero or more `ModelConfig` entries. `RetryingClient` in `ryvos-llm` cycles
through the fallback chain when the primary provider exhausts its retries.
Each entry is a full `ModelConfig` — same schema as `[model]`.

## `[gateway]`

Controls the HTTP/WebSocket surface. See
[../crates/ryvos-gateway.md](../crates/ryvos-gateway.md).

| Field | Type | Default | Description |
|---|---|---|---|
| `bind` | string | `"127.0.0.1:18789"` | TCP bind address. |
| `token` | string | `null` | Deprecated single admin bearer token. |
| `password` | string | `null` | Deprecated admin query-string password. |
| `api_keys` | array | `[]` | Zero or more `ApiKeyConfig` entries. |
| `webhooks` | table | `null` | `WebhookConfig` for `/api/hooks/wake`. |

### `[[gateway.api_keys]]`

| Field | Type | Default | Description |
|---|---|---|---|
| `name` | string | — | Label for logging. |
| `key` | string | — | Bearer token value. |
| `role` | enum | `operator` | `viewer`, `operator`, or `admin`. |

### `[gateway.webhooks]`

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable `/api/hooks/wake`. |
| `token` | string | `null` | Shared secret for inbound webhook calls. |

## `[channels.*]`

Each channel is optional; include a section to enable the adapter.

### `[channels.telegram]`

| Field | Type | Default | Description |
|---|---|---|---|
| `bot_token` | string | — | Token from `@BotFather`. |
| `allowed_users` | array of int64 | `[]` | Telegram user IDs on the allowlist. |
| `dm_policy` | enum | `allowlist` | `allowlist`, `open`, or `disabled`. |

### `[channels.discord]`

| Field | Type | Default | Description |
|---|---|---|---|
| `bot_token` | string | — | Discord bot token. |
| `allowed_users` | array of u64 | `[]` | Discord user IDs on the allowlist. |
| `dm_policy` | enum | `allowlist` | DM policy. |

### `[channels.slack]`

| Field | Type | Default | Description |
|---|---|---|---|
| `bot_token` | string | — | `xoxb-...` token for the Web API. |
| `app_token` | string | — | `xapp-...` token for Socket Mode. |
| `allowed_users` | array of string | `[]` | Slack user IDs on the allowlist. |
| `dm_policy` | enum | `allowlist` | DM policy. |

### `[channels.whatsapp]`

| Field | Type | Default | Description |
|---|---|---|---|
| `access_token` | string | — | Permanent Meta Business access token. |
| `phone_number_id` | string | — | Meta Business phone number ID. |
| `verify_token` | string | — | Webhook handshake token. |
| `allowed_users` | array of string | `[]` | E.164 phone numbers on the allowlist. |
| `dm_policy` | enum | `allowlist` | DM policy. |

## `[mcp.servers.<name>]`

One entry per external MCP server. The section key is the server's logical
name; the body is an `McpServerConfig` (`config.rs:762`).

| Field | Type | Default | Description |
|---|---|---|---|
| `transport` | table | — | Discriminated union: `{ type = "stdio", command, args, env }` or `{ type = "sse", url }`. |
| `auto_connect` | bool | `true` | Connect on daemon start. |
| `allow_sampling` | bool | `false` | Allow the server to call back for LLM inference. |
| `timeout_secs` | integer | `120` | Per-tool-call timeout. |
| `tier_override` | string | `null` | Force every tool from this server to a specific tier. |
| `headers` | table | `{}` | Custom HTTP headers for SSE transport. |

`.mcp.json` in the current directory is merged into `[mcp.servers]` at
startup using `McpJsonServerEntry::to_server_config` (`config.rs:826`),
matching Claude Code's project-local convention.

## `[hooks]`

Lifecycle shell hooks. Every field is an array of shell commands run with
the session and context injected through environment variables like
`RYVOS_SESSION` and `RYVOS_TEXT`.

| Field | Fired on |
|---|---|
| `on_start` | REPL/daemon start. |
| `on_message` | Every inbound user message. |
| `on_tool_call` | Before each tool dispatch. |
| `on_response` | After each run completes. |
| `on_turn_complete` | After each turn. |
| `on_tool_error` | After each tool failure. |
| `on_session_start` | New session created. |
| `on_session_end` | Session closed. |

## `[cron]`

Persistent cron jobs; each entry uses standard five-field cron syntax.

```toml
[[cron.jobs]]
name = "morning-standup"
schedule = "0 9 * * *"
prompt = "Summarize overnight activity"
channel = "telegram"
goal = "Deliver a concise status update"
```

| Field | Type | Description |
|---|---|---|
| `name` | string | Job label. |
| `schedule` | string | Cron expression. |
| `prompt` | string | Initial prompt sent at fire time. |
| `channel` | string | Optional routing channel. |
| `goal` | string | When set, routes the run through the Director. |

## `[heartbeat]`

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable **[Heartbeat](../glossary.md#heartbeat)**. |
| `interval_secs` | integer | `1800` | Seconds between checks. |
| `target_channel` | string | `null` | Alert routing target. |
| `active_hours` | table | `null` | `ActiveHoursConfig` (below). |
| `ack_max_chars` | integer | `300` | Max response length considered an ack. |
| `heartbeat_file` | string | `"HEARTBEAT.md"` | Workspace file used as the prompt. |
| `prompt` | string | `null` | Override the default prompt. |

### `[heartbeat.active_hours]`

| Field | Type | Default | Description |
|---|---|---|---|
| `start_hour` | u8 | `9` | Window start hour (0-23). |
| `end_hour` | u8 | `22` | Window end hour (0-23). |
| `utc_offset_hours` | i32 | `0` | Local offset from UTC. |

## `[web_search]`

| Field | Type | Default | Description |
|---|---|---|---|
| `provider` | string | `"tavily"` | `brave` or `tavily`. |
| `api_key` | string | — | Credential. |

## `[security]`

| Field | Type | Default | Description |
|---|---|---|---|
| `auto_approve_up_to` | enum | `T1` | **Deprecated.** Pre-v0.6 tier ceiling. |
| `deny_above` | enum | `null` | **Deprecated.** Pre-v0.6 deny ceiling. |
| `approval_timeout_secs` | integer | `60` | Soft-checkpoint acknowledgment timeout. |
| `tool_overrides` | table | `{}` | Per-tool tier overrides; informational. |
| `dangerous_patterns` | array | `[]` | **Deprecated.** No longer blocks. |
| `sub_agent_policy` | table | `null` | Retained for backwards compatibility. |
| `pause_before` | array | `[]` | Tools that wait for an approval acknowledgment. |

See [../adr/002-passthrough-security.md](../adr/002-passthrough-security.md)
for the rationale behind the deprecation and
[../guides/migrating-from-tier-security.md](../guides/migrating-from-tier-security.md)
for the migration path.

## `[embedding]`

| Field | Type | Default | Description |
|---|---|---|---|
| `provider` | string | — | `openai`, `ollama`, or any OpenAI-compatible provider. |
| `model` | string | — | Model name, e.g., `text-embedding-3-small`. |
| `base_url` | string | `null` | API base URL. |
| `api_key` | string | `null` | Credential. |
| `dimensions` | integer | `1536` | Embedding vector length. |

## `[daily_logs]`

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `true` | Write `memory/YYYY-MM-DD.md` files. |
| `retention_days` | integer | `30` | Days to keep before pruning. |
| `log_dir` | string | `<workspace>/memory` | Output directory. |

## `[registry]`

| Field | Type | Default | Description |
|---|---|---|---|
| `url` | string | `https://raw.githubusercontent.com/Ryvos/registry/main/index.json` | Skill registry index. |
| `cache_dir` | string | `null` | Local cache for downloaded skill packages. |

## `[budget]`

| Field | Type | Default | Description |
|---|---|---|---|
| `monthly_budget_cents` | integer | — | Hard monthly ceiling. `0` means unlimited. |
| `warn_pct` | u8 | `80` | Soft warning threshold. |
| `hard_stop_pct` | u8 | `100` | Hard stop threshold. |
| `pricing` | table | `{}` | Per-model `ModelPricing` overrides (`input_cents_per_mtok`, `output_cents_per_mtok`). |

## `[openviking]`

| Field | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Route Viking operations to a standalone server. |
| `base_url` | string | `http://localhost:1933` | Viking server address. |
| `user_id` | string | `"ryvos-default"` | Namespace for `viking://` entries. |
| `auto_iterate` | bool | `true` | Auto-extract memories after each session. |

## Per-provider integrations

| Section | Fields |
|---|---|
| `[google]` | `client_secret_path`, `tokens_path`, `gmail` (true), `calendar` (true), `drive` (true), `contacts` (false). |
| `[notion]` | `api_key`. |
| `[jira]` | `base_url`, `email`, `api_token`. |
| `[linear]` | `api_key`. |

## `[integrations]`

One-click OAuth app registrations used by the Web UI integration page. Each
subsection stores a `client_id` / `client_secret` pair.

| Subsection | Type |
|---|---|
| `[integrations.gmail]` | `OAuthAppConfig` |
| `[integrations.slack]` | `OAuthAppConfig` |
| `[integrations.github]` | `OAuthAppConfig` |
| `[integrations.jira]` | `OAuthAppConfig` |
| `[integrations.linear]` | `OAuthAppConfig` |
| `[integrations.notion]` | `NotionIntegrationConfig` (`api_key` only — Notion uses internal tokens, not OAuth) |

## Environment variable expansion

Any string value — at any depth in the TOML tree — supports `${VAR}`
expansion. The expansion runs before TOML parsing, so a variable may supply
a quoted string, an integer literal, or even a whole fragment of TOML. A
reference to an undefined variable is left verbatim so the subsequent TOML
parse fails loudly rather than silently inserting an empty value. The full
list of variables Ryvos consumes is in
[environment-variables.md](environment-variables.md).

Cross-references:
[environment-variables.md](environment-variables.md),
[../crates/ryvos-core.md](../crates/ryvos-core.md),
[../adr/002-passthrough-security.md](../adr/002-passthrough-security.md).
