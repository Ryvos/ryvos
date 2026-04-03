# ADR-005: Event-Driven Pub/Sub Architecture

## Status

Accepted

## Context

Ryvos has a lot of moving parts. The agent loop processes tasks. The Guardian
monitors for anomalies. Channels (Telegram, Slack, Discord) send and receive
messages. The web UI shows real-time status. The cost tracker accumulates token
usage. The healer watches for repeated failures.

All of these components need to know what is happening in real time. The naive
approach would be direct function calls: the agent loop calls the cost tracker
after each LLM response, calls the Guardian after each tool execution, calls
the web UI to push updates, and so on. That creates tight coupling. Every new
component means modifying the agent loop.

We needed a way for components to communicate without knowing about each other.

## Decision

We built a central EventBus using tokio's broadcast channel with a capacity of
256 messages. Any component can publish events, and any component can subscribe
to the stream and filter for the events it cares about.

The event system defines 30+ event types organized by lifecycle phase:

- **Session events:** SessionStarted, SessionEnded, SessionResumed
- **Agent events:** TaskStarted, TaskCompleted, TaskFailed, ThinkingStarted
- **Tool events:** ToolCallStarted, ToolCallCompleted, ToolCallFailed
- **LLM events:** LlmRequestStarted, LlmResponseReceived, TokensUsed
- **Channel events:** MessageReceived, MessageSent, ApprovalRequested
- **Guardian events:** AnomalyDetected, SafetyLessonRecorded
- **System events:** ShutdownRequested, HealthCheckCompleted

Each event carries a typed payload with relevant data (timestamps, tool names,
token counts, error messages, etc.) and is serializable for audit logging.

Components subscribe to the bus at startup and spawn a tokio task that loops
over incoming events. For example, the cost tracker listens for TokensUsed
events and updates its running totals. The web UI listens for everything and
pushes updates to connected WebSocket clients. The Guardian listens for
ToolCallCompleted events and runs its anomaly detection logic.

## Consequences

**What went well:**

- Components are fully decoupled. The agent loop publishes events and does
  not know or care who is listening. Adding a new subscriber (say, a metrics
  exporter) requires zero changes to existing code.
- Real-time updates flow naturally. The web UI gets instant updates because
  it is just another subscriber. No polling needed.
- The audit log is a natural fit. A subscriber records every event to the
  audit database, giving us a complete timeline of everything that happened
  during a session.
- Testing is easier. You can test a component by feeding it synthetic events
  without needing the rest of the system running.

**What is harder:**

- tokio's broadcast channel drops the oldest messages when a subscriber falls
  behind. If a subscriber is slow (maybe the web UI has a stalled WebSocket
  connection), it will miss events. The channel capacity of 256 provides a
  reasonable buffer, but sustained bursts can overflow it.
- There is no guaranteed delivery. If a subscriber crashes and restarts, it
  misses whatever happened while it was down. For critical events (like cost
  tracking), we mitigate this by also writing to the database directly, so
  the subscriber can reconcile on startup.
- Debugging event flows can be tricky. When something goes wrong, you need to
  trace which events were published and which subscribers handled them. The
  audit log helps, but it adds a layer of indirection compared to a direct
  function call that you can step through in a debugger.
- The 30+ event types mean a lot of enum variants and match arms. This is
  manageable with Rust's exhaustive pattern matching (the compiler tells you
  if you miss a case), but it is still a lot of boilerplate.

The broadcast channel approach is simple and works well for our scale. If we
ever need guaranteed delivery or persistent event streams, we could move to
something like a write-ahead log. But for a single-process agent with a handful
of subscribers, tokio broadcast is the right tool.
