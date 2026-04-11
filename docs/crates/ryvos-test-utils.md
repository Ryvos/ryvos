# ryvos-test-utils

`ryvos-test-utils` is the shared testing infrastructure for the Ryvos
workspace. It provides mock implementations of the four extension-point
traits defined in [`ryvos-core`](ryvos-core.md) — `LlmClient`, `Tool`,
`ChannelAdapter`, `SessionStore` — plus a small set of fixtures for
constructing test configs and tool contexts. Every crate that wants to
unit-test logic involving those traits pulls `ryvos-test-utils` in as a
`[dev-dependencies]` entry and avoids the cost of spinning up a real LLM,
a real SQLite database, a real Telegram bot, or a real subprocess.

The crate is small by design (six source files, roughly 580 lines total,
and a handful of tiny dependencies). It is not part of the runtime
dependency graph shown in
[../architecture/system-overview.md](../architecture/system-overview.md) —
no production crate depends on it. It sits in the foundation layer
alongside `ryvos-core` only because both are workspace-wide primitives
that everything else consumes.

This document covers the four mock implementations, the two fixture
functions, the typical wiring pattern for integration tests, and the cases
where a real implementation is the right choice instead.

## Position in the stack

`ryvos-test-utils` has exactly one workspace dependency: `ryvos-core`. Its
external dependencies are `tokio`, `futures`, `serde_json`, `toml`, and
`tempfile`. It does not depend on `ryvos-llm`, `ryvos-memory`,
`ryvos-agent`, or any integration-layer crate. This layering is
deliberate — it keeps the mock set reusable from any crate in the
workspace without risking a circular dependency.

Each mock lives in its own module under `crates/ryvos-test-utils/src/`
and is re-exported from `crates/ryvos-test-utils/src/lib.rs:14` so that
consumers can write `use ryvos_test_utils::MockLlmClient;` without
knowing the internal file layout.

## MockLlmClient

`MockLlmClient` (`crates/ryvos-test-utils/src/mock_llm.rs:14`) is a
scripted, thread-safe implementation of `LlmClient`. Internally it owns
two `Arc<Mutex<Vec<...>>>` fields: one queue of response sequences and
one log of recorded call-site messages.

Tests push response sequences onto the queue with a builder-style API.
`with_response` accepts a raw `Vec<StreamDelta>` for full control.
`with_text_response(text)` is a convenience that expands to a
`TextDelta`, a `Usage` delta with `(100, 50)` token counts, and a
`Stop(StopReason::EndTurn)`. `with_tool_call(name, input_json)` expands
to a `ToolUseStart`, a `ToolInputDelta` carrying the raw JSON string, a
`Usage`, and a `Stop(StopReason::ToolUse)`. Multiple responses can be
chained: the first call to `chat_stream` pops the first response, the
second call pops the second, and so on. If the queue is empty the mock
returns `RyvosError::LlmRequest("No more mock responses")`, which is
intentional — a test that runs more LLM calls than it scripted is
almost always a test bug.

The mock also records every inbound call so that assertions can inspect
the conversation state the runtime constructed. `call_count` returns the
number of `chat_stream` invocations; `call_messages(n)` returns the
`Vec<ChatMessage>` that was passed on the nth call. This is what tests
use to verify that the agent loop built the expected context window,
appended tool results in the right order, or pruned older turns when it
should have.

Under the hood `chat_stream` turns the popped response into a
`futures::stream::iter` wrapped in `Box::pin` so that it matches the
`BoxStream` return type declared by the trait. The implementation is
strictly in-memory — it never sleeps, never spawns, and never performs
I/O, which keeps unit tests fast and deterministic.

## InMemorySessionStore

`InMemorySessionStore` (`crates/ryvos-test-utils/src/mock_store.rs:11`)
is a `HashMap<String, Vec<ChatMessage>>` behind a `Mutex`, implementing
the full `SessionStore` trait.

`append_messages` extends the vector for the given session id, creating
it with `entry().or_default()` if it does not exist. `load_history`
returns the last `limit` messages, computed by
`v.len().saturating_sub(limit)` so that asking for more messages than the
session contains returns everything without panicking. `search` does a
case-insensitive substring scan: it lowercases the query once, iterates
every session, extracts each message's flattened text via
`ChatMessage::text()`, and collects messages whose text contains the
query. Every match has a fixed `rank` of `1.0` — this is a mock, not an
FTS5 index, and tests that care about ranking should use the real
`SqliteSessionStore` in `ryvos-memory`.

Two introspection helpers sit alongside the trait methods:
`session_count` returns the number of distinct session ids the store has
seen, and `message_count(session_id)` returns the length of a specific
session's message vector. Tests use these as quick sanity checks after
an agent run (for example, "the agent persisted exactly six messages for
this session").

The in-memory store is the right choice for any test that exercises the
agent loop's persistence pathway but does not need FTS5 semantics, SQLite
migrations, or cross-process durability. For tests that do need any of
those, construct a temporary directory with `tempfile::TempDir` and wire
up the real `SqliteSessionStore`.

## MockChannelAdapter

`MockChannelAdapter` (`crates/ryvos-test-utils/src/mock_channel.rs:11`)
is a scripted `ChannelAdapter` that captures everything sent through it
for later assertion. It owns two `Arc<Mutex<Vec<...>>>` fields: one for
messages sent to specific sessions via `send`, and one for messages
broadcast to all users via `broadcast`.

The `name` method returns whatever string the test passed to
`MockChannelAdapter::new`. `start` is a no-op that returns `Ok(())` —
the mock does not pretend to be a real event source, and tests that need
to inject inbound messages should drive the `mpsc::Sender` directly.
`send` clones the `MessageContent` and appends `(session_id, content)`
to the `sent` log. `broadcast` clones and appends to the `broadcasts`
log. `stop` is a no-op.

Three introspection helpers support assertions: `sent_messages()`
returns a clone of the full `(session_id, content)` log,
`broadcast_messages()` returns a clone of the broadcast log, and
`send_count()` returns the length of the sent log. Tests usually assert
on `send_count()` first to verify that the expected number of responses
came out, then on `sent_messages()[0].1` to verify the content of a
specific response. The mock does not implement `send_approval`, so it
inherits the default trait impl that returns `Ok(false)` — which causes
the runtime to fall back to plain-text approval UI, exactly the path
most tests want to exercise.

## MockTool

`MockTool` (`crates/ryvos-test-utils/src/mock_tool.rs:11`) is a
configurable implementation of the `Tool` trait that records every
invocation. Its internal state is the tool's name, description, JSON
schema, result, security tier, and an `Arc<Mutex<Vec<serde_json::Value>>>`
of recorded inputs.

`MockTool::new(name)` constructs a tool that returns
`ToolResult::success("mock output")` and carries a stub schema of
`{"type": "object", "properties": {}}`. The builder methods
`with_result`, `with_description`, `with_schema`, and `with_tier`
override these fields. Tests that need to simulate a failure path use
`with_result(ToolResult::error("..."))`; tests that care about the
tool's declared tier (for example, to verify that the audit trail
records the right value) use `with_tier(SecurityTier::T3)`.

`execute` clones the recorded result into the returned future, so a
single `MockTool` instance can be invoked any number of times and will
always return the same output. The invocation log is appended before the
future is constructed, so tests can assert on `invocation_count()` even
if the future has not been polled. `invocation_input(n)` returns the JSON
value from the nth call, which is what tests use to verify that the
agent loop forwarded the LLM's arguments unchanged.

Default values are worth noting. The tier defaults to
`SecurityTier::T0` (read-only), which is the safest choice for tests
that do not explicitly care about tiers. `MockTool` does not override
`timeout_secs` or `requires_sandbox`, so it inherits the trait defaults
of `30` and `false`. A test that needs to exercise the runtime's
per-tool timeout path should use a real tool implementation with a
tightly scoped timeout rather than trying to stretch the mock.

## Fixtures

Two fixture helpers in `crates/ryvos-test-utils/src/fixtures.rs` cover
the constructors that almost every test needs.

`test_config()` parses a fixed inline TOML snippet into an `AppConfig`.
The snippet defines a `[model]` section with
`provider = "openai"`, `model_id = "test-model"`,
`api_key = "test-key"`, plus a `[agent]` section with `max_turns = 5`
and `max_duration_secs = 30`. Every other field is `#[serde(default)]`
in `ryvos-core` so the resulting config is complete. A test that needs
to tweak one field clones the returned config and mutates it directly;
there is no builder macro.

`test_tool_context()` returns a `ToolContext` with session id
`"test-session"`, working directory `/tmp/ryvos-test`, and every
optional field set to `None`. This is the context passed to `Tool::execute`
in the bulk of unit tests — a tool that does not need a real working
directory, a session store, an agent spawner, a sandbox config, a config
file path, or a Viking client can use this as-is.
`test_tool_context_with_dir(dir)` is the same but lets the caller
supply a `PathBuf`, which is what tests do when they pair the context
with a `tempfile::TempDir` so that the tool writes land in an isolated
scratch directory that cleans up on drop.

## Testing patterns

A typical integration test for the agent loop wires all four mocks
together. Conceptually:

1. Build a `test_config()` and optionally patch its `model` or `agent`
   fields if the test needs specific turn limits.
2. Construct a `MockLlmClient` and chain `with_text_response` and
   `with_tool_call` calls to script the LLM's turn-by-turn behavior.
3. Construct any number of `MockTool` instances and wrap them in an
   `Arc` for the tool registry.
4. Wrap an `InMemorySessionStore` in an `Arc` and pass it to whatever
   runtime-construction call the test is exercising.
5. Optionally construct a `MockChannelAdapter` if the test needs to
   assert on outbound messages.
6. Run the subject under test (typically an `AgentRuntime::run`
   call from `ryvos-agent`) and await completion.
7. Assert on the mock counters: `MockLlmClient::call_count` for the
   number of turns the agent took, `MockTool::invocation_count` for the
   number of tool dispatches, `InMemorySessionStore::message_count` for
   the number of persisted messages, and
   `MockChannelAdapter::sent_messages` for the final outbound content.

The mocks are intentionally passive: they record state and return
scripted responses. They do not assert anything themselves, and they do
not fail the test if something unexpected happens — they leave that to
the test body. This keeps each test in charge of its own assertions and
avoids the "helper checks it for you" failure mode that makes tests
hard to debug when they break in CI.

Because every mock is thread-safe (`Arc<Mutex<...>>` internals), the
same mock instance can be cloned into multiple tokio tasks and assertions
can be made from outside the task that drove the runtime. This is
particularly useful for tests that subscribe to an `EventBus` from a
separate task to verify that a specific `AgentEvent` was published.

Most tests that use these mocks live in the `#[cfg(test)]` modules of
the crate they are testing, and a few live in the
`crates/*/tests/` integration-test directories. The mocks' own tests
(at the bottom of each `mock_*.rs` file) double as usage examples for
anyone wiring them up for the first time.

## When not to use these mocks

Mock-driven tests are the default because they are fast and
deterministic, but three categories of test should reach for real
implementations instead.

Tests that exercise SQLite behavior — FTS5 ranking, migration upgrades,
crash-recovery checkpoint reload, concurrent writes from multiple
sessions — should use `SqliteSessionStore` from `ryvos-memory` wired to
a `tempfile::TempDir`. `InMemorySessionStore::search` is a lowercase
substring scan and produces ranks of `1.0`, which is not enough to
validate anything FTS5-specific.

Tests that exercise real LLM provider behavior — streaming backpressure,
retry logic, SigV4 signing for Bedrock, CLI subprocess spawn for
`claude-code` — should use the real provider in `ryvos-llm` behind an
`#[ignore]` marker or a `--features integration` feature flag. The goal
of `MockLlmClient` is to test the agent loop's use of the trait, not
the provider's conformance to the protocol. Provider-conformance tests
belong inside `ryvos-llm` itself.

Tests that exercise real channel behavior — Telegram inline-button
callbacks, Discord slash-command routing, Slack `/approve` modal dialogs,
WhatsApp template messages — need a live API or a carefully recorded
HTTP fixture. `MockChannelAdapter` captures outbound text but does not
simulate anything platform-specific. Contributors adding a new channel
adapter should rely on the platform's own SDK test harness (if any) or
on `wiremock` HTTP fixtures.

For everything else — unit tests that exercise the ReAct loop, tool
dispatch, session persistence, goal evaluation, guardian detection, and
channel routing — the four mocks in this crate are the right starting
point. They are the piece of infrastructure that keeps Ryvos's test
suite deterministic without a dedicated test environment or a third-party
mocking framework.

## Cross references

- The traits these mocks implement are defined in
  [ryvos-core.md](ryvos-core.md).
- The real `SqliteSessionStore` and the rest of the persistence layer
  are documented in [ryvos-memory.md](ryvos-memory.md).
- The real LLM provider implementations are documented in
  [ryvos-llm.md](ryvos-llm.md).
- Test-writing conventions for new contributors are covered in
  [../CONTRIBUTING.md](../../CONTRIBUTING.md).
- The agent loop internals that these mocks are typically used to
  exercise are in
  [../internals/agent-loop.md](../internals/agent-loop.md).
