use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};

use ryvos_agent::{ApprovalBroker, SessionManager};
use ryvos_core::config::{DiscordConfig, DmPolicy};
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::{ApprovalDecision, ApprovalRequest};
use ryvos_core::traits::ChannelAdapter;
use ryvos_core::types::{MessageContent, MessageEnvelope, SessionId};

use serenity::all::{
    ButtonStyle, Context, CreateActionRow, CreateButton, CreateInteractionResponse,
    CreateInteractionResponseMessage, CreateMessage, EventHandler, GatewayIntents, Interaction,
    Ready,
};
use serenity::model::channel::Message;
use serenity::model::id::ChannelId;
use serenity::prelude::TypeMapKey;
use serenity::Client;

use crate::util::split_message;

const DISCORD_MAX_LEN: usize = 2000;

/// Typed keys for serenity's TypeMap.
struct EnvelopeSender;
impl TypeMapKey for EnvelopeSender {
    type Value = mpsc::Sender<MessageEnvelope>;
}
struct SessionMgrKey;
impl TypeMapKey for SessionMgrKey {
    type Value = Arc<SessionManager>;
}
struct ChannelMapKey;
impl TypeMapKey for ChannelMapKey {
    type Value = Arc<Mutex<HashMap<String, ChannelId>>>;
}
struct HttpKey;
impl TypeMapKey for HttpKey {
    type Value = Arc<serenity::http::Http>;
}
struct DmPolicyKey;
impl TypeMapKey for DmPolicyKey {
    type Value = DmPolicy;
}
struct AllowedUsersKey;
impl TypeMapKey for AllowedUsersKey {
    type Value = Vec<u64>;
}
struct ApprovalBrokerKey;
impl TypeMapKey for ApprovalBrokerKey {
    type Value = Arc<ApprovalBroker>;
}

struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore bot messages
        if msg.author.bot {
            return;
        }

        let data = ctx.data.read().await;
        let tx = match data.get::<EnvelopeSender>() {
            Some(t) => t.clone(),
            None => return,
        };
        let session_mgr = match data.get::<SessionMgrKey>() {
            Some(s) => s.clone(),
            None => return,
        };
        let channel_map = match data.get::<ChannelMapKey>() {
            Some(c) => c.clone(),
            None => return,
        };
        let dm_policy = data.get::<DmPolicyKey>().cloned().unwrap_or_default();
        let allowed_users = data.get::<AllowedUsersKey>().cloned().unwrap_or_default();
        drop(data);

        // Enforce DM policy
        match dm_policy {
            DmPolicy::Disabled => return,
            DmPolicy::Allowlist => {
                if !allowed_users.is_empty() && !allowed_users.contains(&msg.author.id.get()) {
                    return;
                }
            }
            DmPolicy::Open => {}
        }

        let key = format!("discord:channel:{}:user:{}", msg.channel_id, msg.author.id);
        let session_id = session_mgr.get_or_create(&key, "discord");

        // Map session -> channel for response routing
        channel_map
            .lock()
            .await
            .insert(session_id.0.clone(), msg.channel_id);

        let envelope = MessageEnvelope {
            id: uuid::Uuid::new_v4().to_string(),
            session_id,
            channel: "discord".into(),
            sender: msg.author.id.to_string(),
            text: msg.content.clone(),
            timestamp: chrono::Utc::now(),
        };

        if let Err(e) = tx.send(envelope).await {
            error!(error = %e, "Failed to forward discord message");
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(user = %ready.user.name, "Discord bot connected");
        // Store HTTP client for send()
        let mut data = ctx.data.write().await;
        data.insert::<HttpKey>(Arc::clone(&ctx.http));
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Component(component) = interaction {
            let custom_id = component.data.custom_id.as_str();

            let (action, request_id) = match custom_id.split_once(':') {
                Some((a, id)) if a == "approve" || a == "deny" => (a, id.to_string()),
                _ => return,
            };

            let data = ctx.data.read().await;
            let broker = match data.get::<ApprovalBrokerKey>() {
                Some(b) => b.clone(),
                None => {
                    warn!("Discord interaction but no broker configured");
                    return;
                }
            };
            drop(data);

            let decision = if action == "approve" {
                ApprovalDecision::Approved
            } else {
                ApprovalDecision::Denied {
                    reason: "denied via Discord".to_string(),
                }
            };

            let resolved = broker.respond(&request_id, decision).await;

            let label = if action == "approve" {
                "Approved"
            } else {
                "Denied"
            };
            let response_text = if resolved {
                format!("{} successfully.", label)
            } else {
                "Request expired or not found.".to_string()
            };

            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(response_text)
                    .ephemeral(true),
            );

            if let Err(e) = component.create_response(&ctx.http, response).await {
                warn!(error = %e, "Failed to respond to Discord interaction");
            }
        }
    }
}

/// Discord channel adapter using serenity gateway.
pub struct DiscordAdapter {
    config: DiscordConfig,
    session_mgr: Arc<SessionManager>,
    /// Maps SessionId -> ChannelId for routing responses back.
    channel_map: Arc<Mutex<HashMap<String, ChannelId>>>,
    /// Shared HTTP client set after the bot connects.
    http: Arc<Mutex<Option<Arc<serenity::http::Http>>>>,
    /// Shard manager for shutdown.
    shard_manager: Arc<Mutex<Option<Arc<serenity::gateway::ShardManager>>>>,
    /// Approval broker for HITL.
    broker: Arc<Mutex<Option<Arc<ApprovalBroker>>>>,
}

impl DiscordAdapter {
    pub fn new(config: DiscordConfig, session_mgr: Arc<SessionManager>) -> Self {
        Self {
            config,
            session_mgr,
            channel_map: Arc::new(Mutex::new(HashMap::new())),
            http: Arc::new(Mutex::new(None)),
            shard_manager: Arc::new(Mutex::new(None)),
            broker: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the approval broker for HITL buttons.
    pub fn set_broker(&mut self, broker: Arc<ApprovalBroker>) {
        self.broker = Arc::new(Mutex::new(Some(broker)));
    }
}

impl ChannelAdapter for DiscordAdapter {
    fn name(&self) -> &str {
        "discord"
    }

    fn start(&self, tx: mpsc::Sender<MessageEnvelope>) -> BoxFuture<'_, Result<()>> {
        let token = self.config.bot_token.clone();
        let session_mgr = self.session_mgr.clone();
        let channel_map = self.channel_map.clone();
        let http_slot = self.http.clone();
        let shard_slot = self.shard_manager.clone();
        let broker_slot = self.broker.clone();

        Box::pin(async move {
            let intents = GatewayIntents::GUILD_MESSAGES
                | GatewayIntents::DIRECT_MESSAGES
                | GatewayIntents::MESSAGE_CONTENT;

            let mut client = Client::builder(&token, intents)
                .event_handler(Handler)
                .await
                .map_err(|e| RyvosError::Channel {
                    channel: "discord".into(),
                    message: e.to_string(),
                })?;

            // Inject shared state into serenity's TypeMap
            {
                let mut data = client.data.write().await;
                data.insert::<EnvelopeSender>(tx);
                data.insert::<SessionMgrKey>(session_mgr);
                data.insert::<ChannelMapKey>(channel_map);
                data.insert::<DmPolicyKey>(self.config.dm_policy.clone());
                data.insert::<AllowedUsersKey>(self.config.allowed_users.clone());
                if let Some(broker) = broker_slot.lock().await.clone() {
                    data.insert::<ApprovalBrokerKey>(broker);
                }
            }

            // Store shard manager for shutdown
            *shard_slot.lock().await = Some(client.shard_manager.clone());

            // Store HTTP for send()
            *http_slot.lock().await = Some(Arc::clone(&client.http));

            info!("Discord adapter starting");

            tokio::spawn(async move {
                if let Err(e) = client.start().await {
                    error!(error = %e, "Discord client error");
                }
            });

            Ok(())
        })
    }

    fn send_approval(
        &self,
        session: &SessionId,
        request: &ApprovalRequest,
    ) -> BoxFuture<'_, Result<bool>> {
        let session_key = session.0.clone();
        let channel_map = self.channel_map.clone();
        let http_slot = self.http.clone();
        let request_id = request.id.clone();
        let tool_name = request.tool_name.clone();
        let tier = request.tier;
        let input_summary = request.input_summary.clone();

        Box::pin(async move {
            let channel_id = {
                let map = channel_map.lock().await;
                map.get(&session_key).copied()
            };

            let channel_id = match channel_id {
                Some(id) => id,
                None => return Ok(false),
            };

            let http_guard = http_slot.lock().await;
            let http = match http_guard.as_ref() {
                Some(h) => h,
                None => return Ok(false),
            };

            let text = format!(
                "🔐 **Approval Required**\n\nTool: `{}`\nTier: {}\nAction: *{}*",
                tool_name, tier, input_summary
            );

            let approve_btn = CreateButton::new(format!("approve:{}", request_id))
                .label("Approve")
                .style(ButtonStyle::Success);
            let deny_btn = CreateButton::new(format!("deny:{}", request_id))
                .label("Deny")
                .style(ButtonStyle::Danger);
            let action_row = CreateActionRow::Buttons(vec![approve_btn, deny_btn]);

            let msg = CreateMessage::new()
                .content(text)
                .components(vec![action_row]);

            match channel_id.send_message(http, msg).await {
                Ok(_) => Ok(true),
                Err(e) => {
                    warn!(error = %e, "Failed to send approval to Discord");
                    Ok(false)
                }
            }
        })
    }

    fn send(&self, session: &SessionId, content: &MessageContent) -> BoxFuture<'_, Result<()>> {
        let session_key = session.0.clone();
        let content = content.clone();
        let channel_map = self.channel_map.clone();
        let http_slot = self.http.clone();

        Box::pin(async move {
            let text = match &content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Streaming { delta, done: _ } => delta.clone(),
            };

            if text.is_empty() {
                return Ok(());
            }

            let channel_id = {
                let map = channel_map.lock().await;
                map.get(&session_key).copied()
            };

            let channel_id = channel_id.ok_or_else(|| RyvosError::Channel {
                channel: "discord".into(),
                message: format!("No channel mapped for session {}", session_key),
            })?;

            let http_guard = http_slot.lock().await;
            let http = http_guard.as_ref().ok_or_else(|| RyvosError::Channel {
                channel: "discord".into(),
                message: "Bot not started".into(),
            })?;

            let chunks = split_message(&text, DISCORD_MAX_LEN);
            for chunk in chunks {
                channel_id
                    .say(http, &chunk)
                    .await
                    .map_err(|e| RyvosError::Channel {
                        channel: "discord".into(),
                        message: e.to_string(),
                    })?;
            }

            Ok(())
        })
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>> {
        let shard_slot = self.shard_manager.clone();

        Box::pin(async move {
            if let Some(manager) = shard_slot.lock().await.take() {
                manager.shutdown_all().await;
            }
            info!("Discord adapter stopped");
            Ok(())
        })
    }
}
