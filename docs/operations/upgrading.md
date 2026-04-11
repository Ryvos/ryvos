# Upgrading

Ryvos upgrades are intentionally simple. The runtime is a single binary,
the schema is additive, and the config format is forward-compatible. An
upgrade is "stop the daemon, replace the binary, start the daemon". This
document covers the general flow plus the per-version notes that a careful
operator should read before jumping multiple minor releases.

The canonical release list lives in [/CHANGELOG.md](../../CHANGELOG.md)
at the repo root. When the notes below and the changelog disagree, the
changelog wins.

## General upgrade flow

The recommended sequence for a self-hosted install:

```bash
# 1. Snapshot — see backup-and-restore.md
systemctl --user stop ryvos.service
tar czf ~/ryvos-pre-upgrade-$(date +%Y%m%d).tar.gz -C "$HOME" .ryvos/

# 2. Self-update
ryvos update --yes

# 3. Restart
systemctl --user start ryvos.service
journalctl --user -u ryvos -n 200 --no-pager
```

`ryvos update` checks the GitHub releases API, detects the correct
artifact for the running platform, downloads it, renames the current
binary to `.bak`, atomic-renames the download into place, and cleans up
the backup on success. On any mid-rename failure the backup is restored,
so a failed update never leaves a half-installed binary. The logic lives
in `src/main.rs:2133`.

Running daemons keep using the binary image loaded into their process
until explicitly restarted — a successful `ryvos update` does not
hot-swap the daemon. Restart the service after updating.

## Schema and data migrations

Every SQLite store in Ryvos opens with an idempotent `CREATE TABLE IF NOT
EXISTS` plus `CREATE INDEX IF NOT EXISTS` sequence; columns that have
been added since the store's last release are added with `ALTER TABLE ADD
COLUMN ... DEFAULT NULL`. The practical consequence is that every store
is forward-compatible across minor releases without an explicit migration
step.

At v0.8.3 there is no versioned migration framework. The CREATE/ALTER
pattern is described in
[../architecture/persistence.md](../architecture/persistence.md#migration-strategy).
When a destructive schema change becomes necessary, it will be a one-shot
rebuild function in the owning store, gated on the presence or absence of
the new schema shape — not a Diesel-style migrations directory.

Data formats are equally forward-compatible:

- The checkpoint store uses delete-and-insert on upsert, so a schema
  change to the `checkpoints` table drops nothing.
- `run_log` rows are never edited after completion; new columns default to
  NULL on upgrade.
- The audit trail is append-only; schema changes never touch existing
  rows.

The only format that has ever broken backwards compatibility is the
embeddings blob shape. v0.5.0 introduced the little-endian f32 layout
used today; pre-v0.5.0 embeddings predate Ryvos as a public project.

## Config compatibility

The config file is read by `toml::from_str` with `#[serde(default)]` on
every field. Unknown fields are ignored; missing fields take their
defaults. Removing a field from the source code does not break old config
files, and adding a field to the source code does not break old config
files either — the old config simply picks up the new default.

Deprecated fields retained for compatibility at v0.8.3:

| Field | Since | Status |
|---|---|---|
| `[security].auto_approve_up_to` | v0.6.0 | Read, not used. Safe to remove. |
| `[security].deny_above` | v0.6.0 | Read, not used. Safe to remove. |
| `[security].dangerous_patterns` | v0.6.0 | Read, passed to CLI providers for informational logging, not used to gate. |
| `[security].sub_agent_policy` | v0.6.0 | Read, not used. |
| `[security].tool_overrides` | v0.6.0 | Read, not used. |
| `[gateway].token` | v0.7.0 | Still functional but prefer `[[gateway.api_keys]]`. |
| `[gateway].password` | v0.7.0 | Still functional but prefer `[[gateway.api_keys]]`. |

A future minor release may drop the `[security]` tier knobs entirely. The
migration is trivial — delete them and add `pause_before` if a soft
checkpoint is wanted. See
[../guides/migrating-from-tier-security.md](../guides/migrating-from-tier-security.md)
for the step-by-step.

## Version-specific notes

The entries below cover every release from v0.6.0 onwards. Older releases
predate the current architecture and upgrading directly to v0.8.3 is
recommended instead of stepping through each intermediate version.

### v0.6.0 — Security overhaul

The cutover from tier-based blocking to passthrough security. Pre-v0.6
configs keyed policy on `auto_approve_up_to` and `deny_above`, which this
release deprecates in favor of constitutional AI plus optional
`pause_before` checkpoints. No config change is strictly required — the
old fields still parse — but a fresh `ryvos init` produces a cleaner
result. Details in ADR-002.

Viking was also introduced in this release. Existing installs gain a
`viking.db` file on first daemon start with `[openviking]` enabled; the
file is created empty and populates as memories are written.

### v0.6.5 — Viking client race condition fix

A race in `VikingClient::health` could leave the client marked connected
after the server had dropped. Fix is in the daemon. No config change.
Restart the daemon to pick up.

### v0.6.11 — Web UI overhaul

Ships a full Svelte 5 rewrite of the Web UI. The UI is embedded into the
binary via `rust_embed`; there is no separate deploy step. If a custom
reverse proxy was rewriting `/assets/*` paths, check that the new UI
loads cleanly after the upgrade.

### v0.7.0 — Dormant systems activated

**[Director](../glossary.md#director)**,
**[Guardian](../glossary.md#guardian)**, and `CostStore` now always
activate on daemon start,
regardless of config. In pre-v0.7 installs these subsystems required
explicit configuration to run; they are now the default. A config from
v0.6.x will pick up the new behavior on first restart.

The `[budget]` section no longer gates `CostStore` creation. The store is
created unconditionally and writes every run regardless of whether a
dollar budget is set. Cost data collected after the upgrade populates
`/api/metrics` even for installs that previously had no `[budget]`
section.

### v0.7.1 — UTF-8 panic fix

Critical. Pre-v0.7.1 builds could panic when truncating strings at byte
offsets that split a UTF-8 code point (emoji in tool output was the
typical trigger). The bug silently killed heartbeat cycles for up to ten
hours per incident. Upgrade immediately on any install that sees emoji
in tool outputs or channel messages.

### v0.7.2 — Native OAuth integrations

Adds the `[integrations]` config section and the `integrations.db`
store. Pre-v0.7.2 installs have no `integrations.db` file; the new
binary creates it on first daemon start. Existing bearer-key integrations
in `[google]`, `[notion]`, `[jira]`, and `[linear]` continue to work
unchanged. The one-click OAuth flow in the Web UI only appears for
providers that have an `[integrations.<provider>]` entry with
`client_id` and `client_secret` set.

This release also fixes cron schedule timezone handling. Pre-v0.7.2
schedules were interpreted in UTC even when a local-time cron expression
was expected. Double-check cron job fire times after the upgrade.

### v0.8.0 — Constitutional AI pipeline fully wired

**[SafetyMemory](../glossary.md#safetymemory)**,
**[Reflexion](../glossary.md#reflexion)**, and the corrective-rule
injection are now
active in production. No action required — the behavior was already
present in earlier v0.7.x builds, this release removes the last
remaining feature flags. Safety lessons accumulate in `safety.db` and are
injected into the narrative context layer on subsequent runs.

### v0.8.1 — Heartbeat auto-bootstrap

`HEARTBEAT.md` is now auto-created on `ryvos init` and on first
heartbeat fire. Pre-v0.8.1 installs with no `HEARTBEAT.md` silently
used an empty prompt; v0.8.1 detects the missing file and writes a
default template. No action required — an existing file is respected.

### v0.8.2 — Director OODA in production

Cron jobs with a `goal` field now route through the Director instead of
the plain ReAct loop. Check any `[[cron.jobs]]` entry that has a `goal`
set; the behavior changes from "run until max_turns" to "run until the
**[Judge](../glossary.md#judge)** accepts or the Director escalates". This is the intended upgrade
but can surprise an operator who was relying on the old shape.

The Web UI gains a Goals page for creating, tracking, and evaluating
goals. No config change required.

### v0.8.3 — Quality sprint

373 tests, ten ADRs, benchmarks, and no breaking changes. The upgrade is
binary-swap only. New documents appear under `docs/` and `docs/adr/` but
do not affect runtime behavior.

## Rolling back

A clean rollback needs the pre-upgrade backup from step 1 of the general
flow plus a copy of the previous binary. The update process keeps a
`.bak` copy next to the installed binary for the duration of the update
transaction; after a successful swap the `.bak` is removed. To keep a
manual rollback copy, either download the previous release's artifact
from GitHub and keep it on disk, or save the binary before running
`ryvos update`:

```bash
cp /usr/local/bin/ryvos /usr/local/bin/ryvos.v0.8.2
ryvos update --yes
# ...if something is wrong:
systemctl --user stop ryvos.service
cp /usr/local/bin/ryvos.v0.8.2 /usr/local/bin/ryvos
tar xzf ~/ryvos-pre-upgrade-20260411.tar.gz -C "$HOME"
systemctl --user start ryvos.service
```

Rolling back across a schema change is the exception to the "no
migration" rule — a v0.8.3 binary that added a column to `audit.db` via
`ALTER TABLE ADD COLUMN` still opens on v0.8.2 because SQLite ignores
unknown columns on SELECT, but v0.8.2 cannot write rows that reference
the new column. In practice this has never mattered because every column
added so far has been informational.

## Container and cloud upgrades

For Docker and Fly.io the upgrade is a new image tag. Pull the new tag,
recreate the container, keep the workspace volume attached. The
HEALTHCHECK on `/api/health` validates the new instance before marking
it healthy, and Fly.io's default rolling strategy completes the swap
without downtime for read traffic (long-running agent runs on the old
instance are interrupted by SIGTERM and resume from checkpoint on the
new instance).

Cross-references:
[/CHANGELOG.md](../../CHANGELOG.md),
[backup-and-restore.md](backup-and-restore.md),
[configuration.md](configuration.md),
[../guides/migrating-from-tier-security.md](../guides/migrating-from-tier-security.md).
