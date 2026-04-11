# ryvos-memory

`ryvos-memory` is the persistence layer. It sits alongside `ryvos-llm` in
the platform tier of the workspace and provides every SQLite-backed store
that the rest of Ryvos relies on: conversation history, hierarchical
**[Viking](../glossary.md#viking)** memory, cost accounting, CLI session
metadata, and encrypted OAuth tokens. The crate has no external database
dependency — every store is a file on disk — and no network surface of its
own, with one exception: `VikingClient` is an HTTP client for the standalone
`ryvos viking-server` binary when Viking is run out-of-process.

The design principle is one database per subsystem, recorded in
[ADR-006](../adr/006-separate-sqlite-databases.md). Each store owns its own
schema, manages its own migrations, and never joins against another store's
tables. Cross-store correlation happens on the **[EventBus](../glossary.md#eventbus)**
or through event IDs, not in SQL. The five stores implemented in this crate
are `SqliteStore`, `VikingStore`, `CostStore`, `SessionMetaStore`, and
`IntegrationStore`; two additional stores (`audit.db` owned by
`ryvos-agent` and `safety.db` owned by `ryvos-agent`) bring the full
runtime total to seven SQLite files.

## Purpose and position

The crate exports five store types plus a Viking HTTP client and a pricing
helper. See `crates/ryvos-memory/src/lib.rs:14`:

```rust
pub mod cost_store;
pub mod embeddings;
pub mod integration_store;
pub mod pricing;
pub mod session_meta;
pub mod store;
pub mod viking;
pub mod viking_store;

pub use cost_store::CostStore;
pub use integration_store::{IntegrationStore, IntegrationToken};
pub use pricing::estimate_cost_cents;
pub use session_meta::SessionMetaStore;
pub use store::SqliteStore;
pub use viking::VikingClient;
pub use viking_store::VikingStore;
```

Every store opens its database file lazily on first use, enables
`journal_mode=WAL` and `synchronous=NORMAL` in the first transaction, and
holds a connection for the lifetime of the binary. Most stores wrap the
connection in `std::sync::Mutex` because `rusqlite::Connection` is not
`Sync`. The single exception is `IntegrationStore`, which uses
`tokio::sync::Mutex` so that token lookups can `.await` without blocking a
runtime thread — the difference matters because integration token reads
happen on hot gateway paths and some writes run concurrently with OAuth
token-refresh HTTP requests.

`ryvos-memory` depends only on `ryvos-core` from the workspace graph. No
higher-layer crate is imported. The `SessionStore` trait implemented by
`SqliteStore` is defined in core, so every higher crate — `ryvos-agent`,
`ryvos-gateway`, `ryvos-channels` — can accept a trait object rather than
binding to SQLite directly. That trait split is what makes the in-memory
test fixture in `ryvos-test-utils` drop-in compatible with real persistence.

## SqliteStore: sessions and messages

`SqliteStore` at `crates/ryvos-memory/src/store.rs` is the canonical
**[SessionStore](../glossary.md#session)** implementation. It backs
`sessions.db` and holds three tables:

- **`messages`**: an id-indexed append log of `(session_id, role, content,
  timestamp)`. The `content` column is the JSON-serialized
  `Vec<ContentBlock>`, which preserves text, tool uses, tool results, and
  thinking blocks as a single round-trippable value.
- **`messages_fts`**: an FTS5 virtual table with `porter unicode61`
  tokenization. Rows are synchronized from `messages` via an `AFTER INSERT`
  trigger. Queries use SQLite's MATCH operator and `ORDER BY rank` to get
  BM25-ranked results. FTS5 is what powers the **[session](../glossary.md#session)**
  search surface that the gateway exposes on `/api/search`.
- **`embeddings`**: a binary blob table holding raw `f32` vectors
  serialized as little-endian bytes, keyed by `message_id`. Embeddings are
  optional — a message that was never embedded simply has no row here.

The `SessionStore` trait requires `append_messages`, `load_history`, and
`search`. `append_messages` serializes each message's content blocks to
JSON, loops over the batch, and inserts one row per message inside a
single lock-and-connection scope. `load_history` reads the last `limit`
messages for a given session ordered by primary key, which is the effective
append order. `search` executes the FTS5 query and returns
`SearchResult` records with BM25 rank.

Vector similarity search lives outside the `SessionStore` trait because it
is optional. `SqliteStore::store_embedding` writes an f32 vector as raw
bytes (little-endian, four bytes per component), and `search_similar`
loads every embedding into memory, computes cosine similarity against the
query vector, and sorts the results. This is deliberately a brute-force
scan rather than an approximate nearest-neighbour index: Ryvos sessions
rarely exceed a few thousand embedded messages, and the simplicity of the
brute-force approach avoids pulling in a dedicated ANN dependency. If a
future version needs a million-scale memory, a vector extension
(sqlite-vss, hnswlib, or a sidecar service) would replace this function
without touching the trait contract.

`SqliteStore` also provides an `in_memory` constructor that creates a
temporary SQLite database without touching disk. The constructor is used
by unit tests throughout the workspace and is the fastest way to exercise
the FTS5 tokenizer or the embeddings blob codec from a test harness.
Production code never calls `in_memory` — the binary always opens
`sessions.db` from the configured data directory.

## VikingStore: hierarchical memory

`VikingStore` at `crates/ryvos-memory/src/viking_store.rs` is the local
implementation of the Viking hierarchical memory system, backing
`viking.db`. It is the "library" half of the Viking design; the "service"
half lives behind `VikingClient` and is described below. The rationale for
using hierarchical memory at all is in
[ADR-003](../adr/003-viking-hierarchical-memory.md), and the full
architectural narrative is in
[architecture/memory-system.md](../architecture/memory-system.md).

Every entry in `viking_entries` lives at a path in the `viking://` URI
namespace (`viking://user/profile/name`, `viking://agent/patterns/git-rebase`,
and so on) and carries three representations that are auto-generated on
write:

- **L0 summary**: first sentence or first 100 characters. Used when the
  **[identity layer](../glossary.md#identity-layer)** or
  **[narrative layer](../glossary.md#narrative-layer)** needs a quick
  reminder of what an entry contains without paying for full content tokens.
- **L1 details**: first three paragraphs or first 500 characters. The
  default level for semantic recall.
- **L2 full**: the complete content. Used when the agent actively reads
  one specific entry and needs every byte.

The auto-generation happens in `VikingStore::generate_l0` and `generate_l1`,
both pure string functions. Writes are UPSERTs — the store deletes any
existing row at the same `(user_id, path)` before inserting the new one,
because FTS5 triggers make `INSERT ON CONFLICT` awkward to synchronize with
the shadow `viking_fts` virtual table. FTS sync is therefore performed
manually on every write and delete.

`viking_fts` is a second FTS5 table with `porter unicode61` tokenization,
storing `content`, `path`, and `user_id`. Search sanitizes the query by
splitting on whitespace, wrapping each term in quotes, and joining with
`OR` so that special FTS characters in the user's query never break the
statement. BM25 scores are returned from SQLite as negative floats and
clamped to `[0.0, 1.0]` as a "relevance score" for API consumers. Directory
prefix filtering is implemented with a `path LIKE 'prefix%'` clause.

Directory listing at `VikingStore::list_directory` groups entries by the
next path segment after the given prefix. Groups with a trailing suffix are
marked as directories; leaf paths become entries carrying their L0 summary.
The grouping walks results into a `BTreeMap`, which preserves alphabetical
order and makes pagination deterministic.

The iteration method at `VikingStore::iterate` is the auto-extraction
pipeline: it splits a session transcript into paragraphs of at least 20
characters and classifies each paragraph into one of six memory categories
— `user/preferences`, `user/profile`, `user/entities`, `agent/patterns`,
`agent/cases`, or `agent/events` — by keyword heuristics. The heuristics
are plain `str::contains` checks (for example, "prefer", "don't like", or
"always use" for preferences). Classified paragraphs are written as new
entries with timestamped paths of the form
`viking://category/YYYYMMDD-HHMMSS-i`. The whole pipeline is deliberately
unsophisticated — it runs after every session and favours recall over
precision, with the agent's own `Judge` or `Reflexion` passes trimming
whatever turns out to be noise.

## VikingClient: HTTP access to the standalone service

Ryvos supports two Viking deployment modes: embedded (`VikingStore`, local
SQLite in the daemon process) and standalone (`ryvos viking-server` on port
`1933`, consumed via `VikingClient`). The HTTP client lives at
`crates/ryvos-memory/src/viking.rs` and carries a base URL, a `reqwest`
client with a 10-second timeout, and a user ID for multi-user scoping.

The client exposes `write_memory`, `read_memory`, `search`, `list_directory`,
`delete_memory`, `trigger_iteration`, and `health`. Each maps to a
`/api/memory/*` endpoint on the standalone server with JSON request bodies
(`POST`) or query parameters (`GET`/`DELETE`). The `ContextLevel` parameter
(`L0`/`L1`/`L2`) lowercases itself via `Display` for URL safety.

Two type enums define the Viking shape independently of either backend.
`ContextLevel` is the detail tier that `read_memory` and search clients can
request. `MemoryCategory` is a typed alias over the six standard directory
paths; it exists so that in-process code can avoid hand-typing
`"viking://user/profile/"` strings. `VikingMeta`, `VikingResult`, and
`VikingDirEntry` are the shared over-the-wire DTOs.

`load_viking_context` is the injection helper that the `ryvos-agent`
context builder calls at the start of every turn. It issues up to three
calls — list the user directory, list the agent directory, and semantic
search on the user's current message — and assembles the results into a
Markdown block headed `# Sustained Context (Viking)` with `## User
Context`, `## Agent Context`, and `## Recalled Memories` sections. The
output is appended to the **[narrative layer](../glossary.md#narrative-layer)**
of the **[onion context](../glossary.md#onion-context)**. The function
swallows every Viking error rather than propagating it: if the HTTP server
is unreachable or returns an error, the onion just gets an empty string and
the turn continues.

`ContextLevelPolicy` is the small configuration struct that controls the
injection: which level to request for user and agent memories and how many
L0 summaries to load from each directory. Defaults are L0 for both
categories and 20 entries per directory, which is a conservative budget
that keeps the prompt under a kilobyte even for heavy users.

## Embeddings: provider-agnostic, brute-force cosine

`embeddings.rs` exposes an `EmbeddingProvider` trait with two methods:
`embed(texts)` returning a future of vectors, and `dimensions()` returning
the expected vector width. The one built-in implementation is
`HttpEmbeddingProvider`, which speaks the OpenAI embeddings wire format
(`POST /embeddings` with `{model, input: [...]}`, parse `data[].embedding`)
and therefore works unchanged against Ollama, OpenAI, and any
OpenAI-compatible embedding server.

`cosine_similarity` is a standalone function used by
`SqliteStore::search_similar` and by any higher-layer code that wants to
score two vectors without allocating. It returns `0.0` for mismatched or
empty vectors rather than erroring, which keeps the call site free of
defensive checks. The implementation is the textbook dot-product-over-
product-of-norms formulation with a guard against a zero denominator.

Embeddings are optional in Ryvos. Installations that do not configure an
embedding provider continue to work — session search falls back to FTS5
alone, and Viking's semantic `search` path uses BM25 instead. Turning
embeddings on is purely additive.

## CostStore: cost events and run logs

`CostStore` at `crates/ryvos-memory/src/cost_store.rs` backs `cost.db` and
holds two tables. `cost_events` is an append log of individual LLM calls
with their input and output token counts, the estimated cost in cents, the
billing type, the model, and the provider. `run_log` aggregates per-run
stats — one row per run with start time, end time, cumulative tokens, total
turns, cost in cents, and a `status` string (`running`, `complete`, or
`error`).

The two tables serve different consumers. `cost_events` is what the
**[Guardian](../glossary.md#guardian)** reads via `monthly_spend_cents` to
enforce the budget cap: it sums `cost_cents` for every event whose
`timestamp >= first day of current month`, and publishes
`BudgetExceeded` when the result crosses the configured threshold.
`run_log` is what the Web UI reads for its Costs dashboard via
`cost_summary` and `cost_by_group`; the `group_by` parameter accepts
`model`, `provider`, or `day` and rewrites the query with a safe
whitelist so that the Svelte frontend can pivot the table without needing
a new endpoint.

The lifecycle hook pair is `record_run` / `complete_run`. A run inserts
itself on start with status `running`, accumulates `cost_events` as
streaming usage deltas arrive from `ryvos-llm`, and updates its row on
completion with the final token totals, turn count, and status. If the
process crashes mid-run the row stays at `running` forever; the
**[checkpoint](../glossary.md#checkpoint)** recovery path in `ryvos-agent`
looks for dangling runs on startup and marks them `error` so that the
budget view does not double-count them.

## Pricing: cents per million tokens

`pricing.rs` is a pure function module that converts a token count into a
cost in cents. The internal `default_pricing` table hardcodes eleven
well-known models with their input and output rates:

| Model | Input (cents/MTok) | Output (cents/MTok) |
|---|---|---|
| `claude-sonnet-4` / `claude-sonnet-4-20250514` | 300 | 1500 |
| `claude-opus-4` / `claude-opus-4-20250514` | 1500 | 7500 |
| `claude-haiku` / `claude-haiku-4-5-20251001` | 80 | 400 |
| `gpt-4o` | 250 | 1000 |
| `gpt-4o-mini` | 15 | 60 |
| `gpt-4-turbo` | 1000 | 3000 |
| `o1` | 1500 | 6000 |
| `o1-mini` | 300 | 1200 |

`estimate_cost_cents` takes a model name, a provider, a billing type, the
input and output token counts, and a map of user-supplied overrides. It
short-circuits to `0` immediately if the billing type is
`BillingType::Subscription` — this is how `claude-code` and `copilot` runs
show `$0.00` even though real LLM calls happened. For API billing, it looks
up the model in the override map, falls back to the default table, and
finally applies a partial-match heuristic: if the model ID contains
`"opus"`, charge Opus rates; if it contains `"haiku"`, charge Haiku; if
`"sonnet"`, Sonnet; and so on. Unknown models default to Sonnet-class rates
(300/1500) rather than zero, so an unrecognized third-party model never
silently undercounts the budget.

Overrides live in `ryvos.toml` as a `[[model.pricing]]` array and are
passed in from the binary at startup. Users who run a custom or local model
can declare its rates explicitly and the budget math stays consistent.

## SessionMetaStore: CLI resume and per-channel stats

`SessionMetaStore` at `crates/ryvos-memory/src/session_meta.rs` is the
bridge between a Ryvos **[session](../glossary.md#session)** and a
**[CLI provider](../glossary.md#cli-provider)**'s own session identifier.
Each row is keyed by `session_key` — a channel-scoped identifier like
`tg:user:123` or `slack:T01:C99:U55` — and stores the Ryvos `session_id`,
the channel name, an optional `cli_session_id`, running totals for turn
count and token count, and start and last-active timestamps.

The CLI session ID is the load-bearing field. When a channel message maps
to an existing session, `get_or_create` returns the stored metadata, and
the CLI provider (`claude-code` or `copilot`) reads `cli_session_id` and
passes it as `--resume`. The CLI continues where it left off and emits a
new session ID in its `result` event, which the provider writes back via
`set_cli_session_id`. If a resume fails (for example, because the CLI
deleted the session file), `clear_cli_session_id` unsets the field so the
next turn starts fresh.

Aggregate stats (`total_runs`, `total_tokens`) are updated via
`record_run_stats` at the end of every run. This data is what the gateway
exposes on the Channels dashboard and what `ryvos sessions list` prints
in the TUI. Cost is not tracked here — the authoritative cost ledger lives
in `CostStore`.

## IntegrationStore: encrypted OAuth tokens

`IntegrationStore` at `crates/ryvos-memory/src/integration_store.rs` backs
`integrations.db` and holds OAuth access tokens, refresh tokens, and scopes
for external services — Gmail, Google Calendar, Slack, Notion, and any
future MCP server that requires a user-level credential. Rows are keyed by
`app_id`, which is the Ryvos-side handle (`gmail`, `google-calendar`,
`slack-workspace-123`), and carry the upstream `provider`, the tokens, an
optional expiry timestamp, the granted scopes, and a connection timestamp.

`IntegrationStore` is the one store in the crate that uses
`tokio::sync::Mutex` rather than `std::sync::Mutex` around its connection.
Every public method is `async` and awaits the lock. The motivation: token
refresh happens on the gateway's hot path and can call out to the provider
over HTTP while holding no other locks, and nesting a blocking
`std::sync::Mutex` inside an async call is a foot-gun that leaks tokio
worker threads under load. The async mutex forces every caller to be
explicit about suspension points and keeps the gateway responsive while a
refresh is in flight.

Token values are persisted as-is today; encryption at rest is planned and
will be layered on the access-token and refresh-token columns without a
schema migration, using a key derived from the daemon's identity file.
Until then, operators running Ryvos on shared machines should protect
`integrations.db` with filesystem permissions the same way they protect
`audit.db` and `safety.db`. The file-permission model is documented in
[operations/deployment.md](../operations/deployment.md).

A second detail worth highlighting: `IntegrationStore` is the only store
in the crate that stores strings the daemon never wants in logs. Every
`tracing::debug!` site that touches an `IntegrationToken` refers to the
`app_id` only; token bodies are never formatted into log messages or
events. The **[audit trail](../glossary.md#audit-trail)** in
`ryvos-agent` honours the same convention.

## The seven-database layout in context

Ryvos splits runtime state across seven SQLite files, five of which live
in this crate and two of which are owned by `ryvos-agent`. The full set
is `sessions.db`, `viking.db`, `cost.db`, `integrations.db`, plus the
session-meta database (which, depending on configuration, may share
`sessions.db` or live alongside it), `audit.db`, and `safety.db`. A
seventh database, `healing.db`, backs the
**[failure journal](../glossary.md#failure-journal)** and is also owned
by the agent crate.

The separation is deliberate. Each file has a different write cadence
(cost and audit are high-churn; viking and integrations are
low-churn), a different backup horizon (a stale cost DB is a budget
bug; a stale audit DB is a compliance incident), and a different
schema lifecycle. Running seven smaller databases also keeps WAL files
small — a single monolithic DB would funnel every write through one
file lock and serialize the subsystems against each other. Under
concurrent gateway, channel, and TUI load, keeping the write paths
independent is what lets the loop, the Guardian, and the Heartbeat
make progress without contending on a single mutex inside rusqlite.
The argument is laid out in full in
[ADR-006](../adr/006-separate-sqlite-databases.md).

## Invariants

Every store in this crate upholds a small set of invariants that the rest
of Ryvos relies on:

- **WAL + NORMAL sync.** Every database is opened with
  `PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL` in its first
  transaction. This gives concurrent reads a consistent snapshot while a
  write is in flight and keeps write amplification low at the cost of a
  narrow crash window. The `audit.db` and `safety.db` stores owned by
  `ryvos-agent` make the same choice.
- **One connection, mutex-wrapped.** Every store holds exactly one
  `rusqlite::Connection` guarded by a mutex. There is no connection pool;
  SQLite's single-writer model makes pooling pointless for this workload.
- **`std::sync::Mutex` by default, `tokio::sync::Mutex` only for
  `IntegrationStore`.** The async mutex is reserved for the one store whose
  callers always hold tokens across await points.
- **Databases never join.** Each file is entirely independent. Cost
  correlation with audit, for example, is done in application code via
  event IDs — not via a cross-database SQL statement.
- **Schemas are created idempotently at `open` time.** `CREATE TABLE IF
  NOT EXISTS` plus `CREATE INDEX IF NOT EXISTS` means the binary is safe to
  start against a fresh directory, an old directory, or a directory one
  schema version behind. Migrations that actually require moving data
  happen at boot via one-shot upgrade functions (not shown here) and are
  documented in [operations/upgrading.md](../operations/upgrading.md).

## Where to go next

The cost-tracking pipeline from `StreamDelta::Usage` deltas through
`CostStore` and into Guardian budget events is covered in detail by
[internals/cost-tracking.md](../internals/cost-tracking.md). The
architectural narrative for Viking — why L0/L1/L2 exists, how the
`viking://` namespace is used by the agent context builder, and how the
standalone server differs from the embedded store — is in
[architecture/memory-system.md](../architecture/memory-system.md) and
[ADR-003](../adr/003-viking-hierarchical-memory.md). The rationale for
splitting storage across seven SQLite files rather than one is in
[ADR-006](../adr/006-separate-sqlite-databases.md). To back up or restore
a running Ryvos installation, see
[operations/backup-and-restore.md](../operations/backup-and-restore.md).
