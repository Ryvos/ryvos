# ryvos-channels

`ryvos-channels` contains the four built-in messaging adapters that let users
talk to Ryvos from platforms other than the Web UI or the terminal: Telegram,
Discord, Slack, and WhatsApp. Every adapter implements the
`ChannelAdapter` trait from `ryvos-core`, translates between the platform's
native message format and Ryvos's `MessageEnvelope`/`MessageContent`
vocabulary, and handles per-platform idioms for approval buttons, message
chunking, and user access control. A thin `ChannelDispatcher` on top
multiplexes all four adapters into a single mpsc stream that feeds the
`AgentRuntime`.

The crate follows ADR-010, the **[channel adapter](../glossary.md#channel-adapter)**
pattern. Every adapter exposes the same five methods (`name`, `start`, `send`,
`broadcast`, `send_approval`, `stop`) behind a single trait so that adding a
fifth platform is a single new file in `crates/ryvos-channels/src/` plus a
branch in the daemon's startup code. None of the four built-in adapters are
privileged over a future addition; they share the dispatcher, the approval
broker, the session manager, the pairing manager, and the message-chunking
utility.

## The ChannelAdapter trait

Every adapter in this crate implements the `ChannelAdapter` trait defined
in `crates/ryvos-core/src/traits.rs`. The trait is intentionally small:

- `name(&self) -> &str` returns a stable short string like `"telegram"`
  or `"slack"` that the dispatcher uses as the lookup key into its
  adapter map.
- `start(&self, tx: mpsc::Sender<MessageEnvelope>) -> BoxFuture<Result<()>>`
  starts the adapter's background work (a long-poll loop, a socket, a
  webhook sink) and arranges for inbound messages to be pushed into the
  sender. The call must return once the adapter is running; any long
  work belongs in a spawned tokio task.
- `send(&self, session, content) -> BoxFuture<Result<()>>` delivers an
  outbound message for a specific session. The adapter is expected to
  chunk the content against its platform's length limit before
  dispatching.
- `broadcast(&self, content) -> BoxFuture<Result<()>>` delivers the same
  content to every user the adapter considers a recipient. The four
  built-in adapters broadcast to users listed in their `allowed_users`
  config; an `Open` policy adapter broadcasts to nobody because there is
  no durable user list to iterate.
- `send_approval(&self, session, request) -> BoxFuture<Result<bool>>`
  renders a platform-native approval UI and returns `true` on success.
  A `false` return means the adapter could not deliver a native prompt
  (no chat ID mapped yet, bot token invalidated, and so on) and the
  dispatcher should fall back to a plain-text prompt.
- `stop(&self) -> BoxFuture<Result<()>>` tears down whatever `start`
  brought up. The four built-in adapters implement this by firing a
  oneshot shutdown channel that their background task is selecting on.

All methods return `BoxFuture` rather than `async fn` because the
dispatcher stores adapters as `Arc<dyn ChannelAdapter>` and Rust's
`async fn` in traits cannot be used through trait objects at the current
MSRV. Every adapter is `Send + Sync + 'static` so that the dispatcher
can share it across tokio tasks without copying.

## Position in the stack

`ryvos-channels` sits in the integration layer alongside `ryvos-gateway`,
`ryvos-skills`, and `ryvos-tui`. It depends directly on `ryvos-core` for the
`ChannelAdapter` trait, `MessageEnvelope`, `MessageContent`, `SessionId`, and
the `DmPolicy` enum; on `ryvos-agent` for `AgentRuntime`, `ApprovalBroker`,
and `SessionManager`; and on `ryvos-memory` for the `SessionMetaStore` used
to persist CLI-provider session IDs across messages.

The external dependency list is deliberately minimal:
- `teloxide` 0.13 for Telegram (long-polling, inline keyboards, MarkdownV2).
- `serenity` 0.12 for Discord (gateway client, typed intents, component
  interactions).
- `tokio-tungstenite` plus `reqwest` for Slack's Socket Mode over WebSocket
  and the Web API.
- `reqwest` alone for WhatsApp's Meta Cloud API — WhatsApp has no long-poll
  or socket transport.

See `crates/ryvos-channels/Cargo.toml` for the exact versions.

## ChannelDispatcher

`ChannelDispatcher` in `crates/ryvos-channels/src/dispatch.rs:34` is the
glue that turns a bag of `Arc<dyn ChannelAdapter>` implementations into a
running subsystem. It owns the `AgentRuntime`, the `EventBus`, a
`CancellationToken`, the adapter map, an optional `HooksConfig`, an optional
`ApprovalBroker`, and an optional `SessionMetaStore`. The daemon's startup
code constructs one dispatcher per process, calls `add_adapter` for each
configured platform, wires the hooks, broker, and session meta store with
the `set_*` methods, and then calls `run()`.

`run` does four things in sequence and then enters its event loop:

1. Creates a 256-slot mpsc channel that all adapters will push
   `MessageEnvelope`s into.
2. Calls `adapter.start(tx)` on every registered adapter, handing each one
   a clone of the sender. An adapter that fails to start (bad token, network
   error, wrong scopes) logs an error and is skipped — the dispatcher keeps
   running for the remaining platforms.
3. Fires the `on_start` lifecycle hook if configured.
4. Spawns a background task that subscribes to the EventBus and forwards
   three event types to adapters: `HeartbeatAlert` (routed to the specific
   `target_channel` if set, broadcast otherwise), `HeartbeatOk` (always
   broadcast), and `CronJobComplete` (routed to the cron job's configured
   channel if set, broadcast otherwise). This is how a heartbeat finding or
   a finished cron job reaches a user's phone without any of the Guardian,
   **[Heartbeat](../glossary.md#heartbeat)**, or cron code knowing that
   Telegram or Slack exists. See [../internals/heartbeat.md](../internals/heartbeat.md)
   for the publisher side.

The main loop then alternates between the cancellation token (for graceful
shutdown) and the mpsc receiver. For every incoming envelope, the
dispatcher first checks whether the text starts with `/approve ` or
`/deny ` — if so, it hands the envelope to `handle_approval_command` and
does not run the agent. Otherwise, it spawns a per-message tokio task that
runs the agent, streams events through the EventBus, and sends the final
response back through the originating adapter.

Per-message execution is handled by `run_channel_message`. It:

- Fires `on_session_start` and `on_message` hooks.
- Restores a previous CLI-provider session ID from `SessionMetaStore` if one
  exists for the envelope's `session_key`, so that the Claude Code CLI or
  the Copilot CLI can `--resume` the same conversation instead of starting
  fresh.
- Subscribes to the EventBus before kicking off the run so that every text
  delta is captured from the very first token.
- Spawns the agent in a background task and collects `TextDelta` events
  into a single response string until it sees `RunComplete` or `RunError`
  for this session.
- Forwards `ApprovalRequested` events to the adapter via `send_approval`;
  if the adapter cannot render a native button (for example, the Telegram
  chat ID has not been seen yet), it falls back to a text prompt telling
  the user to reply with `/approve <short-id>` or `/deny <short-id>`.
- Forwards `ToolBlocked` events as plain-text warnings.
- Fires the `on_tool_call`, `on_tool_error`, and `on_turn_complete` hooks
  as events arrive.
- Persists the CLI provider's new session ID back to `SessionMetaStore`
  after the run finishes.
- Fires `on_response` and sends the collected text through
  `adapter.send()`.
- Fires `on_session_end`.

The approval command handler, `handle_approval_command`, parses the
`/approve <prefix>` or `/deny <prefix> [reason]` form, looks up the full
approval ID via `ApprovalBroker::find_by_prefix`, and calls
`ApprovalBroker::respond` with the appropriate `ApprovalDecision`. Success
and failure both produce a one-line confirmation message back in the same
channel.

## Message chunking

Every platform imposes a different per-message length limit: Telegram caps
text messages at 4096 characters, Discord at 2000, Slack at 4000, and
WhatsApp at 4096. Instead of re-implementing chunking in every adapter, the
crate ships a single `split_message(text, max_len)` helper in
`crates/ryvos-channels/src/util.rs` that every adapter calls before sending.

The splitter prefers newline boundaries: it walks the input line by line and
flushes the current chunk whenever adding the next line would exceed the
limit. If a single line is itself longer than the limit — a long log tail
or a minified JSON blob — the function hard-splits the line at the byte
limit and emits each fragment as its own chunk. Empty input returns a
single empty chunk, so adapters can unconditionally call the splitter and
iterate without special-casing the empty case. The unit tests in the same
file cover the corner cases: exactly-at-limit, one-byte-over, unicode
multibyte characters, consecutive newlines, and newline-only input.

## TelegramAdapter

`TelegramAdapter` in `crates/ryvos-channels/src/telegram.rs:24` wraps a
`teloxide::Bot`. On `start`, it validates the bot token by calling `get_me`
and bailing out with a `RyvosError::Config` if the call fails. Bot instance,
a `chat_map` from `SessionId` to `ChatId`, and a oneshot shutdown channel
are stored in `Arc<Mutex<...>>` fields so the spawned dispatcher task can
access them.

Inbound messages are processed by a teloxide `Update::filter_message()`
endpoint. The handler enforces the adapter's `DmPolicy` first: `Disabled`
silently drops every message, `Allowlist` drops messages from users whose
Telegram user ID is not in `config.allowed_users` (Telegram user IDs are
`i64`), and `Open` lets everything through. The per-user session key is
`telegram:user:{id}`, which the `SessionManager` maps to a stable
`SessionId`; the chat ID is stored in `chat_map` so that `send()` can route
responses back to the right conversation.

Approval requests are rendered as an `InlineKeyboardMarkup` with two
buttons: "Approve" sends callback data `approve:<request_id>`, "Deny"
sends `deny:<request_id>`. The message text is formatted with MarkdownV2
and emoji for visual emphasis. If MarkdownV2 parsing fails (Telegram's
parser is strict about escape rules), the adapter falls back to a plain-text
message with the same two buttons and no markdown. A second teloxide
endpoint, `Update::filter_callback_query()`, decodes the callback data,
looks up the shared `ApprovalBroker`, calls `respond()` with the
corresponding `ApprovalDecision`, answers the callback to clear the
button's loading spinner, and edits the original message to reflect the
decision.

The dispatcher task runs teloxide's `Dispatcher` in a `tokio::select!`
against the oneshot shutdown channel so that `stop()` can cleanly unwind
the long-poll loop.

## DiscordAdapter

`DiscordAdapter` in `crates/ryvos-channels/src/discord.rs` uses serenity
0.12 and its typed `GatewayIntents`. The intents requested are
`GUILD_MESSAGES | DIRECT_MESSAGES | MESSAGE_CONTENT` — enough to read user
messages in both DMs and guild channels. The adapter does not request
`GUILD_MEMBERS` or presence intents, which keeps the bot compatible with
the large-bot verification threshold on Discord.

Shared state is handed to serenity's `EventHandler` through its `TypeMap`
with a set of typed keys: `EnvelopeSender`, `SessionMgrKey`, `ChannelMapKey`,
`HttpKey`, `DmPolicyKey`, `AllowedUsersKey`, and `ApprovalBrokerKey`. Each
key implements `TypeMapKey` so that the handler can fetch its dependencies
without cloning an `Arc` for every closure. Discord's user IDs are `u64`,
so the allowlist type here is `Vec<u64>` rather than the Telegram adapter's
`Vec<i64>`.

The `message` handler ignores bot authors, enforces the DM policy, builds a
session key of the form `discord:channel:{channel_id}:user:{user_id}`, and
pushes a `MessageEnvelope` into the dispatcher's mpsc channel. The
session key is per-channel-and-user on purpose: a user who DMs the bot
gets a different session from the same user in a guild channel, so
contexts cannot leak across rooms.

Approvals are rendered as a `CreateActionRow` with two `CreateButton`s:
"Approve" uses `ButtonStyle::Success` and a custom ID of
`approve:<request_id>`, "Deny" uses `ButtonStyle::Danger` and `deny:<request_id>`.
The `interaction_create` handler receives the click as an
`Interaction::Component`, parses the custom ID, resolves the broker from
the `TypeMap`, calls `respond()`, and sends an ephemeral interaction
response so the acknowledgement is only visible to the person who clicked.

## SlackAdapter

`SlackAdapter` in `crates/ryvos-channels/src/slack.rs` uses Slack's
Socket Mode rather than the webhook-based Events API. On `start`, the
adapter spawns a task that runs a reconnect loop:

1. Call `apps.connections.open` with the app-level token as a bearer to
   obtain a WebSocket URL.
2. Dial the URL with `tokio_tungstenite::connect_async`.
3. Read frames in a `tokio::select!` against the oneshot shutdown channel.
4. On any WebSocket error, close frame, or explicit `disconnect` envelope
   from Slack, break the inner loop, sleep one second, and reconnect. If
   the initial `apps.connections.open` or `connect_async` fails, the loop
   sleeps five seconds before retrying.

This keeps the adapter resilient to Slack's scheduled socket rotations
without any external supervision. Every inbound envelope is immediately
ACKed by echoing `{"envelope_id": ...}` on the same socket; failure to
ACK causes Slack to retry delivery, which would duplicate messages.

Slack envelopes arrive with a `type` field: `hello` on connect,
`events_api` for messages, `interactive` for Block Kit actions, and
`disconnect` when Slack wants the client to reconnect. The `events_api`
branch filters out bot messages and edits (`bot_id` or `subtype` present),
extracts the user ID, channel ID, and text, and pushes a
`MessageEnvelope` with session key `slack:user:{user_id}`. The
`channel_map` records the channel ID so that `send()` can route responses
via `chat.postMessage`.

The `interactive` branch handles Block Kit button clicks. When the
payload's `type` is `block_actions`, the adapter walks the `actions`
array, matches `action_id` values of the form `approve:<request_id>` or
`deny:<request_id>`, and calls `ApprovalBroker::respond` with the
appropriate decision. Approval messages are sent as Block Kit blocks with
two buttons.

Outbound messages go through `chat.postMessage` with the bot token as a
bearer. `split_message` chunks long replies at the 4000-character limit.

## WhatsAppAdapter

`WhatsAppAdapter` in `crates/ryvos-channels/src/whatsapp.rs` is the only
adapter that is webhook-based rather than connection-based. The Meta Cloud
API does not expose a polling or socket transport; instead, it POSTs
incoming messages to a URL the app owner has registered with Meta, which
in the Ryvos case is `/api/whatsapp/webhook` on the gateway.

On `start`, the adapter stores the dispatcher's mpsc sender in a shared
`webhook_tx` field and returns. It never opens a connection of its own. A
`WhatsAppWebhookHandle` is returned from `webhook_handle()`; the gateway's
`GatewayServer::set_whatsapp_handle` picks it up and installs it on
`AppState`, which lets the webhook routes translate raw HTTP POSTs into
`MessageEnvelope`s and push them through `webhook_tx`. The webhook GET
handshake (`/api/whatsapp/webhook` with `hub.mode=subscribe`) is handled
in the gateway's `whatsapp_verify` route using the configured verify token.

Outbound messages use Meta's Graph API v21.0 at
`https://graph.facebook.com/v21.0/{phone_number_id}/messages` with the
access token as a bearer. The body format is
`{"messaging_product": "whatsapp", "to": <phone>, "type": "text", "text": {"body": <chunk>}}`.
Approval messages are sent as interactive messages with button replies.
Incoming button replies arrive through the same webhook as regular
messages, marked so the adapter can route them to the broker.

Because the adapter has no persistent connection, it has no reconnect
logic and no lifecycle beyond registering its sink. `stop()` is a no-op.

## Pairing manager

`PairingManager` in `crates/ryvos-channels/src/pairing.rs:20` is a small
in-memory helper that the Web UI uses to approve unknown senders who try
to DM the bot from a platform where the user's ID is not yet in the
allowlist. When an unknown user sends a first message, the adapter asks
the pairing manager to mint a code; the code is then displayed in the
Web UI alongside the sender's metadata, and the operator can approve or
deny it.

Codes are eight characters long, drawn from the alphabet
`ABCDEFGHJKMNPQRSTUVWXYZ23456789` — uppercase letters and digits with the
visually ambiguous `0`, `O`, `1`, `I`, and `L` removed, so that a code read
aloud from a terminal cannot be mistyped. Each code carries a
`created_at`, an `expires_at` one hour later, the channel it came from,
the sender ID, and an optional human-readable sender name.

`create_pairing` enforces three invariants before minting: expired codes
are swept from the table, at most three pending codes per channel are
allowed, and a sender who already has a pending code on the same channel
cannot create a second one. All three checks run under a single `Mutex`
so the invariants hold under concurrent callers. `approve(code)` consumes
the code atomically (returning `None` on expiry or a missing code),
`deny(code)` drops it, `list_pending` returns the non-expired codes, and
`find_by_prefix` does a case-insensitive prefix search so the operator
can paste just the first three characters.

## DM policy

Every adapter's config carries a `DmPolicy` enum from `ryvos-core`: `Open`
(everyone can message the bot), `Allowlist` (only user IDs listed in the
adapter's `allowed_users` field may message), or `Disabled` (every message
is silently dropped). The allowlist type is platform-dependent: Telegram
uses `i64`, Discord uses `u64`, and Slack uses `String` (Slack user IDs
are opaque strings like `U01ABCDEF`). The policy check runs inside each
adapter's receive path before any message makes it to the dispatcher, so
an unauthorized sender never reaches the agent runtime, never consumes
tokens, and never shows up in the audit trail.

## Where to go next

For the pattern rationale, read
[ADR-010](../adr/010-channel-adapter-pattern.md). To add a fifth adapter,
follow [../guides/adding-a-channel.md](../guides/adding-a-channel.md) — the
guide walks through the `ChannelAdapter` trait surface and uses Telegram
as the reference implementation. For the publisher side of the heartbeat
and cron events that the dispatcher forwards, read
[../internals/heartbeat.md](../internals/heartbeat.md).
