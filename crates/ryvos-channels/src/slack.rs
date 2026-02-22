use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{debug, error, info, warn};

use ryvos_agent::{ApprovalBroker, SessionManager};
use ryvos_core::config::{DmPolicy, SlackConfig};
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::{ApprovalDecision, ApprovalRequest};
use ryvos_core::traits::ChannelAdapter;
use ryvos_core::types::{MessageContent, MessageEnvelope, SessionId};

use crate::util::split_message;

const SLACK_MAX_LEN: usize = 4000;

/// Slack channel adapter using Socket Mode (WebSocket) for receiving
/// and Web API for sending messages.
pub struct SlackAdapter {
    config: SlackConfig,
    session_mgr: Arc<SessionManager>,
    /// Maps session_id -> channel_id for routing responses back.
    channel_map: Arc<Mutex<HashMap<String, String>>>,
    http: reqwest::Client,
    shutdown_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    /// Approval broker for HITL.
    broker: Arc<Mutex<Option<Arc<ApprovalBroker>>>>,
}

impl SlackAdapter {
    pub fn new(config: SlackConfig, session_mgr: Arc<SessionManager>) -> Self {
        Self {
            config,
            session_mgr,
            channel_map: Arc::new(Mutex::new(HashMap::new())),
            http: reqwest::Client::new(),
            shutdown_tx: Arc::new(Mutex::new(None)),
            broker: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the approval broker for HITL Block Kit buttons.
    pub fn set_broker(&mut self, broker: Arc<ApprovalBroker>) {
        self.broker = Arc::new(Mutex::new(Some(broker)));
    }

    /// Request a Socket Mode WebSocket URL from Slack.
    async fn get_ws_url(http: &reqwest::Client, app_token: &str) -> Result<String> {
        let resp = http
            .post("https://slack.com/api/apps.connections.open")
            .bearer_auth(app_token)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .await
            .map_err(|e| RyvosError::Channel {
                channel: "slack".into(),
                message: format!("Failed to open connection: {e}"),
            })?;

        let body: serde_json::Value = resp.json().await.map_err(|e| RyvosError::Channel {
            channel: "slack".into(),
            message: format!("Invalid response from connections.open: {e}"),
        })?;

        if !body["ok"].as_bool().unwrap_or(false) {
            return Err(RyvosError::Channel {
                channel: "slack".into(),
                message: format!(
                    "apps.connections.open failed: {}",
                    body["error"].as_str().unwrap_or("unknown")
                ),
            });
        }

        body["url"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| RyvosError::Channel {
                channel: "slack".into(),
                message: "No URL in connections.open response".into(),
            })
    }

    /// Send a message via Slack Web API.
    async fn post_message(
        http: &reqwest::Client,
        bot_token: &str,
        channel: &str,
        text: &str,
    ) -> Result<()> {
        let resp = http
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(bot_token)
            .json(&serde_json::json!({
                "channel": channel,
                "text": text,
            }))
            .send()
            .await
            .map_err(|e| RyvosError::Channel {
                channel: "slack".into(),
                message: format!("chat.postMessage failed: {e}"),
            })?;

        let body: serde_json::Value = resp.json().await.map_err(|e| RyvosError::Channel {
            channel: "slack".into(),
            message: format!("Invalid postMessage response: {e}"),
        })?;

        if !body["ok"].as_bool().unwrap_or(false) {
            return Err(RyvosError::Channel {
                channel: "slack".into(),
                message: format!(
                    "chat.postMessage error: {}",
                    body["error"].as_str().unwrap_or("unknown")
                ),
            });
        }

        Ok(())
    }
}

impl ChannelAdapter for SlackAdapter {
    fn name(&self) -> &str {
        "slack"
    }

    fn send_approval(
        &self,
        session: &SessionId,
        request: &ApprovalRequest,
    ) -> BoxFuture<'_, Result<bool>> {
        let session_key = session.0.clone();
        let channel_map = self.channel_map.clone();
        let http = self.http.clone();
        let bot_token = self.config.bot_token.clone();
        let request_id = request.id.clone();
        let tool_name = request.tool_name.clone();
        let tier = request.tier;
        let input_summary = request.input_summary.clone();

        Box::pin(async move {
            let channel_id = {
                let map = channel_map.lock().await;
                map.get(&session_key).cloned()
            };

            let channel_id = match channel_id {
                Some(id) => id,
                None => return Ok(false),
            };

            let blocks = serde_json::json!([
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": format!(
                            ":lock: *Approval Required*\n\nTool: `{}`\nTier: {}\nAction: _{}_",
                            tool_name, tier, input_summary
                        )
                    }
                },
                {
                    "type": "actions",
                    "elements": [
                        {
                            "type": "button",
                            "text": { "type": "plain_text", "text": "Approve" },
                            "style": "primary",
                            "action_id": format!("approve:{}", request_id)
                        },
                        {
                            "type": "button",
                            "text": { "type": "plain_text", "text": "Deny" },
                            "style": "danger",
                            "action_id": format!("deny:{}", request_id)
                        }
                    ]
                }
            ]);

            let resp = http
                .post("https://slack.com/api/chat.postMessage")
                .bearer_auth(&bot_token)
                .json(&serde_json::json!({
                    "channel": channel_id,
                    "text": format!("[APPROVAL] {} ({}): \"{}\"", tool_name, tier, input_summary),
                    "blocks": blocks,
                }))
                .send()
                .await
                .map_err(|e| RyvosError::Channel {
                    channel: "slack".into(),
                    message: format!("Failed to send approval: {e}"),
                })?;

            let body: serde_json::Value = resp.json().await.map_err(|e| RyvosError::Channel {
                channel: "slack".into(),
                message: format!("Invalid approval response: {e}"),
            })?;

            if body["ok"].as_bool().unwrap_or(false) {
                Ok(true)
            } else {
                warn!(
                    error = %body["error"].as_str().unwrap_or("unknown"),
                    "Slack approval postMessage failed"
                );
                Ok(false)
            }
        })
    }

    fn start(&self, tx: mpsc::Sender<MessageEnvelope>) -> BoxFuture<'_, Result<()>> {
        let config = self.config.clone();
        let session_mgr = self.session_mgr.clone();
        let channel_map = self.channel_map.clone();
        let http = self.http.clone();
        let shutdown_tx_arc = self.shutdown_tx.clone();
        let broker_arc = self.broker.clone();

        Box::pin(async move {
            let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
            *shutdown_tx_arc.lock().await = Some(stop_tx);

            info!("Slack adapter starting (Socket Mode)");

            tokio::spawn(async move {
                loop {
                    // Get WebSocket URL
                    let ws_url = match Self::get_ws_url(&http, &config.app_token).await {
                        Ok(url) => url,
                        Err(e) => {
                            error!(error = %e, "Failed to get Slack WebSocket URL");
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue;
                        }
                    };

                    // Connect WebSocket
                    let ws_stream = match tokio_tungstenite::connect_async(&ws_url).await {
                        Ok((stream, _)) => stream,
                        Err(e) => {
                            error!(error = %e, "Failed to connect Slack WebSocket");
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue;
                        }
                    };

                    info!("Slack Socket Mode connected");
                    let (mut ws_tx, mut ws_rx) = ws_stream.split();

                    loop {
                        tokio::select! {
                            _ = &mut stop_rx => {
                                info!("Slack adapter received shutdown signal");
                                return;
                            }
                            msg = ws_rx.next() => {
                                let msg = match msg {
                                    Some(Ok(m)) => m,
                                    Some(Err(e)) => {
                                        warn!(error = %e, "Slack WebSocket error, reconnecting");
                                        break; // Reconnect
                                    }
                                    None => {
                                        warn!("Slack WebSocket closed, reconnecting");
                                        break; // Reconnect
                                    }
                                };

                                let text = match msg {
                                    WsMessage::Text(t) => t.to_string(),
                                    WsMessage::Ping(data) => {
                                        let _ = ws_tx.send(WsMessage::Pong(data)).await;
                                        continue;
                                    }
                                    WsMessage::Close(_) => {
                                        warn!("Slack WebSocket close frame, reconnecting");
                                        break;
                                    }
                                    _ => continue,
                                };

                                let envelope: serde_json::Value = match serde_json::from_str(&text) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        debug!(error = %e, "Invalid JSON from Slack");
                                        continue;
                                    }
                                };

                                let envelope_type = envelope["type"].as_str().unwrap_or("");

                                // ACK the envelope
                                if let Some(envelope_id) = envelope["envelope_id"].as_str() {
                                    let ack = serde_json::json!({"envelope_id": envelope_id});
                                    if let Err(e) = ws_tx.send(WsMessage::Text(ack.to_string().into())).await {
                                        warn!(error = %e, "Failed to ACK Slack envelope");
                                    }
                                }

                                match envelope_type {
                                    "hello" => {
                                        info!("Slack Socket Mode hello received");
                                    }
                                    "events_api" => {
                                        let payload = &envelope["payload"];
                                        let event = &payload["event"];
                                        let event_type = event["type"].as_str().unwrap_or("");

                                        if event_type != "message" {
                                            continue;
                                        }

                                        // Skip bot messages and subtypes (edits, joins, etc.)
                                        if event.get("bot_id").is_some()
                                            || event.get("subtype").is_some()
                                        {
                                            continue;
                                        }

                                        let user_id = match event["user"].as_str() {
                                            Some(u) => u.to_string(),
                                            None => continue,
                                        };
                                        let channel_id = match event["channel"].as_str() {
                                            Some(c) => c.to_string(),
                                            None => continue,
                                        };
                                        let msg_text = event["text"]
                                            .as_str()
                                            .unwrap_or("")
                                            .to_string();

                                        if msg_text.is_empty() {
                                            continue;
                                        }

                                        // Enforce DM policy
                                        match config.dm_policy {
                                            DmPolicy::Disabled => {
                                                debug!(user = %user_id, "Slack DMs disabled, ignoring");
                                                continue;
                                            }
                                            DmPolicy::Allowlist => {
                                                if !config.allowed_users.is_empty()
                                                    && !config.allowed_users.contains(&user_id)
                                                {
                                                    debug!(user = %user_id, "Slack message from non-allowed user");
                                                    continue;
                                                }
                                            }
                                            DmPolicy::Open => {}
                                        }

                                        let key = format!(
                                            "slack:channel:{}:user:{}",
                                            channel_id, user_id
                                        );
                                        let session_id =
                                            session_mgr.get_or_create(&key, "slack");

                                        // Map session -> channel for response routing
                                        channel_map
                                            .lock()
                                            .await
                                            .insert(session_id.0.clone(), channel_id);

                                        let msg_envelope = MessageEnvelope {
                                            id: uuid::Uuid::new_v4().to_string(),
                                            session_id,
                                            channel: "slack".into(),
                                            sender: user_id,
                                            text: msg_text,
                                            timestamp: chrono::Utc::now(),
                                        };

                                        if let Err(e) = tx.send(msg_envelope).await {
                                            error!(error = %e, "Failed to forward Slack message");
                                        }
                                    }
                                    "interactive" => {
                                        let payload = &envelope["payload"];
                                        if payload["type"].as_str() == Some("block_actions") {
                                            let broker_guard = broker_arc.lock().await;
                                            if let Some(ref broker) = *broker_guard {
                                                if let Some(actions) = payload["actions"].as_array() {
                                                    for action in actions {
                                                        let action_id = action["action_id"]
                                                            .as_str()
                                                            .unwrap_or("");
                                                        let (act, request_id) = match action_id.split_once(':') {
                                                            Some((a, id)) if a == "approve" || a == "deny" => (a, id),
                                                            _ => continue,
                                                        };
                                                        let decision = if act == "approve" {
                                                            ApprovalDecision::Approved
                                                        } else {
                                                            ApprovalDecision::Denied {
                                                                reason: "denied via Slack".to_string(),
                                                            }
                                                        };
                                                        let resolved = broker.respond(request_id, decision).await;
                                                        debug!(
                                                            action = act,
                                                            request_id,
                                                            resolved,
                                                            "Slack interactive approval"
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    "disconnect" => {
                                        info!("Slack requested disconnect, reconnecting");
                                        break;
                                    }
                                    _ => {
                                        debug!(envelope_type, "Unhandled Slack envelope type");
                                    }
                                }
                            }
                        }
                    }

                    // Brief delay before reconnect
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            });

            Ok(())
        })
    }

    fn send(&self, session: &SessionId, content: &MessageContent) -> BoxFuture<'_, Result<()>> {
        let session_key = session.0.clone();
        let content = content.clone();
        let channel_map = self.channel_map.clone();
        let http = self.http.clone();
        let bot_token = self.config.bot_token.clone();

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
                map.get(&session_key).cloned()
            };

            let channel_id = channel_id.ok_or_else(|| RyvosError::Channel {
                channel: "slack".into(),
                message: format!("No channel mapped for session {}", session_key),
            })?;

            let chunks = split_message(&text, SLACK_MAX_LEN);
            for chunk in chunks {
                Self::post_message(&http, &bot_token, &channel_id, &chunk).await?;
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
            info!("Slack adapter stopped");
            Ok(())
        })
    }
}
