# ADR-003: Viking Memory with FTS5

## Status

Accepted

## Context

Agents need persistent memory across sessions. Without it, every conversation
starts from zero. The user has to re-explain their preferences, project context,
and prior decisions. That gets old fast.

We evaluated several options for the memory backend:

- **Flat files (markdown).** Simple, but hard to search efficiently. Works fine
  for a handful of notes, breaks down at hundreds or thousands.
- **Vector database (Qdrant, Chroma, pgvector).** Great for semantic search,
  but adds a heavy external dependency. Qdrant alone is 200MB+ and needs its
  own process. That violates our single-binary constraint.
- **Relational database (Postgres, MySQL).** Overkill. We do not need relational
  queries, joins, or complex transactions for memory storage.
- **SQLite with FTS5.** Embedded, zero external deps, fast full-text search.
  Ships as part of our binary. Already proven at massive scale (every phone
  on earth runs SQLite).

We also needed a way to organize memory hierarchically. Not everything is a flat
key-value pair. Some memories are high-level summaries ("this user prefers Rust"),
some are detailed context ("the crate layout uses 10 workspace members"), and
some are full documents (complete research notes).

## Decision

We built the Viking memory system on SQLite with FTS5 full-text search. Memory
is organized using a hierarchical protocol: `viking://path/to/topic`.

Each memory entry has three detail levels:

- **L0 (Summary):** A one-line summary. Used when the agent needs a quick
  refresher on what a topic is about. Think of it as the title and subtitle.
- **L1 (Details):** A paragraph or two of structured information. Used when
  the agent needs working context about a topic.
- **L2 (Full):** The complete document. Used when the agent needs to deeply
  reference something. Research notes, architecture docs, full specs.

The viking:// protocol provides a filesystem-like namespace. For example:
- `viking://project/ryvos/architecture` for the codebase architecture
- `viking://user/preferences` for user settings and habits
- `viking://research/mcp` for MCP protocol research notes

Listing, reading, writing, and searching all go through a clean Rust API that
the agent calls via tools (viking_read, viking_write, viking_search, viking_list).

The FTS5 index covers all three levels, so a search for "tokio broadcast" will
find memories that mention those terms at any detail level.

## Consequences

**What went well:**

- Zero external dependencies. The memory system is just another SQLite file
  in the data directory. No separate process to manage.
- FTS5 search is fast. Sub-millisecond for typical queries against thousands
  of memory entries. More than adequate for an agent's needs.
- The hierarchical levels solve the "too much context" problem elegantly.
  The agent can read L0 summaries to decide which topics are relevant, then
  drill into L1 or L2 only for what it actually needs. This keeps token usage
  reasonable.
- The viking:// namespace makes memory organization intuitive. Both the agent
  and the user can browse it like a filesystem.
- Works completely offline. No API calls, no cloud dependency.

**What is limited:**

- No semantic search. FTS5 is keyword-based, so searching for "async runtime"
  will not find a memory that only mentions "tokio" unless "async" or "runtime"
  appears in the text. This is a real gap that we plan to address later,
  possibly with a small local embedding model.
- FTS5 tokenization is English-centric. Other languages work but may not get
  optimal stemming or stopword handling.
- The L0/L1/L2 levels are populated manually by the agent (or by the user
  through the MCP tools). There is no automatic summarization yet. If the
  agent writes a long L2 document and forgets to update the L0 summary, the
  summary can become stale.

Overall, Viking with FTS5 gives us 90% of what we need with 10% of the
complexity of a vector database solution. The missing semantic search is a
known gap that we can fill incrementally without changing the architecture.
