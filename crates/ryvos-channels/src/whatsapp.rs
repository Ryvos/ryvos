use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

use ryvos_agent::{ApprovalBroker, SessionManager};
use ryvos_core::config::{DmPolicy, WhatsAppConfig};
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::ApprovalRequest;
use ryvos_core::traits::ChannelAdapter;
use ryvos_core::types::{MessageContent, MessageEnvelope, SessionId};

use crate::util::split_message;

const WHATSAPP_MAX_LEN: usize = 4096;

/// WhatsApp Cloud API channel adapter.
///
/// Incoming messages arrive via webhook (handled by gateway routes).
/// Outgoing messages are sent via the Graph API.
pub struct WhatsAppAdapter {
    config: WhatsAppConfig,
    session_mgr: Arc<SessionManager>,
    /// Maps session_id -> phone number for routing responses back.
    phone_map: Arc<Mutex<HashMap<String, String>>>,
    http: reqwest::Client,
    /// Sender for incoming webhook messages, given to the gateway webhook routes.
    webhook_tx: Arc<Mutex<Option<mpsc::Sender<MessageEnvelope>>>>,
    /// Approval broker for HITL.
    broker: Arc<Mutex<Option<Arc<ApprovalBroker>>>>,
}

impl WhatsAppAdapter {
    pub fn new(config: WhatsAppConfig, session_mgr: Arc<SessionManager>) -> Self {
        Self {
            config,
            session_mgr,
            phone_map: Arc::new(Mutex::new(HashMap::new())),
            http: reqwest::Client::new(),
            webhook_tx: Arc::new(Mutex::new(None)),
            broker: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the approval broker for HITL interactive buttons.
    pub fn set_broker(&mut self, broker: Arc<ApprovalBroker>) {
        self.broker = Arc::new(Mutex::new(Some(broker)));
    }

    /// Get a handle for the gateway to forward incoming webhook messages.
    pub fn webhook_handle(&self) -> WhatsAppWebhookHandle {
        WhatsAppWebhookHandle {
            config: self.config.clone(),
            session_mgr: self.session_mgr.clone(),
            phone_map: self.phone_map.clone(),
            webhook_tx: self.webhook_tx.clone(),
        }
    }

    /// Send a text message via WhatsApp Cloud API.
    async fn send_text(
        http: &reqwest::Client,
        access_token: &str,
        phone_number_id: &str,
        to: &str,
        text: &str,
    ) -> Result<()> {
        let url = format!(
            "https://graph.facebook.com/v21.0/{}/messages",
            phone_number_id
        );

        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "text",
            "text": { "body": text }
        });

        let resp = http
            .post(&url)
            .bearer_auth(access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| RyvosError::Channel {
                channel: "whatsapp".into(),
                message: format!("Failed to send message: {e}"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(RyvosError::Channel {
                channel: "whatsapp".into(),
                message: format!("API returned {}: {}", status, body_text),
            });
        }

        Ok(())
    }
}

impl ChannelAdapter for WhatsAppAdapter {
    fn name(&self) -> &str {
        "whatsapp"
    }

    fn start(&self, tx: mpsc::Sender<MessageEnvelope>) -> BoxFuture<'_, Result<()>> {
        let webhook_tx = self.webhook_tx.clone();

        Box::pin(async move {
            *webhook_tx.lock().await = Some(tx);
            info!("WhatsApp adapter started (waiting for webhook messages)");
            Ok(())
        })
    }

    fn send(&self, session: &SessionId, content: &MessageContent) -> BoxFuture<'_, Result<()>> {
        let session_key = session.0.clone();
        let content = content.clone();
        let phone_map = self.phone_map.clone();
        let http = self.http.clone();
        let access_token = self.config.access_token.clone();
        let phone_number_id = self.config.phone_number_id.clone();

        Box::pin(async move {
            let text = match &content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Streaming { delta, done: _ } => delta.clone(),
            };

            if text.is_empty() {
                return Ok(());
            }

            let phone = {
                let map = phone_map.lock().await;
                map.get(&session_key).cloned()
            };

            let phone = phone.ok_or_else(|| RyvosError::Channel {
                channel: "whatsapp".into(),
                message: format!("No phone mapped for session {}", session_key),
            })?;

            let chunks = split_message(&text, WHATSAPP_MAX_LEN);
            for chunk in chunks {
                Self::send_text(&http, &access_token, &phone_number_id, &phone, &chunk).await?;
            }

            Ok(())
        })
    }

    fn send_approval(
        &self,
        session: &SessionId,
        request: &ApprovalRequest,
    ) -> BoxFuture<'_, Result<bool>> {
        let session_key = session.0.clone();
        let phone_map = self.phone_map.clone();
        let http = self.http.clone();
        let access_token = self.config.access_token.clone();
        let phone_number_id = self.config.phone_number_id.clone();
        let request_id = request.id.clone();
        let tool_name = request.tool_name.clone();
        let tier = request.tier;
        let input_summary = request.input_summary.clone();

        Box::pin(async move {
            let phone = {
                let map = phone_map.lock().await;
                map.get(&session_key).cloned()
            };

            let phone = match phone {
                Some(p) => p,
                None => return Ok(false),
            };

            let url = format!(
                "https://graph.facebook.com/v21.0/{}/messages",
                phone_number_id
            );

            // WhatsApp interactive buttons (max 3 per message)
            let body = serde_json::json!({
                "messaging_product": "whatsapp",
                "to": phone,
                "type": "interactive",
                "interactive": {
                    "type": "button",
                    "body": {
                        "text": format!(
                            "Approval Required\n\nTool: {}\nTier: {}\nAction: {}",
                            tool_name, tier, input_summary
                        )
                    },
                    "action": {
                        "buttons": [
                            {
                                "type": "reply",
                                "reply": {
                                    "id": format!("approve:{}", request_id),
                                    "title": "Approve"
                                }
                            },
                            {
                                "type": "reply",
                                "reply": {
                                    "id": format!("deny:{}", request_id),
                                    "title": "Deny"
                                }
                            }
                        ]
                    }
                }
            });

            let resp = http
                .post(&url)
                .bearer_auth(&access_token)
                .json(&body)
                .send()
                .await
                .map_err(|e| RyvosError::Channel {
                    channel: "whatsapp".into(),
                    message: format!("Failed to send approval: {e}"),
                })?;

            if resp.status().is_success() {
                Ok(true)
            } else {
                let err_text = resp.text().await.unwrap_or_default();
                warn!(error = %err_text, "WhatsApp approval send failed");
                Ok(false)
            }
        })
    }

    fn broadcast(&self, content: &MessageContent) -> BoxFuture<'_, Result<()>> {
        let content = content.clone();
        let http = self.http.clone();
        let access_token = self.config.access_token.clone();
        let phone_number_id = self.config.phone_number_id.clone();
        let allowed_users = self.config.allowed_users.clone();

        Box::pin(async move {
            let text = match &content {
                MessageContent::Text(t) => t.clone(),
                MessageContent::Streaming { delta, .. } => delta.clone(),
            };
            if text.is_empty() {
                return Ok(());
            }

            let chunks = split_message(&text, WHATSAPP_MAX_LEN);
            for phone in &allowed_users {
                for chunk in &chunks {
                    if let Err(e) =
                        Self::send_text(&http, &access_token, &phone_number_id, phone, chunk).await
                    {
                        warn!(phone = %phone, error = %e, "Failed to broadcast to WhatsApp user");
                    }
                }
            }
            Ok(())
        })
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>> {
        let webhook_tx = self.webhook_tx.clone();

        Box::pin(async move {
            *webhook_tx.lock().await = None;
            info!("WhatsApp adapter stopped");
            Ok(())
        })
    }
}

/// Handle passed to gateway routes for processing incoming WhatsApp webhooks.
#[derive(Clone)]
pub struct WhatsAppWebhookHandle {
    config: WhatsAppConfig,
    session_mgr: Arc<SessionManager>,
    phone_map: Arc<Mutex<HashMap<String, String>>>,
    webhook_tx: Arc<Mutex<Option<mpsc::Sender<MessageEnvelope>>>>,
}

impl WhatsAppWebhookHandle {
    /// Verify the webhook challenge (GET request from Meta).
    pub fn verify_webhook(&self, mode: &str, token: &str, challenge: &str) -> Option<String> {
        if mode == "subscribe" && token == self.config.verify_token {
            Some(challenge.to_string())
        } else {
            None
        }
    }

    /// Process an incoming webhook payload (POST from Meta).
    pub async fn process_webhook(&self, payload: serde_json::Value) {
        let tx_guard = self.webhook_tx.lock().await;
        let tx = match tx_guard.as_ref() {
            Some(t) => t,
            None => {
                debug!("WhatsApp webhook received but adapter not started");
                return;
            }
        };

        // Parse the Cloud API webhook payload
        let entries = match payload["entry"].as_array() {
            Some(e) => e,
            None => return,
        };

        for entry in entries {
            let changes = match entry["changes"].as_array() {
                Some(c) => c,
                None => continue,
            };

            for change in changes {
                let value = &change["value"];
                let messages = match value["messages"].as_array() {
                    Some(m) => m,
                    None => {
                        // Check for interactive button replies (approvals)
                        if let Some(statuses) = value["statuses"].as_array() {
                            debug!(count = statuses.len(), "WhatsApp status update (ignored)");
                        }
                        continue;
                    }
                };

                for msg in messages {
                    let from = match msg["from"].as_str() {
                        Some(f) => f.to_string(),
                        None => continue,
                    };

                    // Enforce DM policy
                    match self.config.dm_policy {
                        DmPolicy::Disabled => {
                            debug!(from = %from, "WhatsApp DMs disabled, ignoring");
                            continue;
                        }
                        DmPolicy::Allowlist => {
                            if !self.config.allowed_users.is_empty()
                                && !self.config.allowed_users.contains(&from)
                            {
                                debug!(from = %from, "WhatsApp message from non-allowed user");
                                continue;
                            }
                        }
                        DmPolicy::Open => {}
                    }

                    let msg_type = msg["type"].as_str().unwrap_or("");

                    let text = match msg_type {
                        "text" => msg["text"]["body"].as_str().unwrap_or("").to_string(),
                        "interactive" => {
                            // Button reply — handle as approval response
                            let button_id = msg["interactive"]["button_reply"]["id"]
                                .as_str()
                                .unwrap_or("");
                            if let Some((_action, _request_id)) = button_id.split_once(':') {
                                // Approval handling would go through the broker
                                // For now, treat as text
                                msg["interactive"]["button_reply"]["title"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string()
                            } else {
                                continue;
                            }
                        }
                        _ => {
                            debug!(msg_type, "Unsupported WhatsApp message type");
                            continue;
                        }
                    };

                    if text.is_empty() {
                        continue;
                    }

                    let key = format!("whatsapp:user:{}", from);
                    let session_id = self.session_mgr.get_or_create(&key, "whatsapp");

                    // Map session -> phone for response routing
                    self.phone_map
                        .lock()
                        .await
                        .insert(session_id.0.clone(), from.clone());

                    let envelope = MessageEnvelope {
                        id: uuid::Uuid::new_v4().to_string(),
                        session_id,
                        session_key: key.clone(),
                        channel: "whatsapp".into(),
                        sender: from,
                        text,
                        timestamp: chrono::Utc::now(),
                    };

                    if let Err(e) = tx.send(envelope).await {
                        error!(error = %e, "Failed to forward WhatsApp message");
                    }
                }
            }
        }
    }
}
