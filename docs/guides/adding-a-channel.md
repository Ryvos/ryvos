# Adding a channel

## When to use this guide

A **[channel adapter](../glossary.md#channel-adapter)** bridges Ryvos to an
external messaging platform. The four built-in adapters — Telegram,
Discord, Slack, and WhatsApp — each implement the `ChannelAdapter` trait
from `ryvos-core` and live in
`crates/ryvos-channels/src/`. Add a new adapter when you want the agent
reachable from a platform Ryvos does not yet speak (Matrix, iMessage,
XMPP, Microsoft Teams, Signal, Mastodon, SMS, and so on).

ADR-010 records the pattern's rationale: every adapter implements the
same five-method trait, the `ChannelDispatcher` multiplexes inbound
messages into a single mpsc stream that feeds the `AgentRuntime`, and
approval broker plus session manager are shared across all adapters.
Adding a fifth adapter is a single new file in
`crates/ryvos-channels/src/` plus a branch in the daemon's startup code.
None of the existing adapters is privileged relative to a new one.

The crate-level reference is
[`ryvos-channels`](../crates/ryvos-channels.md). The dispatcher, pairing
manager, and message-chunking helper are covered there. This guide walks
the nine-step workflow for a new adapter from blank file to a running
integration.

## The `ChannelAdapter` trait

The trait is defined in `crates/ryvos-core/src/traits.rs` and has five
methods, three required and two with defaults:

```rust
pub trait ChannelAdapter: Send + Sync + 'static {
    fn name(&self) -> &str;

    fn start(
        &self,
        tx: mpsc::Sender<MessageEnvelope>,
    ) -> BoxFuture<'_, Result<()>>;

    fn send(
        &self,
        session: &SessionId,
        content: MessageContent,
    ) -> BoxFuture<'_, Result<()>>;

    fn send_approval(
        &self,
        session: &SessionId,
        request: ApprovalRequest,
    ) -> BoxFuture<'_, Result<bool>> { /* default: Ok(false) */ }

    fn broadcast(&self, content: MessageContent) -> BoxFuture<'_, Result<()>> {
        /* default: no-op */
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>>;
}
```

The `BoxFuture` return type exists because the dispatcher stores
adapters as `Arc<dyn ChannelAdapter>` and async functions in traits are
not yet usable through trait objects at the workspace's MSRV. Every
adapter is `Send + Sync + 'static` so it can be shared across tokio
tasks.

## Step-by-step workflow

1. **Pick a transport model.** Platforms fall into three buckets.
   **Long-poll** platforms (Telegram via teloxide) expose a library
   that drives a background loop. **Socket** platforms (Slack Socket
   Mode, Discord Gateway) need a reconnecting WebSocket loop. **Webhook**
   platforms (WhatsApp Cloud API) have no client connection at all — the
   gateway route receives POSTs and the adapter's job is to register a
   sink. Read the platform's API docs and decide which bucket applies
   before writing any code.

2. **Create the file.** Add
   `crates/ryvos-channels/src/your_platform.rs` and a
   `pub mod your_platform;` line to
   `crates/ryvos-channels/src/lib.rs`. The struct holds whatever the
   platform's SDK needs plus the shared state the trait needs:

   ```rust
   pub struct YourAdapter {
       client: Arc<PlatformSdk>,
       chat_map: Arc<Mutex<HashMap<SessionId, ChatId>>>,
       allowed_users: Vec<String>,
       dm_policy: DmPolicy,
       shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
       approval_broker: Arc<ApprovalBroker>,
   }
   ```

   `chat_map` is what lets `send()` route a response back to the right
   conversation. `shutdown_tx` is a oneshot channel that `stop()` fires
   to unwind the background task. `approval_broker` comes from
   `ryvos-agent` and is shared across every adapter.

3. **Implement `name`.** Return a stable short string like `"matrix"`
   that the dispatcher uses as the lookup key and the audit trail uses
   as the channel identifier. It must not collide with an existing
   adapter's name.

4. **Implement `start`.** Validate credentials, spin up the background
   task, and return. The long-running work happens in a spawned tokio
   task — the function itself should return as soon as the adapter is
   ready to receive messages.

   ```rust
   fn start(&self, tx: mpsc::Sender<MessageEnvelope>)
       -> BoxFuture<'_, Result<()>>
   {
       Box::pin(async move {
           self.validate_credentials().await?;
           let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
           *self.shutdown_tx.lock().unwrap() = Some(shutdown_tx);

           tokio::spawn(async move {
               loop {
                   tokio::select! {
                       _ = &mut shutdown_rx => break,
                       msg = platform.next_message() => { /* handle */ }
                   }
               }
           });
           Ok(())
       })
   }
   ```

5. **Route inbound messages.** Inside the background task, for each
   incoming platform message:
   - Enforce the **[DM policy](../glossary.md#channel-adapter)** first.
     `DmPolicy::Disabled` silently drops every message. `DmPolicy::Allowlist`
     drops messages from users whose platform ID is not in
     `allowed_users`. `DmPolicy::Open` lets everything through. The
     policy check runs before the message reaches the dispatcher so an
     unauthorized sender never consumes tokens or shows up in the audit
     trail.
   - Build the `session_key`. The convention is
     `{platform}:{scope}:{user_id}` — Telegram uses `telegram:user:12345`,
     Discord uses `discord:channel:{cid}:user:{uid}` (per channel and
     user so contexts do not leak across rooms), Slack uses
     `slack:user:U01ABCDEF`. Pick a scheme that isolates conversations
     the way your platform expects.
   - Construct a `MessageEnvelope` with the session key, the sender
     metadata, the raw text or `MessageContent::Text`, and push it into
     the `mpsc::Sender<MessageEnvelope>` handed to `start`.
   - Record the chat ID in `chat_map` so `send()` can route responses
     back.

6. **Implement `send`.** Look up the `ChatId` (or the platform
   equivalent) in `chat_map`, chunk the content against the platform's
   per-message length limit using
   `crates/ryvos-channels/src/util.rs`'s `split_message(text, max_len)`,
   and dispatch each chunk through the platform's outbound API.
   Telegram's limit is 4096, Discord's is 2000, Slack's is 4000,
   WhatsApp's is 4096 — look up the new platform's limit in its API
   docs and hardcode it at the call site.

   ```rust
   fn send(&self, session: &SessionId, content: MessageContent)
       -> BoxFuture<'_, Result<()>>
   {
       Box::pin(async move {
           let chat_id = self.chat_map.lock().unwrap()
               .get(session).cloned()
               .ok_or_else(|| RyvosError::Channel("unknown session".into()))?;
           for chunk in split_message(&content.text(), PLATFORM_MAX_LEN) {
               self.client.send_message(chat_id, chunk).await?;
           }
           Ok(())
       })
   }
   ```

7. **Implement `send_approval`.** The platform-specific approval UI is
   the most rewarding part of the adapter to get right. Each built-in
   adapter picks an idiom that fits its platform:
   - **Telegram** sends an `InlineKeyboardMarkup` with two buttons
     labeled Approve and Deny, carrying callback data of the form
     `approve:<request_id>` and `deny:<request_id>`. A second handler
     catches the callback query, looks up the broker, and calls
     `respond()`.
   - **Discord** sends a `CreateActionRow` with two `CreateButton`s
     using `ButtonStyle::Success` and `ButtonStyle::Danger`. The custom
     ID is the same `approve:<id>` / `deny:<id>` format. The
     `interaction_create` handler resolves the click.
   - **Slack** sends Block Kit blocks with `button` elements. The
     `interactive` payload arrives through the socket and the adapter
     matches the `action_id` against `approve:<id>` / `deny:<id>`.
   - **WhatsApp** sends an interactive message with button replies.
     Incoming replies arrive through the same webhook as regular
     messages.

   If the adapter cannot render a native button (no chat ID mapped yet,
   SDK error, platform-specific rate limit), return `Ok(false)` and the
   dispatcher falls back to a plain-text `/approve <prefix>` prompt
   that the user can reply to manually.

8. **Implement `broadcast` and `stop`.** `broadcast` delivers the same
   content to every user in `allowed_users` (or is a no-op for `Open`
   policy adapters that have no durable user list). `stop` fires the
   oneshot shutdown sender and awaits the background task's exit.

9. **Register the adapter.** Wire the new adapter into `main.rs` of the
   `ryvos` binary next to the four existing `add_adapter` calls. Read
   a new sub-config from `AppConfig::channels` — add a
   `your_platform: Option<YourPlatformConfig>` field to `ChannelsConfig`
   in `ryvos-core` if the platform needs its own TOML section, or reuse
   an existing generic shape if the config is a token plus a DM policy.
   The daemon's channel dispatcher only lights up adapters whose config
   is `Some`, so a disabled platform is a no-op.

   A minimal TOML section for a new adapter:

   ```toml
   [channels.matrix]
   homeserver_url = "https://matrix.org"
   access_token = "${MATRIX_ACCESS_TOKEN}"
   dm_policy = "allowlist"
   allowed_users = ["@me:matrix.org"]
   ```

## Session key conventions

The `session_key` is the only string the dispatcher uses to route an
inbound message to a `SessionId`. Two messages with the same session
key go to the same session; two with different keys go to different
sessions. The convention matters because the agent's memory and audit
trail are scoped to the session. Follow the existing adapters:

- One-user DM platforms (Telegram) use `platform:user:{id}`.
- Multi-channel platforms (Discord, Slack) use
  `platform:channel:{cid}:user:{uid}` so a user's DM is a different
  session from the same user's conversation in a guild or workspace
  channel.
- Group-chat platforms may want `platform:group:{gid}` if the agent
  should reason about the group as a single conversation.

The session manager maps the key to a stable `SessionId` the first
time it sees the key; subsequent messages with the same key resolve to
the same session without re-creating state.

## Verification

1. Build with `cargo build --release` and run `ryvos daemon`. Startup
   logs should show the adapter registering alongside the existing
   ones.
2. Send a message from the platform. The dispatcher routes it through
   the session manager into `AgentRuntime::run`, the agent streams a
   response, and the adapter's `send` puts the reply back in the
   platform. Watch the logs and the `audit.db` entries.
3. Trigger an approval: set `security.pause_before = ["bash"]`, ask
   the agent to run a bash command, and confirm that `send_approval`
   renders the platform's native UI. Click Approve and the tool
   proceeds.
4. Test DM policy: flip `dm_policy` to `"allowlist"` and send from a
   platform account not in `allowed_users`. The message should be
   silently dropped — no tokens consumed, no audit entry, no response.
5. Test `stop`: kill the daemon with SIGINT. The shutdown token fires,
   the adapter's background task exits, and the platform connection
   closes cleanly.

For the approval broker internals, the pairing manager, and the
dispatcher's event-forwarding path that routes heartbeat and cron
results back through adapters, read
[../crates/ryvos-channels.md](../crates/ryvos-channels.md). For the
session manager's per-channel isolation guarantees, read
[../internals/session-manager.md](../internals/session-manager.md). For
the pattern rationale, read
[../adr/010-channel-adapter-pattern.md](../adr/010-channel-adapter-pattern.md).
