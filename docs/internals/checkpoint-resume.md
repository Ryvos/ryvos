# Checkpoint and resume

Ryvos daemons run for a long time. They get restarted by operators, killed
by OOM, interrupted by power cuts, upgraded by systemd, and occasionally
crashed by a bug. When that happens, the user's ongoing conversations
should not vanish — the next message a Telegram user sends should land in
the same **[session](../glossary.md#session)** as the last one, and the
LLM should keep the context it was accumulating before the interruption.

Two independent mechanisms make that work. The first is turn-level
checkpointing: after every successful **[turn](../glossary.md#turn)**, the
agent loop persists the full message list and accumulated token counts to
a SQLite store so that a mid-run crash can be resumed without losing
intermediate state. The second is CLI session resumption: when the
underlying LLM provider is a **[CLI provider](../glossary.md#cli-provider)**
like `claude-code` or `copilot`, the vendor's own session id is persisted
across daemon restarts so the CLI can be told to `--resume` the same
conversation on its own side. The two mechanisms are complementary and
operate at different layers.

This document walks both: the `CheckpointStore` in
`crates/ryvos-agent/src/checkpoint.rs`, the `SessionMetaStore` in
`crates/ryvos-memory/src/session_meta.rs`, their call sites in the agent
loop and the channel dispatcher, and the CLI provider's `--resume` flag
handling. For the broader session routing story, see
[session-manager.md](session-manager.md). For persistence shapes in
general, see [../architecture/persistence.md](../architecture/persistence.md).

## Two resumption layers

The distinction matters because the two layers solve different problems
and can coexist or stand alone.

**Turn-level checkpointing** is internal to Ryvos. It captures the full
message history the agent loop has built for a single **[run](../glossary.md#run)**
at the granularity of one checkpoint per turn. If the daemon dies mid-run,
a clean restart can load the latest checkpoint, reinstate the message
list, and continue the loop from the next turn. Checkpointing works
regardless of the LLM provider, because the messages are Ryvos's own
`ChatMessage` values, not anything the provider knows about. It is
opt-in — the agent runtime only writes checkpoints if a
`CheckpointStore` has been attached via `set_checkpoint_store`.

**CLI session resumption** is external to Ryvos. It captures the *other
side's* opinion of the conversation — specifically, the session id that
`claude-code` or `copilot` assigns during `system.init`. If the daemon
restarts and the next user message arrives, the dispatcher fishes the
stored CLI session id out of `SessionMetaStore` and passes it to the
provider as `--resume <id>`. The provider then continues the same
conversation on its own side, with its own cached context window and its
own prompt history. This matters for **[subscription-billed](../glossary.md#subscription-billing)**
providers where the real conversation state lives inside Claude Code or
Copilot's own on-disk cache, not in the LLM API.

Both layers persist to SQLite. Turn checkpointing lives in its own
database (`checkpoints.db`, under the workspace); CLI session metadata
lives in `session_meta.db` under the workspace's `data/` directory. The
two stores are opened independently and have no foreign keys between
them.

## The Checkpoint record

`Checkpoint` at `crates/ryvos-agent/src/checkpoint.rs:11`:

```rust
pub struct Checkpoint {
    pub session_id: String,
    pub run_id: String,
    pub turn: usize,
    pub messages_json: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub timestamp: DateTime<Utc>,
}
```

Seven fields. `session_id` and `run_id` together identify the scope;
`turn` is the turn number at which the snapshot was taken (so the
resume logic knows where in the loop to pick up); `messages_json` is
the serialized `Vec<ChatMessage>` that the agent loop had just finished
processing; the two token counts carry the running totals; `timestamp`
marks the write time.

`run_id` deserves a note. It is distinct from `session_id` — sessions
outlive runs, and the same session can contain many runs (one per
user message). `run_id` is a freshly generated UUID at the top of
`AgentRuntime::run`, visible in
`crates/ryvos-agent/src/agent_loop.rs:332`. The checkpoint store uses
both columns in its uniqueness constraint so that multiple
concurrent runs in the same session cannot trample each other's
snapshots.

## CheckpointStore schema

The store opens or creates `checkpoints.db` with a single table. See
`crates/ryvos-agent/src/checkpoint.rs:43`:

```sql
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;

CREATE TABLE IF NOT EXISTS checkpoints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    turn INTEGER NOT NULL,
    messages_json TEXT NOT NULL,
    total_input_tokens INTEGER NOT NULL DEFAULT 0,
    total_output_tokens INTEGER NOT NULL DEFAULT 0,
    timestamp TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_cp_session_run
    ON checkpoints(session_id, run_id, turn DESC);
```

WAL mode is used for the same reason every other Ryvos SQLite store
uses it: concurrent read/write without blocking. `synchronous=NORMAL`
is the Ryvos default tradeoff — a small durability window on crash in
exchange for much faster writes. Messages are serialized as
JSON text (via `serde_json`) rather than as blobs because SQLite's
JSON functions make it possible to inspect checkpoints from the
command line without rebuilding `ChatMessage`.

The index on `(session_id, run_id, turn DESC)` is what makes
`load_latest` cheap — the most recent checkpoint for a session is the
leftmost row of the index, one key lookup away.

## Save, load, delete

`save` at `crates/ryvos-agent/src/checkpoint.rs:69` is a delete-then-
insert upsert. For a given `(session_id, run_id)` pair it deletes every
existing row and writes a fresh one:

```rust
conn.execute(
    "DELETE FROM checkpoints WHERE session_id = ?1 AND run_id = ?2",
    params![cp.session_id, cp.run_id],
)?;

conn.execute(
    "INSERT INTO checkpoints (session_id, run_id, turn, messages_json, total_input_tokens, total_output_tokens, timestamp)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    params![
        cp.session_id, cp.run_id,
        cp.turn as i64,
        cp.messages_json,
        cp.total_input_tokens as i64,
        cp.total_output_tokens as i64,
        cp.timestamp.to_rfc3339(),
    ],
)?;
```

The delete ensures that each `(session_id, run_id)` pair has exactly
one snapshot — the latest. Older snapshots for the same run are
discarded as soon as a new one is written. This is intentional: the
resume logic only ever wants the latest, and keeping older rows would
let `checkpoints.db` grow without bound on long-running sessions.

`load_latest` at `crates/ryvos-agent/src/checkpoint.rs:98` fetches the
most recent checkpoint for a session, scanning any run_id:

```rust
let mut stmt = conn.prepare(
    "SELECT session_id, run_id, turn, messages_json, total_input_tokens, total_output_tokens, timestamp
     FROM checkpoints
     WHERE session_id = ?1
     ORDER BY timestamp DESC
     LIMIT 1",
)?;
```

Ordering by `timestamp DESC` rather than by `turn DESC` handles the
edge case where a session has multiple run_ids: the latest run's
first turn is more recent than an older run's tenth turn, and the
restore should pick the latest run.

`delete(session_id)` nukes every checkpoint for a session;
`delete_run(session_id, run_id)` scopes the deletion to one run. The
loop uses `delete_run` on clean completion (keeping other runs
intact) and `delete` when the full session is being torn down. Two
helper methods, `serialize_messages` and `deserialize_messages`,
handle the JSON round-trip without the caller having to touch
`serde_json`.

## Save call site in the loop

The agent loop saves a checkpoint after every successful turn. See
`crates/ryvos-agent/src/agent_loop.rs:1139`:

```rust
if let Some(ref cp_store) = self.checkpoint_store {
    if let Ok(json) = CheckpointStore::serialize_messages(&messages) {
        let cp = crate::checkpoint::Checkpoint {
            session_id: session_id.0.clone(),
            run_id: run_id.clone(),
            turn,
            messages_json: json,
            total_input_tokens,
            total_output_tokens,
            timestamp: Utc::now(),
        };
        if let Err(e) = cp_store.save(&cp) {
            warn!(error = %e, "Failed to save checkpoint");
        }
    }
}
```

Three observations. First, the save is guarded by
`if let Some(ref cp_store)`: the runtime runs fine without a
checkpoint store attached, and the daemon has to opt in by calling
`set_checkpoint_store` at bootstrap. Second, a serialization or
save failure is logged but not propagated — a checkpoint failure
must not fail a run, because the run's real work has already
happened by this point. Third, the `messages` value at this point
is the post-pruning, post-tool-result state for the turn that just
finished, so a resume picks up with the pruned list and does not
replay pruning work.

On successful run completion, the checkpoint for the current run is
deleted. See `crates/ryvos-agent/src/agent_loop.rs:851`:

```rust
if let Some(ref cp_store) = self.checkpoint_store {
    cp_store.delete_run(&session_id.0, &run_id).ok();
}
```

Keeping a checkpoint after a clean exit would only waste space, since
the full history is already in the session store. The delete is
non-fatal — `.ok()` throws away any error — for the same reason as
the save.

## Resumption flow on crash

Suppose the daemon crashes mid-run. The next startup does not
automatically replay every incomplete run from the checkpoint store;
the design instead keeps checkpoints as a *recovery reserve* that the
next incoming message for a session can draw on.

When the next user message arrives on a channel, the channel
dispatcher maps it to a `session_key`, asks the **[session manager](../glossary.md#session)**
for the session id, and calls `runtime.run(&session_id, &text)`. The
runtime builds the message list the normal way: load history from
`sessions.db`, prepend the onion context, append the user message.
If a checkpoint store were consulted here, it would provide the
intermediate state from the crashed run — but the current
implementation does not do that automatic merge. The checkpoint
store is present primarily as a safety net for operator tooling
(`ryvos sessions inspect --checkpoint`) and for the goal-driven
**[Director](../glossary.md#director)**, which uses it to resume
incomplete graph executions.

The reason this works in practice is that `sessions.db` already
carries every user message and assistant response that was committed
before the crash. The loop appends the user message to the store
*before* the first LLM call, and it appends every assistant message
and tool result after each turn. So even without replaying a
checkpoint, the next run starts with a history that reflects
everything that the user saw. The checkpoint's extra value is
intermediate state — the specific turn number, the cumulative token
counts, and any in-flight mid-turn state that had not yet reached
the session store — which is useful for Director runs and for
operators diagnosing a crash but not required for normal channel
flow.

## SessionMetaStore

The second resumption layer is entirely different. `SessionMetaStore`
at `crates/ryvos-memory/src/session_meta.rs:12` persists per-session
metadata that must survive a daemon restart: the Ryvos session id,
the channel, the CLI session id (if any), running counters, and
timestamps.

```rust
pub struct SessionMetaStore {
    conn: Mutex<Connection>,
}

pub struct SessionMeta {
    pub session_key: String,
    pub session_id: String,
    pub channel: String,
    pub cli_session_id: Option<String>,
    pub total_runs: u64,
    pub total_tokens: u64,
    pub billing_type: Option<String>,
    pub started_at: String,
    pub last_active: String,
}
```

`session_key` is the primary key — a channel-scoped identifier like
`telegram:user:12345` that the dispatcher uses to map inbound
messages to sessions. The schema is one table, `session_meta`, with
one row per key. Every write bumps `last_active` to `now()`; every
`get_or_create` is idempotent.

The interesting column is `cli_session_id`, and the three methods
that manipulate it:

- `set_cli_session_id(key, id)` at
  `crates/ryvos-memory/src/session_meta.rs:134` is called after a
  run completes, to record the CLI's upstream session id for the
  next turn's `--resume`.
- `clear_cli_session_id(key)` at
  `crates/ryvos-memory/src/session_meta.rs:145` is called if a
  resume attempt fails, to force the next run to start fresh.
- `get(key)` at
  `crates/ryvos-memory/src/session_meta.rs:99` fetches the current
  row; the caller reads `cli_session_id` from the result.

`record_run_stats(key, tokens, billing_type)` at
`crates/ryvos-memory/src/session_meta.rs:156` bumps `total_runs` and
`total_tokens` atomically in one `UPDATE`. This is how the sessions
dashboard gets accurate per-user counters across restarts.

## CLI resume flow

The CLI resume flow is a four-step dance between the channel
dispatcher, the session meta store, the agent runtime, and the CLI
provider.

Step one: the channel dispatcher receives an incoming message with a
`session_key`. Before running the agent, it looks up the meta store
and reads `cli_session_id`. See
`crates/ryvos-channels/src/dispatch.rs:323`:

```rust
if let Some(ref meta_store) = session_meta {
    if let Ok(Some(meta)) = meta_store.get(&envelope.session_key) {
        if let Some(cli_id) = meta.cli_session_id {
            info!(cli_session = %cli_id, session_key = %envelope.session_key, "Resuming CLI session");
            runtime.set_cli_session_id(Some(cli_id));
        }
    }
}
```

`runtime.set_cli_session_id` stores the id in a one-shot override
field on the runtime; the next `run` call picks it up, splices it
into a cloned `ModelConfig` (leaving the shared config untouched),
and passes it to the LLM client.

Step two: the agent runtime calls `llm.chat_stream` with the model
config. For the `claude-code` provider, the client reads
`config.cli_session_id` and appends it to the command line. See
`crates/ryvos-llm/src/providers/claude_code.rs:146`:

```rust
// Session resumption
if let Some(ref session_id) = config.cli_session_id {
    args.push("--resume".to_string());
    args.push(session_id.clone());
}
```

The spawned `claude` CLI loads its own cached conversation from
`~/.claude/sessions/<id>.json`, appends the new prompt, and resumes
with its own context window intact.

Step three: the CLI starts streaming. Its first JSON event is a
`system` message with subtype `init` and a `session_id` field. The
provider recognizes this and emits a `StreamDelta::MessageId(id)`,
which the agent loop captures into `last_message_id`. See
`crates/ryvos-llm/src/providers/claude_code.rs:266`:

```rust
if json.get("subtype").and_then(|s| s.as_str()) == Some("init") {
    if let Some(session_id) = json["session_id"].as_str() {
        return Some(Ok(StreamDelta::MessageId(session_id.to_string())));
    }
}
```

The id coming out here is *not* necessarily the same as the id that
went in as `--resume`. Some CLI vendors rotate session ids on
resume, some do not; Ryvos treats them as opaque and always records
the latest value.

Step four: after the run finishes, the channel dispatcher reads
`runtime.last_message_id()` and writes it back to the meta store.
See `crates/ryvos-channels/src/dispatch.rs:461`:

```rust
if let Some(ref meta_store) = session_meta {
    if let Some(new_cli_id) = runtime.last_message_id() {
        info!(cli_session = %new_cli_id, session_key = %envelope.session_key, "Persisting CLI session ID");
        meta_store
            .set_cli_session_id(&envelope.session_key, &new_cli_id)
            .ok();
    }
}
```

The next incoming message for the same `session_key` will pick up
the new id and pass it as `--resume` again, continuing the chain.

If the resume fails — say, the user wiped `~/.claude/` between
restarts and the CLI rejected the id with "session not found" — the
CLI returns an error, the provider surfaces it as an LLM error, and
the dispatcher clears the stored id so the next attempt starts
fresh. The exact cleanup path lives in the recovery logic of the
specific provider implementation; from the session meta store's
perspective, `clear_cli_session_id` is called and the row's field
becomes NULL.

## Daemon hydration at startup

When the daemon starts, it loads every row from `SessionMetaStore`
and restores them into the in-memory `SessionManager`. See
`src/main.rs:896`:

```rust
if let Ok(metas) = session_meta.list() {
    let count = metas.len();
    for meta in metas {
        session_mgr.restore(
            &meta.session_key,
            &meta.session_id,
            &meta.channel,
            meta.cli_session_id.as_deref(),
        );
    }
    if count > 0 {
        info!(count, "Hydrated sessions from persistent store");
    }
}
```

After this, every previously-seen channel key is present in the
in-memory `SessionManager` with the correct session id, channel,
and (if any) CLI session id. The first incoming message for each
key will be routed to the same session as before the restart.

Checkpoints are not hydrated at startup. They are loaded on demand
if and when a resume is explicitly requested (e.g., via an
operator command or a Director recovery path).

## File layout

Every store the daemon uses is rooted in the workspace directory.
The session-meta store lives at a stable path opened during
bootstrap; the checkpoint store is opt-in and defaults to the
workspace root when a caller wires it via `set_checkpoint_store`:

```text
~/.ryvos/                         # workspace root
  data/
    session_meta.db               # SessionMetaStore (CLI resume + stats)
  checkpoints.db                  # CheckpointStore (when wired)
  sessions.db                     # SqliteStore (authoritative history)
  audit.db                        # AuditTrail
  ... (other subsystem DBs)
```

The split is deliberate. Checkpoints are an ephemeral recovery
reserve, deleted on successful run completion, and can be wiped
without losing data. Session meta is long-lived per-user state that
must survive restarts. Session history (`sessions.db`) is the
authoritative transcript that user-facing features read. Losing
`checkpoints.db` is a nuisance; losing `session_meta.db` breaks
`--resume` but preserves history; losing `sessions.db` is a real
data loss. The backup strategy in
[../operations/backup-and-restore.md](../operations/backup-and-restore.md)
treats them accordingly.

## Why two layers

A reasonable question is whether the two layers should be merged.
The answer is no, because they serve fundamentally different
purposes and have different failure modes.

Turn checkpointing is about intermediate state within a single run.
It is useful only for runs that crashed in the middle, and its
typical lifetime is "between two turns" — a few seconds to a few
minutes. It is written often (every turn) and deleted often (on
every clean completion).

CLI session metadata is about cross-run continuity. It is useful
for every run on a subscription-billed provider, and its typical
lifetime is "the full lifespan of a user relationship" — weeks or
months. It is written rarely (once per run, at the end) and
essentially never deleted.

Merging them would require one store to handle both workloads,
which would make the lifecycle rules more complex (when does a
turn checkpoint get promoted to long-lived metadata?), entangle
the two recovery paths, and complicate the backup story. Keeping
them separate costs one extra table and one extra SQLite file in
exchange for a much clearer contract.

## Testing

`CheckpointStore`'s test module at
`crates/ryvos-agent/src/checkpoint.rs:165` covers save/load round
trip, overwrite-on-same-run, delete, resume-simulation (save, load,
deserialize, continue, delete), and load-nonexistent. The tests
use a fresh temp directory per run so they are hermetic.

`SessionMetaStore`'s test module at
`crates/ryvos-memory/src/session_meta.rs:207` covers create/get,
the CLI session id lifecycle (set, read, clear), and the
`record_run_stats` accumulation. The stores are each ~250 lines of
code and ~80 lines of tests, which is appropriate — they are
simple persistence wrappers whose correctness rides on SQLite's own
guarantees.

There is no end-to-end test of a real daemon crash and restart;
that would require spinning up a subprocess and killing it
midway. The integration is verified by hand during release
testing.

## Where to go next

- [agent-loop.md](agent-loop.md) — the per-turn loop, including the
  `checkpoint_store.save` site and the `delete_run` on clean exit.
- [session-manager.md](session-manager.md) — the in-memory session
  index that `SessionMetaStore` hydrates at startup.
- [../crates/ryvos-agent.md](../crates/ryvos-agent.md) — the
  `CheckpointStore` position in the crate's module map.
- [../crates/ryvos-memory.md](../crates/ryvos-memory.md) — the
  `SessionMetaStore` position in the memory crate.
- [../crates/ryvos-llm.md](../crates/ryvos-llm.md) — CLI providers
  and the `--resume` flag plumbing.
- [../architecture/persistence.md](../architecture/persistence.md)
  — the full map of seven SQLite databases and their isolation
  rules.
