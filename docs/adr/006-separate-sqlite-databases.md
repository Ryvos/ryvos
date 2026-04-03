# ADR-006: Separate SQLite Databases per Subsystem

## Status

Accepted

## Context

Ryvos persists a lot of state. Audit logs, session history, cost tracking,
self-healing records, Viking memory entries, safety lessons, and integration
configurations all need to survive restarts. The question is how to organize
that storage.

The typical approach is one database with many tables. That gives you
cross-table joins, single transaction boundaries, and one file to back up.
But it also means schema migrations affect everything, a corrupt index in one
table can block access to others, and the file grows without bound as different
subsystems accumulate data at different rates.

We considered three options:

1. **One big SQLite file.** Simple to manage, but schema conflicts between
   subsystems become a coordination problem. The audit log alone can grow to
   hundreds of megabytes while the safety lessons table stays tiny.
2. **Postgres or another server database.** Adds an external dependency. We
   would need to ship, configure, and manage a database server. Violates the
   single-binary goal.
3. **One SQLite file per subsystem.** Each subsystem owns its own database
   file. No coordination needed.

## Decision

Each subsystem gets its own SQLite database file in the Ryvos data directory
(typically `~/.ryvos/`):

- `audit.db` for the complete tool execution audit trail
- `sessions.db` for session metadata and conversation history
- `cost.db` for token usage and cost tracking
- `healing.db` for self-healing records and failure patterns
- `viking.db` for the Viking hierarchical memory system with FTS5
- `safety.db` for safety lessons and constitutional AI learning
- `integrations.db` for MCP server configs and integration state

Each database is opened independently by its owning subsystem using rusqlite.
Schema migrations run per-database at startup. Each subsystem defines its own
tables and indexes without worrying about name collisions.

The data directory structure looks like this:

```
~/.ryvos/
  audit.db
  sessions.db
  cost.db
  healing.db
  viking.db
  safety.db
  integrations.db
  config.toml
```

## Consequences

**What went well:**

- Zero coordination between subsystems. The Viking memory team (so to speak)
  can add columns, indexes, and FTS tables without touching anything else.
- Easy to reset individual stores. If the audit log gets too big, you can
  delete audit.db and the rest of the system is unaffected. Useful during
  development and for users who want to clear specific data.
- Failure isolation. A corrupt viking.db does not prevent the audit system
  from working. Each database fails independently.
- Backup granularity. Users can back up just their viking.db (their memory)
  without copying gigabytes of audit logs.
- SQLite handles concurrent reads well. Different subsystems can query their
  databases in parallel without lock contention, because they are literally
  different files with separate lock states.

**What is harder:**

- No cross-database joins. If you want to correlate audit entries with
  session data, you need to query both databases and join in application
  code. In practice, we rarely need this, but when we do, it is more work.
- No single transaction boundary. You cannot atomically write to audit.db
  and sessions.db in one transaction. If the process crashes between writes,
  the databases can be inconsistent with each other. We accept this because
  each database is self-consistent (SQLite's WAL mode ensures that) and
  cross-database consistency is not critical for our use case.
- More file handles. Seven open database connections instead of one. This is
  not a problem on any modern operating system, but it is worth noting.
- The data directory has many files. Some users expect a single database file
  and are surprised by the collection. Clear documentation helps.

This approach trades relational power for operational simplicity. For an
embedded agent that values resilience and modularity over complex queries,
it is the right call.
