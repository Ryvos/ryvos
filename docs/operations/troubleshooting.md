# Troubleshooting

This document is a diagnostic runbook. It covers the common ways a Ryvos
daemon fails to start, fails to respond, or fails mid-run, and points at
the specific command, config field, or database that resolves each case.
The structure is "symptom, cause, fix" — use the table of contents to
jump to the failure mode you're seeing.

The first tool to reach for is always `ryvos doctor`. It runs seven
independent checks against the live config and workspace, and its output
is the fastest way to narrow a vague "nothing works" down to a specific
subsystem.

## First: `ryvos doctor`

`src/doctor.rs` runs these seven checks in order and prints a one-line
summary for each:

| Check | What it verifies |
|---|---|
| API Key | The `[model].api_key` field is non-empty and not an unresolved `${VAR}`. Skipped for `ollama`. |
| Workspace | The workspace directory exists, is a directory, and is writable. |
| Database | `sessions.db` opens successfully. |
| Channels | Every configured channel has a non-empty bot token and (for Slack) a non-empty app token. |
| Cron | Every `[[cron.jobs]].schedule` parses as a cron expression. |
| MCP | Reports how many MCP servers are configured and how many auto-connect. Never fails. |
| Security | The deprecated tier knobs are internally consistent. |

A clean run ends with "7 passed, 0 issues found". A single failing check
indicates the exact subsystem at fault. `ryvos doctor` does not make any
network calls — it is safe to run offline and fast to run repeatedly.

## Agent will not start

The daemon exits immediately or fails before the first log line. Walk the
list in order.

### Config syntax error

Run `ryvos config`. It parses the config and prints a normalized TOML
dump. A parse error shows up as a clap error with the line and column of
the failure. The most common cause is an unclosed table header or a
missing quote around a string value with special characters. TOML's error
messages point at the correct line in the expanded (post-env-var) source,
so an error in a `${VAR}` substitution may surface as a line-offset
mismatch against the raw file.

### Missing API key

`ryvos doctor` flags an empty `api_key`. Three common causes:

- The `${VAR}` reference points at an environment variable that is not
  set in the daemon's environment. The REPL sees shell environment; the
  daemon sees only the service unit's `Environment=` entries. Fix by
  adding `Environment=ANTHROPIC_API_KEY=sk-...` to the systemd unit or
  the equivalent block in the launchd plist.
- The variable name is misspelled. `expand_env_vars` leaves unresolved
  references verbatim, so the field ends up as the literal string
  `${WRONG_NAME}` — `ryvos doctor` matches on the `${` prefix.
- The provider is `ollama` (which needs no key) but the config still
  expects one. Set `api_key = ""` or remove the field.

### Workspace not writable

`ryvos doctor` shows "Workspace: ... (not writable: ...)". The daemon's
service user does not have write access to `~/.ryvos`. Fix:

```bash
chown -R "$USER:$USER" ~/.ryvos
chmod -R u+rwX ~/.ryvos
```

On Docker the workspace is `/data` and must be a writable volume. The
image runs as the non-root `ryvos` user (uid matches the Dockerfile);
bind-mounted host directories may need `chown` on the host side to match.

### Database corruption

`ryvos doctor` shows "Database: ... error". Check integrity directly:

```bash
sqlite3 ~/.ryvos/sessions.db "PRAGMA integrity_check;"
```

A healthy database reports `ok`. Anything else — "malformed",
"database disk image is malformed", "not a database" — indicates
corruption. See [backup-and-restore.md](backup-and-restore.md) for the
restore path. For an immediate workaround, move the broken file aside
(`mv sessions.db sessions.db.broken`) and let the daemon recreate it
empty on next start.

## LLM provider errors

### HTTP 429 (rate limit)

`RetryingClient` handles 429 by applying exponential backoff up to
`retry.max_backoff_ms` and, after `retry.max_retries`, falling through to
the next `[[fallback_models]]` entry. Three mitigations:

- Add a second provider as a fallback: a cheap OpenAI model behind an
  Anthropic primary, or the reverse.
- Lower the initial backoff (`initial_backoff_ms = 500`) so the next
  retry fires sooner if the provider's rate limit window is short.
- Check the dashboard of the upstream provider for the actual rate limit
  in effect; the error message may or may not quote it.

### HTTP 401 (authentication)

Rotate the API key. If the key is correct when tested with `curl`, the
daemon is seeing a different environment. Confirm with:

```bash
systemctl --user show-environment
# or: launchctl getenv ANTHROPIC_API_KEY
```

Service environments are isolated from the login shell; see
[environment-variables.md](environment-variables.md) for the per-launcher
rules.

### Timeout

The LLM stream hangs and the **[Guardian](../glossary.md#guardian)** fires
a `GuardianStall` event.
Causes:

- A very long output with `max_tokens` set higher than the provider will
  actually stream. Lower `max_tokens` or raise `[agent].max_duration_secs`.
- A provider that is silently dropping the connection mid-stream. Switch
  to a fallback model temporarily.
- A local network blackhole (WireGuard, corporate proxy). Test the
  endpoint with `curl -v` from the same host.

### Claude Code CLI not found

Error: `claude: not found`. The `claude-code` provider spawns the
`claude` binary. Fix by setting `claude_command = "/full/path/to/claude"`
in `[model]`, or adding the directory containing `claude` to the
daemon's `PATH`. systemd user units inherit a minimal `PATH`; set it
explicitly:

```text
Environment=PATH=/home/user/.local/bin:/usr/local/bin:/usr/bin
```

### Copilot CLI not authenticated

The `gh copilot` subprocess returns an auth error. Run `gh auth login`
as the daemon's service user, not as the login shell user. On a
single-user system these are the same; on a multi-user host they may
differ.

## Tool execution errors

### Tool timeout

A single tool takes longer than its `timeout_secs`. Tool timeouts come
from the `Tool::timeout_secs()` trait method; overriding them from
config is deprecated. If a specific MCP tool consistently times out,
raise the server's `timeout_secs` in `[mcp.servers.<name>]`.

### Docker sandbox unreachable

`bollard` cannot reach the Docker daemon. Check:

```bash
docker ps
groups $USER   # expect "docker" for rootless socket access
```

If `docker ps` works as the service user but Ryvos gets "permission
denied", the service unit is running without the `docker` group. Add
`SupplementaryGroups=docker` to the unit's `[Service]` section (systemd
user units inherit primary group only by default).

### Permission denied on the filesystem

The `write_file` or `bash` tool gets EACCES. Usually the workspace or
the target directory is owned by a different user, or the daemon is
running with `ProtectHome=` enabled in a hardened unit file. The
installer-generated unit does not set any hardening directives; check
for local customizations.

### Browser tools fail to launch

`browser_navigate` and friends need Chrome or Chromium on disk.
Install it and either accept the default path discovery or set
`CHROME_PATH=/usr/bin/chromium` in the daemon environment.

## Channel errors

### Telegram: invalid bot token

The daemon logs "Telegram API error: Unauthorized". The bot token is
wrong or the bot has been revoked. Create a new bot with `@BotFather`
and update `[channels.telegram].bot_token`.

### Discord: missing intent

The adapter connects but never receives messages. Discord requires the
**Message Content Intent** to be enabled on the bot's application page.
Without it, `message.content` is empty on every inbound event. The
adapter treats empty content as no input and silently drops the event.

### Discord: bot not in guild

The bot token is valid but the bot has not been invited to any guild.
Invite via the OAuth2 URL generator with the `bot` scope.

### Slack: socket mode failures

Slack requires two tokens (`bot_token` starting `xoxb-`, `app_token`
starting `xapp-`) and Socket Mode enabled on the app. A socket-mode
connection failure usually means one of:

- The app does not have Socket Mode enabled. Enable it under
  App Home → Socket Mode.
- The `app_token` is missing the `connections:write` scope.
- The bot is not installed into the workspace. Install from the
  OAuth & Permissions page.

### WhatsApp: verify token mismatch

The Meta Business webhook returns a 403 on the handshake. The
`verify_token` in `[channels.whatsapp]` must match the value entered
in the Meta Business webhook configuration exactly — no whitespace,
same case.

## Gateway errors

### Port already in use

`bind: address already in use` on start. Another process is bound to
`18789`. Either change `[gateway].bind` or kill the other process:

```bash
ss -tlnp 'sport = :18789'
```

### 401 Unauthorized

The gateway returns 401 for a request that has a token. Walk the auth
precedence chain in [../api/auth-and-rbac.md](../api/auth-and-rbac.md):
bearer header, then `?token=`, then `?password=`, then anonymous. The
most common cause is a mismatch between the Authorization header and
the configured `[[gateway.api_keys]].key`. Check with:

```bash
curl -H "Authorization: Bearer $KEY" http://localhost:18789/api/health
```

### CORS blocked

The browser console shows a CORS error. The gateway ships a permissive
CORS layer (`tower-http::cors::CorsLayer::very_permissive()`); any CORS
error likely comes from an upstream reverse proxy stripping the
`Access-Control-Allow-Origin` header. Check the proxy configuration —
Caddy and nginx both need explicit instructions to preserve the header.

## Guardian stalls

The Guardian fires `GuardianStall` more often than expected. Three
mitigations:

- Raise `[agent.guardian].stall_timeout_secs`. The default 120 is tuned
  for interactive chat; batch workloads benefit from 300+.
- Inspect the tool that stalled — the event payload names the tool. A
  tool that consistently runs long should expose progress events, or be
  replaced with a bounded version.
- Check the failure journal:

```bash
ryvos health --days 7
```

Tools with a high failure rate are the usual culprits.

## Heartbeat loops failing

**[Heartbeat](../glossary.md#heartbeat)** events are not firing, or they
always return `HeartbeatAlert`. Check:

- `HEARTBEAT.md` exists in the workspace. Since v0.8.1 it is
  auto-created on first fire; pre-v0.8.1 installs need to create it
  manually.
- The active-hours window includes the current local time. The
  `[heartbeat.active_hours].utc_offset_hours` field is a simple integer
  offset; daylight saving transitions are not handled.
- The **[audit trail](../glossary.md#audit-trail)** for the heartbeat
  session shows the agent reaching the end of its loop without errors.
  Query:

```sql
SELECT timestamp, tool_name, outcome
FROM audit_log
WHERE session_id LIKE 'heartbeat-%'
ORDER BY timestamp DESC LIMIT 20;
```

## Memory and context issues

### Context full before compaction

The agent hits the context ceiling and fails before
`enable_summarization` kicks in. Causes:

- `max_context_tokens` is set too low for the model. Raise it.
- The LLM provider is counting tokens differently than Ryvos's estimator.
  Lower `max_context_tokens` by ten percent as a safety margin.
- A single tool output exceeded `max_tool_output_tokens`. The output is
  truncated but the truncated version still takes space; lower
  `max_tool_output_tokens` or narrow the tool's arguments.

### Viking server unreachable

The daemon logs "OpenViking unreachable". The
**[Viking](../glossary.md#viking)** server is either
not running or not reachable at `[openviking].base_url`. The daemon
falls back to running without sustained context, so the failure is
non-fatal but the agent loses hierarchical memory. Fix:

```bash
ryvos viking-server --bind 127.0.0.1:1933 &
```

In daemon mode with `[openviking].base_url` pointing at localhost,
Ryvos auto-spawns the Viking server as a sibling task; the error
typically means the auto-spawn raced the first read. Restart the
daemon.

## Failed Director runs

Goal-driven runs escalate without reaching `Accept`. Open the Web UI
Goals page or query the audit trail for the run — the
**[Director](../glossary.md#director)** publishes structured events
there — and look for
`semantic_failure` events. Two common patterns:

- The goal's success criteria are too tight. An `OutputContains` on a
  specific string fails when the model paraphrases. Loosen to
  `LlmJudge` for a more forgiving check.
- The Director hits `max_evolution_cycles` (default 3) without making
  progress. Either raise the limit or break the goal into smaller
  subgoals routed through separate runs.

## Database corruption

Any `PRAGMA integrity_check` failure. The recovery path is documented
in [backup-and-restore.md](backup-and-restore.md). The short version:

```bash
sqlite3 broken.db ".recover" | sqlite3 recovered.db
sqlite3 recovered.db "VACUUM INTO 'clean.db';"
mv broken.db broken.db.original
mv clean.db broken.db
```

Failure isolation (see
[../architecture/persistence.md](../architecture/persistence.md#failure-isolation-in-practice))
means a single corrupt database does not block the others. The daemon
starts, logs "Failed to initialize X store", and proceeds without that
subsystem.

## Debug logging

The default filter is `ryvos=info,warn`. Two useful escalations:

```bash
RUST_LOG=ryvos=debug ryvos daemon --gateway
RUST_LOG=ryvos_agent=trace,ryvos_llm=debug,ryvos=info ryvos daemon --gateway
```

`trace` on `ryvos_agent` is loud but captures every agent-loop state
transition, which is the right tool for "the agent is doing something
unexpected". For LLM request content, `ryvos_llm=trace` logs the full
request bodies; redact before sharing in a bug report.

## Reporting bugs

A good bug report contains:

- `ryvos --version`.
- Redacted `config.toml` — API keys and bot tokens replaced with
  `<redacted>`.
- `ryvos doctor` output.
- The tail of `journalctl --user -u ryvos -n 500` or
  `~/.ryvos/daemon.log`.
- The exact prompt or action that reproduces the issue.

File at `https://github.com/Ryvos/ryvos/issues`.

Cross-references:
[../guides/debugging-runs.md](../guides/debugging-runs.md),
[monitoring.md](monitoring.md),
[backup-and-restore.md](backup-and-restore.md).
