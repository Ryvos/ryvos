# Backup and restore

Ryvos persists state in seven independent SQLite databases under the
workspace directory, plus a handful of markdown files that together form
the **[onion context](../glossary.md#onion-context)**. A backup is the union
of those files — no external services, no external state, no cross-database
joins. The split design is documented in
[../architecture/persistence.md](../architecture/persistence.md); this doc
is the operator's how-to for getting a consistent snapshot and restoring it
on a new host.

## What to back up

Everything that matters lives under `~/.ryvos/` (the default workspace).
The complete list:

| Path | Owner | Importance |
|---|---|---|
| `~/.ryvos/config.toml` | User | Critical — every secret and integration setting. |
| `~/.ryvos/SOUL.md` | `ryvos soul` | Personality and operator context. |
| `~/.ryvos/IDENTITY.md` | User | Agent identity header. |
| `~/.ryvos/AGENTS.toml` | User | Repo-local agent profile (often absent). |
| `~/.ryvos/TOOLS.md` | User | Tool usage conventions. |
| `~/.ryvos/USER.md` | User | Operator preferences. |
| `~/.ryvos/BOOT.md` | User | One-time boot instructions. |
| `~/.ryvos/HEARTBEAT.md` | Auto-created v0.8.1+ | Heartbeat prompt. |
| `~/.ryvos/MEMORY.md` | User | High-level memory index. |
| `~/.ryvos/memory/*.md` | `daily_log_write` | Daily logs; retention-pruned. |
| `~/.ryvos/skills/` | `ryvos skill install` | TOML/Lua/Rhai skill packages. |
| `~/.ryvos/logs/runs.jsonl` | `RunLogger` | JSONL run log; append-only. |
| `~/.ryvos/sessions.db` | `SqliteStore` | Conversation history, FTS, embeddings, session meta. |
| `~/.ryvos/viking.db` | `VikingStore` | Hierarchical Viking memory. |
| `~/.ryvos/cost.db` | `CostStore` | Cost events and run log. |
| `~/.ryvos/healing.db` | `FailureJournal` | Failure journal, success journal, decision journal. |
| `~/.ryvos/safety.db` | `SafetyMemory` | Constitutional self-learning lessons. |
| `~/.ryvos/audit.db` | `AuditTrail` | Append-only tool invocation trail. |
| `~/.ryvos/integrations.db` | `IntegrationStore` | **Encrypted** OAuth tokens. |

The seven `.db` files each have two SQLite WAL siblings (`<name>.db-wal` and
`<name>.db-shm`). A backup that copies only the `.db` file misses any
committed rows that WAL has not yet checkpointed into the main file. There
are two ways to handle this: checkpoint before copy, or copy all three
files.

## Cold backup (daemon stopped)

The simplest approach. Stop the daemon, archive the workspace, restart. WAL
does not matter because SQLite checkpoints cleanly on clean shutdown.

```bash
systemctl --user stop ryvos.service
# or: launchctl unload ~/Library/LaunchAgents/com.ryvos.agent.plist

tar czf ryvos-backup-$(date +%Y%m%d).tar.gz \
  -C "$HOME" .ryvos/

systemctl --user start ryvos.service
```

The archive is a complete, consistent snapshot. It can be restored on any
host with a compatible architecture by unpacking it into `$HOME` and
starting the daemon. Ryvos runs the `CREATE TABLE IF NOT EXISTS` path on
every open, so even a slightly newer binary starting against an older
schema works — see [upgrading.md](upgrading.md) for the exceptions.

## Hot backup (daemon running)

For a zero-downtime backup, force a WAL checkpoint on each database before
copying. The `sqlite3` CLI can do this against a running writer because
WAL mode allows a reader to open the database in parallel. Example for one
database:

```bash
sqlite3 ~/.ryvos/cost.db "PRAGMA wal_checkpoint(TRUNCATE);"
cp ~/.ryvos/cost.db /backup/cost.db
```

The checkpoint folds every committed WAL entry into the main `.db` file
and truncates the log to zero length. Subsequent writes from the running
daemon go into a fresh WAL, so the copy taken immediately after the
checkpoint is a point-in-time snapshot of every row committed before the
checkpoint call. Concurrent writes arriving mid-copy are not visible in
the backup — the SQLite OS-page copy sees the checkpoint-consistent
database pages and the fresh WAL stays on disk.

Full loop:

```bash
BACKUP_DIR=/backup/ryvos-$(date +%Y%m%d-%H%M%S)
mkdir -p "$BACKUP_DIR"

for db in sessions.db viking.db cost.db healing.db safety.db audit.db integrations.db; do
  sqlite3 "$HOME/.ryvos/$db" "PRAGMA wal_checkpoint(TRUNCATE);"
  cp "$HOME/.ryvos/$db" "$BACKUP_DIR/$db"
done

rsync -a --exclude='*.db' --exclude='*.db-wal' --exclude='*.db-shm' \
  "$HOME/.ryvos/" "$BACKUP_DIR/"

tar czf "$BACKUP_DIR.tar.gz" -C "$(dirname "$BACKUP_DIR")" "$(basename "$BACKUP_DIR")"
rm -rf "$BACKUP_DIR"
```

Because there are no cross-database transactions (see
[../architecture/persistence.md](../architecture/persistence.md#concurrency-across-stores)),
a snapshot taken under load may differ by at most one `run_log` row across
`audit.db` and `cost.db` — the agent published `ToolEnd`, the audit writer
committed, and the cost writer was mid-commit. Reconciliation is by
`run_id` on restore and is rarely needed in practice.

The alternative hot-backup shape is the SQLite Online Backup API via the
`.backup` command:

```bash
sqlite3 ~/.ryvos/cost.db ".backup /backup/cost.db"
```

This uses the same underlying primitive as `PRAGMA wal_checkpoint` but
interleaves its pages with writer activity for lower write-latency impact.
Either form is safe.

## Restore

Stop the daemon, unpack the archive, restart. No schema migration step is
needed — the stores all upgrade their schemas on open with idempotent
`CREATE TABLE IF NOT EXISTS` plus `ALTER TABLE ADD COLUMN` calls. Nothing
in Ryvos is version-pinned against the SQLite schema.

```bash
systemctl --user stop ryvos.service

rm -rf ~/.ryvos-old
mv ~/.ryvos ~/.ryvos-old
tar xzf ryvos-backup-20260411.tar.gz -C "$HOME"

systemctl --user start ryvos.service
journalctl --user -u ryvos -n 100 --no-pager
```

The `mv` instead of `rm` keeps a recovery path if the backup is
corrupted — swap the directory back and retry. Once the restored daemon
has run for an hour without incident, `rm -rf ~/.ryvos-old`.

## Encryption and the integrations database

`integrations.db` stores OAuth tokens for Gmail, Slack, GitHub, Jira,
Linear, and Notion. Rows are encrypted at rest using AES-256-GCM with a key
derived from an environment variable. Losing the key makes the backup
unreadable — the `.db` file copies, but the row bodies decrypt to noise.

Consequences for backup strategy:

- Back up the key separately from the database, not alongside it. A secret
  manager (AWS KMS, 1Password vault, age file) is the correct shape.
- If a backup is restored on a new host without the key, Ryvos starts but
  every OAuth-based integration returns "token unreadable" and requires
  re-authorization through the Web UI.
- Rotating the key requires a full re-auth pass across every integration.
  There is no in-place re-encryption command at v0.8.3.

Everything else in the workspace is plaintext. Local LLM API keys are
plaintext in `config.toml`, daily logs are plaintext markdown, the audit
trail stores tool inputs and outputs verbatim, and the session store holds
raw conversation history. Treat the whole `~/.ryvos/` directory as
sensitive material and encrypt the backup archive at rest (`gpg`, `age`,
or the filesystem layer).

## Rotation

A reasonable baseline for a personal daemon:

| Cadence | Type | Retention |
|---|---|---|
| Every hour | Hot backup (WAL checkpoint + cp) | Last 24 |
| Every day | Hot backup + markdown rsync | Last 7 |
| Every week | Cold backup (full tar.gz) | Last 4 |

For a shared or production install, add an offsite copy — restic or
rclone against S3 or B2 both work against the archive output. The backup
window is always dominated by `sessions.db` and `audit.db`; everything
else is typically under 10 MB.

## Verification

Backups that are never restored are not backups. Run a periodic verify:

```bash
sqlite3 /backup/ryvos-20260411/cost.db "PRAGMA integrity_check;"
sqlite3 /backup/ryvos-20260411/audit.db "PRAGMA integrity_check;"
# repeat for each .db file
```

`PRAGMA integrity_check` reports "ok" on a healthy file and a list of
corruption details otherwise. A corrupt backup usually means a concurrent
writer missed the checkpoint — re-run the backup with the daemon stopped
to get a known-good snapshot.

For a higher-fidelity verify, spin up a disposable Ryvos instance against
the backup and let it boot. The daemon will log which stores opened
successfully; any `Failed to initialize X store` error in the logs is a
corruption signal for that specific database.

## Selective reset

The seven-database split makes it cheap to reset one subsystem without
touching the others. Common cases:

- **Clear conversation history.** Stop the daemon, delete `sessions.db*`,
  restart. The next turn starts with an empty FTS index and fresh session
  metadata.
- **Clear costs.** Stop the daemon, delete `cost.db*`, restart. Monthly
  budget enforcement resets to zero.
- **Clear [Viking](../glossary.md#viking) memory.** Stop the daemon, delete `viking.db*`, restart.
  All `viking://` entries are gone; `MEMORY.md` and daily logs are
  untouched.
- **Clear safety lessons.** Delete `safety.db*`. Constitutional AI
  continues to work but starts with no learned corrective rules.

The audit trail is the only store where deletion is strongly
discouraged — it is the post-incident analysis substrate for
[../internals/safety-memory.md](../internals/safety-memory.md) and the
reference data for `ryvos health`.

Cross-references:
[../architecture/persistence.md](../architecture/persistence.md),
[../crates/ryvos-memory.md](../crates/ryvos-memory.md),
[upgrading.md](upgrading.md).
