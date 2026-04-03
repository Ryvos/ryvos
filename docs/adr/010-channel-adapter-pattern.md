# ADR-010: Channel Adapter Trait

## Status

Accepted

## Context

Ryvos needs to talk to users through multiple messaging platforms. Some people
live in Telegram, others use Slack for work, Discord is popular for communities,
and WhatsApp is the default in many countries.

Each platform has its own API, authentication model, message format, rate limits,
and capabilities. Building a separate integration from scratch for each one would
mean duplicating a lot of logic: message chunking, approval formatting, error
handling, connection management, and retry logic.

We needed a pattern that makes adding new channels easy while keeping the core
agent logic platform-agnostic.

## Decision

We defined a ChannelAdapter trait that every messaging platform must implement.
The trait has six methods:

- `name() -> String` returns the adapter's identifier (e.g., "telegram",
  "slack", "discord").
- `start()` initializes the connection, sets up webhooks or polling, and
  begins listening for incoming messages.
- `send(recipient, message)` sends a text message to a specific user or
  channel. The adapter handles platform-specific formatting, message chunking
  for length limits, and retry on transient failures.
- `broadcast(message)` sends a message to all configured recipients. Used
  for system notifications and status updates.
- `send_approval(recipient, request)` sends an approval request with
  interactive buttons (approve/deny). Each platform renders this differently,
  but the semantic meaning is the same.
- `stop()` gracefully shuts down the adapter, closing connections and
  cleaning up resources.

A central ChannelDispatcher manages all registered adapters. When the agent
needs to send a message, it tells the dispatcher, and the dispatcher routes
it to the appropriate adapter (or all adapters for broadcasts). Incoming
messages from any channel flow through the dispatcher into the event bus as
MessageReceived events.

The dispatcher also handles common concerns:

- **Message chunking.** Long responses are split at natural boundaries
  (paragraph breaks, code block boundaries) before being passed to the
  adapter. Each platform has different length limits (Telegram: 4096 chars,
  Discord: 2000 chars), and the chunking respects those limits.
- **Approval routing.** When the agent requests approval, the dispatcher
  sends it to the user's preferred channel and tracks which channel the
  response comes from.
- **Unified message format.** Internally, messages use a simple format with
  plain text and optional code blocks. Each adapter converts this to its
  platform's native format (Markdown for Telegram, Slack blocks for Slack,
  embeds for Discord).

## Consequences

**What went well:**

- Adding a new channel is a focused task. You implement six methods and
  register the adapter. The Telegram adapter is about 400 lines. The Discord
  adapter is similar. No changes needed to the agent loop, the dispatcher,
  or any other adapter.
- The approval UI works consistently across platforms. Whether a user is on
  Telegram or the web UI, they see the same approval request and can respond
  the same way. This is important for the passthrough security model where
  optional approval checkpoints need to be reachable from anywhere.
- Message chunking is handled once and works everywhere. A 10,000-character
  agent response gets split appropriately for each platform without the agent
  or the LLM needing to know about length limits.
- The dispatcher makes multi-channel setups natural. A user can receive
  notifications on Telegram and do detailed work through the web UI. The
  agent does not need to know or care which channels are active.

**What is harder:**

- Lowest common denominator problem. The trait interface captures what all
  platforms can do, but not what each platform does uniquely. Telegram's
  inline keyboards, Slack's interactive blocks, and Discord's rich embeds
  are all more powerful than our standardized interface. We lose that
  richness by standardizing.
- Platform-specific features require escape hatches. Using Slack threads or
  Telegram reply keyboards means extending the trait (affecting all adapters)
  or adding conditional platform-specific methods. Neither is clean.
- Testing requires mock platform APIs. Each adapter needs its own test
  infrastructure that simulates webhook or polling behavior.
- Connection management varies wildly. Telegram uses long-polling, Slack uses
  Socket Mode WebSocket, Discord uses a Gateway WebSocket with heartbeat.
  Connection bugs are adapter-specific and hard to generalize about.

The trait pattern is the right abstraction for our needs. It keeps the core
simple and makes the common case (send a message, receive a message, handle
an approval) easy. The platform-specific richness that we lose is a reasonable
tradeoff for the ability to support many channels without the codebase growing
linearly with each one.
