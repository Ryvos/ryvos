# Session manager

Every Ryvos daemon sees a mix of inbound traffic from wildly different
sources — Telegram chats, Discord channels, Slack messages, WhatsApp
conversations, the Web UI, the REPL TUI, the REST API, scheduled cron
fires, and heartbeat self-checks. Every one of those sources needs to
end up in the same underlying execution model: the **[agent runtime](../glossary.md#agent-runtime)**
takes a `SessionId`, a user message, and runs. Something has to map
"the Telegram user with chat id 12345" and "the Discord user with id
98765 in channel X" to stable `SessionId` values that survive daemon
restarts and do not leak between channels.

That something is the `SessionManager`. It is small — about 120 lines
in `crates/ryvos-agent/src/session.rs` — but it owns a load-bearing
invariant: every channel adapter, every cron job, and every heartbeat
fire uses a consistent naming convention for its session key, and the
manager guarantees each key gets exactly one session that persists for
the lifetime of the daemon (and across restarts via
`SessionMetaStore`).

This document covers the in-memory `SessionManager`, the persistent
`SessionMetaStore` that hydrates it at startup, the channel key
conventions, and the channel dispatcher's use of both. For the deeper
persistence story, see [checkpoint-resume.md](checkpoint-resume.md).
For a crate-level view, see
[../crates/ryvos-agent.md](../crates/ryvos-agent.md).

## The in-memory index

`SessionManager` at `crates/ryvos-agent/src/session.rs:6`:

```rust
pub struct SessionManager {
    sessions: Mutex<HashMap<String, SessionInfo>>,
}

pub struct SessionInfo {
    pub session_id: SessionId,
    pub channel: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub last_active: chrono::DateTime<chrono::Utc>,
    pub cli_session_id: Option<String>,
    pub total_runs: u64,
    pub total_tokens: u64,
    pub billing_type: Option<String>,
}
```

A single mutex-protected hashmap. Keys are channel-scoped strings (see
the conventions table below); values are `SessionInfo` records carrying
everything the channel dispatcher and the sessions dashboard need to
know about a conversation. The `Mutex` is `std::sync::Mutex`, not
tokio's — the lock is only held for a handful of microseconds per
operation, and the sync mutex is faster when contention is rare, which
is the common case.

The manager is held behind an `Arc` for the lifetime of the daemon.
Every channel adapter has a clone; so does the cron scheduler, the
heartbeat task, the gateway, and the approval broker. All of them
consult the same index.

## The main API

Five methods cover almost everything callers do.

`get_or_create(key, channel)` at
`crates/ryvos-agent/src/session.rs:29` is the primary entry point. It
looks up `key` in the map; on hit, it bumps `last_active` to `now()`
and returns the stored `SessionId`; on miss, it generates a fresh
`SessionId`, inserts a new `SessionInfo` with the passed `channel`
name, and returns the new id. The channel name is stored only once,
at creation time, and does not change even if the same key is later
addressed through a different channel (which should not happen — the
key convention embeds the channel in the key itself).

`set_cli_session_id(key, id)`, `get_cli_session_id(key)`, and
`clear_cli_session_id(key)` mutate the `cli_session_id` field in
place. These are the in-memory counterparts to the persistent
versions in `SessionMetaStore`: every update goes through both, so
the in-memory value is fresh and the persistent value survives a
restart. The separation is an optimization — reading
`get_cli_session_id` on every incoming message should not touch
SQLite, but the authoritative value must survive daemon death.

`record_run_stats(key, tokens, billing_type)` at
`crates/ryvos-agent/src/session.rs:76` bumps `total_runs` by one,
adds `tokens` to `total_tokens`, updates `billing_type` (which is
per-run in theory because an operator could switch providers
mid-session), and sets `last_active` to `now()`. It is called after
every run completes, from the channel dispatcher or the cron
scheduler.

`restore(key, session_id, channel, cli_session_id)` at
`crates/ryvos-agent/src/session.rs:87` is the hydration path.
Unlike `get_or_create`, it takes an explicit `session_id` rather
than generating one, and it is called exactly once per key during
daemon startup to rebuild the in-memory index from the persistent
store. After hydration, subsequent `get_or_create` calls find the
existing entry and return the hydrated `SessionId`.

`list()` returns every current key as a `Vec<String>`, used by the
`/sessions list` command and the gateway's sessions dashboard.
Callers that need the `SessionInfo` details rather than just keys
typically go through the gateway's REST endpoint, which reads from
the persistent store for accurate cross-restart values.

## Channel session key conventions

The key format is the contract between channel adapters and the
session manager. Every adapter has a unique format that embeds
enough context to route messages correctly. The current
conventions:

| Source | Key format | Set by |
|---|---|---|
| Telegram | `telegram:user:{chat_id}` | `crates/ryvos-channels/src/telegram.rs:143` |
| Discord | `discord:channel:{channel_id}:user:{user_id}` | `crates/ryvos-channels/src/discord.rs:97` |
| Slack | `slack:channel:{channel_id}:user:{user_id}` | `crates/ryvos-channels/src/slack.rs:364` |
| WhatsApp | `whatsapp:user:{phone}` | `crates/ryvos-channels/src/whatsapp.rs:393` |
| Web UI | `webui:{timestamp}` or `webui:default` | `ui-src/src/lib/pages/Chat.svelte` |
| Cron | `cron:{job_name}` | `crates/ryvos-agent/src/scheduler.rs:116` |
| Heartbeat | `heartbeat:{YYYYMMDD-HHMMSS}` | `crates/ryvos-agent/src/heartbeat.rs:106` |
| REST API | Client-supplied | Gateway endpoint handlers |
| CLI REPL | `cli:repl` | Binary entry point |

The conventions respect two invariants. First, *every platform's
key starts with the platform name* followed by a colon. This
guarantees that no two platforms can accidentally collide. Second,
*the key contains enough identity to separate users*. Telegram uses
the raw chat id, which is unique per chat. Discord and Slack
include both the channel and the user because a user can have
parallel conversations in multiple channels. WhatsApp uses the
phone number. The Web UI uses a timestamp-derived token per browser
tab (or `webui:default` for single-tab setups) so multiple tabs are
different conversations. Cron jobs name themselves; heartbeat fires
stamp themselves with the fire time.

Only the adapter creating the key is responsible for the format;
once the key is built, the session manager treats it as an opaque
string.

## Per-channel isolation

The per-channel keying is what keeps messages from leaking between
platforms even for the same physical user. If user Alice writes to
the Ryvos daemon on Telegram and also on the Web UI, she gets two
separate `SessionId`s — `telegram:user:12345` and `webui:abc123`
— which map to two separate rows in `sessions.db` and two separate
conversation histories. The LLM does not see either conversation
when it is answering the other.

This is a deliberate design choice. The alternative — one Alice,
one session, all channels — sounds nicer in the abstract but
breaks down in practice: different channels have different context
appropriate to them (a casual Telegram DM vs. a professional Slack
thread vs. a private voice note), and stitching them together
creates context that Alice did not intend to share. The per-channel
split gives Alice N relationships with the agent, each appropriate
to its medium, and operators can cross-reference them when needed
through the Web UI by navigating multiple sessions manually.

Cron jobs and heartbeat fires are their own sessions for the same
reason: the cron's context is not the user's context, and mixing
them would pollute both.

## The dispatcher's use

The channel dispatcher in `crates/ryvos-channels/src/dispatch.rs`
is the biggest caller of `SessionManager` and shows the end-to-end
flow. When an adapter publishes a `MessageEnvelope` onto the
dispatcher's inbound mpsc, the envelope already carries a
`session_key` and a `session_id`. The adapter builds both at the
moment it sees the message: it constructs the key from the
convention (e.g., `format!("telegram:user:{}", chat_id)`), calls
`session_mgr.get_or_create(&key, "telegram")` to obtain the
`SessionId`, stores the mapping in its own `chat_map` so later
`send` calls can route responses back to the right chat, and
pushes the envelope into the dispatcher.

The dispatcher's `run_channel_message` path then does two things:
it looks up `session_meta.get(&envelope.session_key)` to retrieve
any persisted `cli_session_id` for CLI resume, and it calls
`meta_store.get_or_create(&envelope.session_key, &session_id.0, &envelope.channel)`
to guarantee the persistent row exists. Both calls are independent
of the in-memory `SessionManager` — they go straight to
`SessionMetaStore`. The split is because the in-memory manager is
tuned for fast lookups on the hot path, while the persistent
store is tuned for durability.

After the run completes, the dispatcher reads
`runtime.last_message_id()` and writes it to the meta store via
`set_cli_session_id`. It does *not* mirror the write to the
in-memory manager — the in-memory manager's `cli_session_id` field
is a convenience for operator tooling and the REPL, not the
authoritative value. The authoritative value is in
`session_meta.db`, and the next run will read it back from there.

## Persistent hydration at startup

`SessionMetaStore` at `crates/ryvos-memory/src/session_meta.rs:12`
is the persistent counterpart of `SessionManager`. It persists
session metadata across daemon restarts through a SQLite table,
keyed by `session_key`. The schema and lifecycle are documented in
[checkpoint-resume.md](checkpoint-resume.md); the relevant bit here
is the startup dance.

At daemon boot, `src/main.rs:896` calls `session_meta.list()`,
iterates every row, and calls `SessionManager::restore` for each:

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

After hydration, every session that existed before the restart is
present in the in-memory map with the same `SessionId`, the same
channel, and the same `cli_session_id`. When the next message from
one of those sessions arrives, `get_or_create` finds the existing
entry and returns it unchanged. The user's conversation continues
seamlessly: their next message lands in the same
`sessions.db`-backed history, the next `--resume` (if applicable)
targets the same CLI session, and the total runs and token counts
carry over to the UI.

Two counters do not survive hydration: `total_runs` and
`total_tokens` are reset to 0 in the in-memory `SessionInfo`. The
authoritative counters live in `SessionMetaStore`, which the
gateway reads directly for the dashboard. The in-memory values
are only used by short-lived consumers that count runs within a
daemon lifetime; after a restart, they start fresh because that is
the right semantics for an "active this session" counter.

## Channel-specific details

Every adapter's use of `SessionManager` follows the same shape
but diverges in the details of how it constructs keys and routes
responses. Three examples:

**Telegram** at `crates/ryvos-channels/src/telegram.rs:143` maps
each incoming chat message to `telegram:user:{chat_id}`. The
chat id is unique across users, groups, and channels, so it
suffices as the identity. The adapter stores
`chat_map[session_id] = chat_id` so the outbound `send` path can
look up the right chat id when the runtime produces a response.

**Discord** at `crates/ryvos-channels/src/discord.rs:97` maps each
message to `discord:channel:{channel_id}:user:{user_id}`. The
channel id is included because a user can have parallel
conversations in different channels of the same server, and mixing
them would be wrong. Server (guild) id is not in the key because
channel ids are unique across servers.

**Slack** at `crates/ryvos-channels/src/slack.rs:364` uses the
same structure as Discord: `slack:channel:{channel_id}:user:{user_id}`.
The Slack adapter also honors a DM policy (`DmPolicy::Open` /
`DmPolicy::Restricted`) before calling `get_or_create`, so
restricted workspaces can block messages from non-allowlisted
users before they touch the session manager at all.

**Web UI** is the odd one. The Svelte app at
`crates/ryvos-gateway/ui-src/src/lib/pages/Chat.svelte` generates
a per-tab key as `webui:{Date.now().toString(36)}` so each browser
tab is a separate conversation, or uses `webui:default` for the
single-tab convention. The key is sent to the gateway WebSocket
over the `agent.send` RPC; the gateway calls
`session_mgr.get_or_create(key, "webui")` on the first message.
Because the key is client-supplied, it is the gateway's
responsibility to validate it before passing it through — a
malicious client could otherwise collide with another user's
session. The Web UI tags its keys with enough entropy that
accidental collisions are vanishingly unlikely, and the gateway's
auth layer enforces that only authorized clients can create new
sessions.

## Cron and heartbeat sessions

Scheduled runs are not user conversations, but they still need
sessions so that `sessions.db` can store their history, the audit
trail can record their tool calls, and cost tracking can attribute
their token usage. Both cron and heartbeat create synthetic
session ids.

Cron at `crates/ryvos-agent/src/scheduler.rs:116`:

```rust
let session_id = SessionId::from_string(&format!("cron:{}", job.name));
```

The job name — a human-readable string from `ryvos.toml` — is the
whole key. The same cron job always runs in the same session, so
its history accumulates over time and the Director can take
advantage of past runs when reasoning about the next. This is an
intentional choice: a cron job is more like a long-running ritual
than like a set of independent one-shots.

Heartbeat at `crates/ryvos-agent/src/heartbeat.rs:106`:

```rust
let session_id =
    SessionId::from_string(&format!("heartbeat:{}", now.format("%Y%m%d-%H%M%S")));
```

Each heartbeat fire is its own session, stamped with the firing
time. This is the opposite choice: a heartbeat is a one-shot
self-check, and its history should not spill into the next fire.
If heartbeats shared a session, the onion context builder would
keep loading stale prior checks and the LLM would drift into
"I have already checked this" responses. Per-fire sessions keep
each check clean.

Both cron and heartbeat sessions are written to `sessions.db`,
audited in `audit.db`, and cost-tracked in `cost.db` the same way
user conversations are, so they appear in the Web UI's sessions
list alongside real users.

## list() and the sessions dashboard

`SessionManager::list()` returns the in-memory key set as a
`Vec<String>`, sorted in insertion order (which is hashmap order,
so effectively unordered). It is called by the gateway's
`/api/sessions` endpoint and by the TUI's session picker to
enumerate currently-loaded sessions.

For the Web UI sessions dashboard, the gateway queries
`SessionMetaStore::list()` directly rather than the in-memory
manager, because the meta store has `total_runs`, `total_tokens`,
`started_at`, and `last_active` — all the fields the dashboard
displays — and because the meta store is authoritative. The
in-memory manager is used for the fast hot-path lookup during
message dispatch; the persistent store is used for listing and
reporting.

## Interaction with checkpoint-resume

The session manager, the session-meta store, and the
**[checkpoint](../glossary.md#checkpoint)** store interact in a
specific order on the recovery path. A daemon restart proceeds as
follows:

1. Open `sessions.db` and make the authoritative message history
   available.
2. Open `session_meta.db` and call
   `session_meta.list()` to get every persisted session.
3. For each row, call `session_mgr.restore(...)` to rebuild the
   in-memory index.
4. (Optional) Open `checkpoints.db`. Leave it available as a
   recovery reserve for operator tooling and the Director's
   semantic-failure replay path. Do not automatically re-inject
   checkpoint state into any session.
5. Start channel adapters. They begin accepting messages and
   routing them through the hydrated session manager.

When a new message arrives on an existing session, the
dispatcher calls `session_mgr.get_or_create`, which returns the
hydrated `SessionId`, which the runtime uses to load history from
`sessions.db`. Everything lines up without any per-session replay
logic.

See [checkpoint-resume.md](checkpoint-resume.md) for the full
story of how the checkpoint store fits into this, and for the
distinction between turn-level checkpoints (internal to Ryvos)
and CLI session ids (external to `claude-code` and `copilot`).

## Where to go next

- [checkpoint-resume.md](checkpoint-resume.md) — the
  `SessionMetaStore` schema, the CLI resume flow, and the
  turn-level checkpoint store.
- [../crates/ryvos-agent.md](../crates/ryvos-agent.md) — the
  `SessionManager` position in the agent crate's module map.
- [../crates/ryvos-channels.md](../crates/ryvos-channels.md) —
  every channel adapter's message routing, including the
  session-key formats.
- [../architecture/data-flow.md](../architecture/data-flow.md) —
  the end-to-end path of a message from channel to LLM to
  response, showing how session routing fits in.
- [agent-loop.md](agent-loop.md) — the runtime's use of the
  `SessionId` when loading history, building context, and
  running the per-turn loop.
