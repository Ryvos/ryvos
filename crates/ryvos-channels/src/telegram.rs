use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use ryvos_agent::{ApprovalBroker, SessionManager};
use ryvos_core::config::{DmPolicy, TelegramConfig};
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::{ApprovalDecision, ApprovalRequest};
use ryvos_core::traits::ChannelAdapter;
use ryvos_core::types::{MessageContent, MessageEnvelope, SessionId};

use teloxide::prelude::*;
use teloxide::respond;
use teloxide::types::{ChatId, InlineKeyboardButton, InlineKeyboardMarkup};

use crate::util::split_message;

const TELEGRAM_MAX_LEN: usize = 4096;

/// Telegram channel adapter using teloxide long-polling.
pub struct TelegramAdapter {
    config: TelegramConfig,
    session_mgr: Arc<SessionManager>,
    /// Maps SessionId -> ChatId for routing responses back.
    chat_map: Arc<Mutex<HashMap<String, ChatId>>>,
    /// The bot instance, created on start().
    bot: Arc<Mutex<Option<Bot>>>,
    /// Shutdown signal.
    shutdown_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    /// Approval broker for HITL.
    broker: Arc<Mutex<Option<Arc<ApprovalBroker>>>>,
}

impl TelegramAdapter {
    pub fn new(config: TelegramConfig, session_mgr: Arc<SessionManager>) -> Self {
        Self {
            config,
            session_mgr,
            chat_map: Arc::new(Mutex::new(HashMap::new())),
            bot: Arc::new(Mutex::new(None)),
            shutdown_tx: Arc::new(Mutex::new(None)),
            broker: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the approval broker for HITL inline buttons.
    pub fn set_broker(&mut self, broker: Arc<ApprovalBroker>) {
        // Use blocking-safe approach: store directly since we're called before start()
        self.broker = Arc::new(Mutex::new(Some(broker)));
    }
}

impl ChannelAdapter for TelegramAdapter {
    fn name(&self) -> &str {
        "telegram"
    }

    fn start(&self, tx: mpsc::Sender<MessageEnvelope>) -> BoxFuture<'_, Result<()>> {
        let bot = Bot::new(&self.config.bot_token);
        let allowed_users = self.config.allowed_users.clone();
        let dm_policy = self.config.dm_policy.clone();
        let session_mgr = self.session_mgr.clone();
        let chat_map = self.chat_map.clone();
        let bot_arc = self.bot.clone();
        let shutdown_tx_arc = self.shutdown_tx.clone();
        let broker_arc = self.broker.clone();

        Box::pin(async move {
            // Store bot for send()
            *bot_arc.lock().await = Some(bot.clone());

            let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
            *shutdown_tx_arc.lock().await = Some(stop_tx);

            info!("Telegram adapter starting long-poll");

            tokio::spawn(async move {
                let message_handler = {
                    let tx = tx.clone();
                    let allowed_users = allowed_users.clone();
                    let dm_policy = dm_policy.clone();
                    let session_mgr = session_mgr.clone();
                    let chat_map = chat_map.clone();

                    Update::filter_message().endpoint(move |msg: Message, _bot: Bot| {
                        let tx = tx.clone();
                        let allowed = allowed_users.clone();
                        let policy = dm_policy.clone();
                        let sm = session_mgr.clone();
                        let cm = chat_map.clone();

                        async move {
                            let user = match msg.from {
                                Some(ref u) => u,
                                None => return respond(()),
                            };

                            // Enforce DM policy
                            match policy {
                                DmPolicy::Disabled => {
                                    debug!(user_id = user.id.0, "Telegram DMs disabled, ignoring");
                                    return respond(());
                                }
                                DmPolicy::Allowlist => {
                                    if !allowed.is_empty() && !allowed.contains(&(user.id.0 as i64))
                                    {
                                        debug!(
                                            user_id = user.id.0,
                                            "Telegram message from non-allowed user, ignoring"
                                        );
                                        return respond(());
                                    }
                                }
                                DmPolicy::Open => {}
                            }

                            let text = msg.text().unwrap_or("").to_string();
                            if text.is_empty() {
                                return respond(());
                            }

                            let key = format!("telegram:user:{}", user.id.0);
                            let session_id = sm.get_or_create(&key, "telegram");

                            // Map session -> chat for response routing
                            cm.lock().await.insert(session_id.0.clone(), msg.chat.id);

                            let envelope = MessageEnvelope {
                                id: uuid::Uuid::new_v4().to_string(),
                                session_id,
                                channel: "telegram".into(),
                                sender: user.id.0.to_string(),
                                text,
                                timestamp: chrono::Utc::now(),
                            };

                            if let Err(e) = tx.send(envelope).await {
                                error!(error = %e, "Failed to forward telegram message");
                            }

                            respond(())
                        }
                    })
                };

                let callback_handler = {
                    let broker = broker_arc.clone();

                    Update::filter_callback_query().endpoint(move |cq: CallbackQuery, bot: Bot| {
                        let broker = broker.clone();

                        async move {
                            let data = match cq.data {
                                Some(ref d) => d.as_str(),
                                None => return respond(()),
                            };

                            let (action, request_id) = match data.split_once(':') {
                                Some((a, id)) if a == "approve" || a == "deny" => (a, id),
                                _ => return respond(()),
                            };

                            let broker_guard = broker.lock().await;
                            let broker = match broker_guard.as_ref() {
                                Some(b) => b.clone(),
                                None => {
                                    warn!("Telegram callback but no broker configured");
                                    return respond(());
                                }
                            };
                            drop(broker_guard);

                            let decision = if action == "approve" {
                                ApprovalDecision::Approved
                            } else {
                                ApprovalDecision::Denied {
                                    reason: "denied via Telegram".to_string(),
                                }
                            };

                            let label = if action == "approve" {
                                "Approved"
                            } else {
                                "Denied"
                            };

                            let resolved = broker.respond(request_id, decision).await;

                            // Answer callback to remove loading spinner
                            let answer_text = if resolved {
                                label.to_string()
                            } else {
                                "Request expired or not found".to_string()
                            };
                            if let Err(e) =
                                bot.answer_callback_query(&cq.id).text(&answer_text).await
                            {
                                warn!(error = %e, "Failed to answer callback query");
                            }

                            // Edit the original message to reflect the decision
                            if let Some(msg) = cq.message {
                                let chat_id = msg.chat().id;
                                let msg_id = msg.id();
                                let original =
                                    msg.regular_message().and_then(|m| m.text()).unwrap_or("");
                                let updated = format!(
                                    "{}\n\n{} {}",
                                    original,
                                    if resolved { "✓" } else { "⚠" },
                                    answer_text
                                );
                                let _ = bot.edit_message_text(chat_id, msg_id, &updated).await;
                            }

                            respond(())
                        }
                    })
                };

                let handler = dptree::entry()
                    .branch(message_handler)
                    .branch(callback_handler);

                let mut dispatcher =
                    teloxide::dispatching::Dispatcher::builder(bot, handler).build();

                // Run until shutdown signal
                tokio::select! {
                    _ = dispatcher.dispatch() => {
                        info!("Telegram dispatcher exited");
                    }
                    _ = &mut stop_rx => {
                        info!("Telegram adapter received shutdown signal");
                        if let Ok(f) = dispatcher.shutdown_token().shutdown() {
                            f.await;
                        }
                    }
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
        let chat_map = self.chat_map.clone();
        let bot_arc = self.bot.clone();
        let request_id = request.id.clone();
        let tool_name = request.tool_name.clone();
        let tier = request.tier;
        let input_summary = request.input_summary.clone();

        Box::pin(async move {
            let chat_id = {
                let map = chat_map.lock().await;
                map.get(&session_key).copied()
            };

            let chat_id = match chat_id {
                Some(id) => id,
                None => return Ok(false),
            };

            let bot_guard = bot_arc.lock().await;
            let bot = match bot_guard.as_ref() {
                Some(b) => b,
                None => return Ok(false),
            };

            let text = format!(
                "🔐 *Approval Required*\n\nTool: `{}`\nTier: {}\nAction: _{}_",
                tool_name, tier, input_summary
            );

            let keyboard = InlineKeyboardMarkup::new(vec![vec![
                InlineKeyboardButton::callback("✅ Approve", format!("approve:{}", request_id)),
                InlineKeyboardButton::callback("❌ Deny", format!("deny:{}", request_id)),
            ]]);

            match bot
                .send_message(chat_id, &text)
                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                .reply_markup(keyboard)
                .await
            {
                Ok(_) => Ok(true),
                Err(e) => {
                    // Fall back — try without markdown in case of parse errors
                    let plain =
                        format!("[APPROVAL] {} ({}): \"{}\"", tool_name, tier, input_summary);
                    let keyboard2 = InlineKeyboardMarkup::new(vec![vec![
                        InlineKeyboardButton::callback(
                            "Approve",
                            format!("approve:{}", request_id),
                        ),
                        InlineKeyboardButton::callback("Deny", format!("deny:{}", request_id)),
                    ]]);
                    match bot
                        .send_message(chat_id, &plain)
                        .reply_markup(keyboard2)
                        .await
                    {
                        Ok(_) => Ok(true),
                        Err(e2) => {
                            warn!(error = %e, fallback_error = %e2, "Failed to send approval to Telegram");
                            Ok(false)
                        }
                    }
                }
            }
        })
    }

    fn send(&self, session: &SessionId, content: &MessageContent) -> BoxFuture<'_, Result<()>> {
        let session_key = session.0.clone();
        let content = content.clone();
        let chat_map = self.chat_map.clone();
        let bot_arc = self.bot.clone();

        Box::pin(async move {
            let text = match &content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Streaming { delta, done: _ } => delta.clone(),
            };

            if text.is_empty() {
                return Ok(());
            }

            let chat_id = {
                let map = chat_map.lock().await;
                map.get(&session_key).copied()
            };

            let chat_id = chat_id.ok_or_else(|| RyvosError::Channel {
                channel: "telegram".into(),
                message: format!("No chat mapped for session {}", session_key),
            })?;

            let bot_guard = bot_arc.lock().await;
            let bot = bot_guard.as_ref().ok_or_else(|| RyvosError::Channel {
                channel: "telegram".into(),
                message: "Bot not started".into(),
            })?;

            let chunks = split_message(&text, TELEGRAM_MAX_LEN);
            for chunk in chunks {
                bot.send_message(chat_id, &chunk)
                    .await
                    .map_err(|e| RyvosError::Channel {
                        channel: "telegram".into(),
                        message: e.to_string(),
                    })?;
            }

            Ok(())
        })
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>> {
        let shutdown_tx = self.shutdown_tx.clone();

        Box::pin(async move {
            if let Some(tx) = shutdown_tx.lock().await.take() {
                let _ = tx.send(());
            }
            info!("Telegram adapter stopped");
            Ok(())
        })
    }
}
