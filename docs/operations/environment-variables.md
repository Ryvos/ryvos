# Environment variables

Ryvos reads two classes of environment variables. The first are consumed by
the binary itself — the workspace path, the tracing filter, the gateway
auth token in container deployments. The second are referenced from the
config file via `${VAR}` expansion and are only meaningful because a config
field points at them.

`expand_env_vars` in `crates/ryvos-core/src/config.rs:1147` is the canonical
expander. It walks the raw TOML source before `toml::from_str` parses it
and replaces every `${NAME}` with `std::env::var("NAME").unwrap_or(original)`.
Undefined variables are left verbatim so the subsequent TOML parse fails
loudly. The expander recognizes exactly the `${NAME}` shape — bare `$NAME`
is not expanded, and default-value syntax (`${NAME:-default}`) is not
supported.

## Runtime variables

These are read by the Ryvos binary itself or by the platform service
installer.

| Variable | Consumed by | Required | Example |
|---|---|---|---|
| `HOME` | `config.rs:1175` (workspace default, `~` expansion) | Effectively yes on Unix | `/home/ryvos` |
| `USER` | `src/onboard/service.rs:45` (systemd lingering) | No | `ryvos` |
| `RUST_LOG` | `tracing_subscriber` (`src/main.rs:203`) | No | `ryvos=info,ryvos_agent=debug` |
| `RYVOS_GATEWAY_TOKEN` | Docker/systemd deployments — expanded into `[gateway].token` | No | `rk_live_abc123` |
| `CLAUDECODE` | Unset by Ryvos before spawning the Claude CLI subprocess, preventing nested harness mode | No | (unset) |
| `GH_TOKEN` | Forwarded to the `gh copilot` CLI subprocess when the copilot provider is active | No | `ghp_xxx` |
| `CHROME_PATH` | Browser automation tools (`browser_navigate`, etc.) | No | `/usr/bin/chromium` |

`RUST_LOG` accepts the standard `tracing_subscriber` filter syntax. The
default when unset is `ryvos=info,warn` — info-level for every Ryvos crate
and warn-level for every external crate. Useful overrides for troubleshooting
are `ryvos=debug`, `ryvos_agent=trace`, or
`ryvos_gateway=debug,ryvos_llm=debug`.

## Provider API keys

These are all optional on their own. The config expands them into the
`api_key` field of a `[model]` or `[[fallback_models]]` entry whose
`provider` selects the matching endpoint. Ryvos never hard-codes a key to a
provider — the binding lives entirely in the TOML. Every string below is
the conventional name that provider presets use in `ryvos.toml.example`.

| Variable | Provider | Required |
|---|---|---|
| `ANTHROPIC_API_KEY` | `anthropic` | Yes if used |
| `OPENAI_API_KEY` | `openai`, `azure` (with `azure_*` config) | Yes if used |
| `GOOGLE_AI_API_KEY` | `gemini` | Yes if used |
| `AZURE_OPENAI_API_KEY` | `azure` | Yes if used |
| `GROQ_API_KEY` | `groq` | Yes if used |
| `TOGETHER_API_KEY` | `together` | Yes if used |
| `FIREWORKS_API_KEY` | `fireworks` | Yes if used |
| `OPENROUTER_API_KEY` | `openrouter` | Yes if used |
| `DEEPSEEK_API_KEY` | `deepseek` | Yes if used |
| `MISTRAL_API_KEY` | `mistral` | Yes if used |
| `XAI_API_KEY` | `xai` | Yes if used |
| `COHERE_API_KEY` | `cohere` | Yes if used |
| `PERPLEXITY_API_KEY` | `perplexity` | Yes if used |
| `CEREBRAS_API_KEY` | `cerebras` | Yes if used |

AWS Bedrock does not use an API key — it uses the AWS credentials chain
(`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, or an
instance role) plus the `aws_region` field in `ModelConfig`. Ollama and any
OpenAI-compatible local provider accept an empty `api_key`.

The **[CLI provider](../glossary.md#cli-provider)** cases are different.
`claude-code` delegates to the local `claude` binary and reads the user's
existing subscription credentials from the CLI's own keychain — no
`ANTHROPIC_API_KEY` is needed. `copilot` delegates to `gh copilot`, which
uses the `GH_TOKEN` that `gh auth login` established. See
[../adr/004-cli-provider-pattern.md](../adr/004-cli-provider-pattern.md).

## Integration credentials

These are long-lived credentials that live in `[google]`, `[notion]`,
`[jira]`, `[linear]`, or the channel sections. Channels and integrations
may alternatively use the one-click OAuth flow instead of a bearer key — the
one-click flow writes its tokens into `integrations.db` (see
[backup-and-restore.md](backup-and-restore.md)) and no environment
variables are involved at runtime.

| Variable | Consumed by | Required |
|---|---|---|
| `NOTION_API_KEY` | `[notion].api_key` | Optional |
| `JIRA_API_TOKEN` | `[jira].api_token` | Optional |
| `LINEAR_API_KEY` | `[linear].api_key` | Optional |
| `GITHUB_TOKEN` | Built-in GitHub tools and the `github` integration | Optional |
| `TELEGRAM_BOT_TOKEN` | `[channels.telegram].bot_token` | Optional |
| `DISCORD_BOT_TOKEN` | `[channels.discord].bot_token` | Optional |
| `SLACK_BOT_TOKEN` | `[channels.slack].bot_token` | Optional |
| `SLACK_APP_TOKEN` | `[channels.slack].app_token` | Optional |
| `WHATSAPP_ACCESS_TOKEN` | `[channels.whatsapp].access_token` | Optional |

## One-click OAuth app secrets

The `[integrations.*]` sections (see
[configuration.md](configuration.md#integrations)) register OAuth app client
IDs and secrets for the one-click browser flow exposed by the gateway. The
pattern is `${PROVIDER}_OAUTH_CLIENT_ID` and `${PROVIDER}_OAUTH_CLIENT_SECRET`,
expanded from the TOML like any other string field.

| Provider | Expected variables |
|---|---|
| Gmail / Google | `GOOGLE_OAUTH_CLIENT_ID`, `GOOGLE_OAUTH_CLIENT_SECRET` |
| Slack | `SLACK_OAUTH_CLIENT_ID`, `SLACK_OAUTH_CLIENT_SECRET` |
| GitHub | `GITHUB_OAUTH_CLIENT_ID`, `GITHUB_OAUTH_CLIENT_SECRET` |
| Jira | `JIRA_OAUTH_CLIENT_ID`, `JIRA_OAUTH_CLIENT_SECRET` |
| Linear | `LINEAR_OAUTH_CLIENT_ID`, `LINEAR_OAUTH_CLIENT_SECRET` |
| Notion | `NOTION_OAUTH_CLIENT_ID`, `NOTION_OAUTH_CLIENT_SECRET` |

These variables are only meaningful if the corresponding
`[integrations.<provider>]` subsection exists in the config and references
them. The variable names above are a convention, not a hard requirement —
any string name works, because the expansion point is the TOML field.

## Search

| Variable | Consumed by | Required |
|---|---|---|
| `BRAVE_API_KEY` | `[web_search].api_key` when `provider = "brave"` | Optional |
| `TAVILY_API_KEY` | `[web_search].api_key` when `provider = "tavily"` | Optional |

Without either, the `web_search` tool is not registered and falls back to
failing closed with a missing-key error.

## Example configs

Minimal Anthropic-only setup, keys from the environment:

```toml
[model]
provider = "anthropic"
model_id = "claude-sonnet-4-20250514"
api_key = "${ANTHROPIC_API_KEY}"
```

Anthropic with OpenAI fallback and a Telegram channel, all keys from the
environment:

```toml
[model]
provider = "anthropic"
model_id = "claude-sonnet-4-20250514"
api_key = "${ANTHROPIC_API_KEY}"

[[fallback_models]]
provider = "openai"
model_id = "gpt-4o"
api_key = "${OPENAI_API_KEY}"

[channels.telegram]
bot_token = "${TELEGRAM_BOT_TOKEN}"
allowed_users = [123456789]

[gateway]
bind = "0.0.0.0:18789"

[[gateway.api_keys]]
name = "cli"
key = "${RYVOS_GATEWAY_TOKEN}"
role = "admin"
```

Run with:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
export OPENAI_API_KEY=sk-...
export TELEGRAM_BOT_TOKEN=123456:ABC
export RYVOS_GATEWAY_TOKEN=rk_live_abc123
export RUST_LOG=ryvos=info,warn
ryvos daemon --gateway
```

## Shell profile and service files

For a long-lived install, set these in one of:

- `~/.profile` or `~/.zshenv` for REPL and manual runs.
- The systemd unit's `Environment=` directives — one per variable — for the
  user service.
- The launchd plist's `EnvironmentVariables` dict for macOS.
- The Docker/compose `environment:` block or Fly.io `fly secrets set` for
  container deployments.

`ryvos init --yes` writes `Environment=RUST_LOG=ryvos=info` into the
systemd unit and the equivalent entry into the launchd plist. Any other
variable has to be added manually; the wizard does not currently edit
variable entries on existing unit files.

Cross-references:
[configuration.md](configuration.md),
[deployment.md](deployment.md),
[troubleshooting.md](troubleshooting.md).
