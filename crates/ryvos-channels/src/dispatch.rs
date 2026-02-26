use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use ryvos_agent::{AgentRuntime, ApprovalBroker};
use ryvos_core::config::HooksConfig;
use ryvos_core::event::EventBus;
use ryvos_core::security::ApprovalDecision;
use ryvos_core::traits::ChannelAdapter;
use ryvos_core::types::{AgentEvent, MessageContent, MessageEnvelope};

/// Dispatches incoming channel messages to the agent runtime
/// and routes responses back to the originating adapter.
pub struct ChannelDispatcher {
    runtime: Arc<AgentRuntime>,
    event_bus: Arc<EventBus>,
    cancel: CancellationToken,
    adapters: HashMap<String, Arc<dyn ChannelAdapter>>,
    hooks: Option<HooksConfig>,
    broker: Option<Arc<ApprovalBroker>>,
}

impl ChannelDispatcher {
    pub fn new(
        runtime: Arc<AgentRuntime>,
        event_bus: Arc<EventBus>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            runtime,
            event_bus,
            cancel,
            adapters: HashMap::new(),
            hooks: None,
            broker: None,
        }
    }

    /// Add a channel adapter.
    pub fn add_adapter(&mut self, adapter: Arc<dyn ChannelAdapter>) {
        self.adapters.insert(adapter.name().to_string(), adapter);
    }

    /// Set lifecycle hooks.
    pub fn set_hooks(&mut self, hooks: HooksConfig) {
        self.hooks = Some(hooks);
    }

    /// Set the approval broker for HITL approval handling.
    pub fn set_broker(&mut self, broker: Arc<ApprovalBroker>) {
        self.broker = Some(broker);
    }

    /// Start all adapters and dispatch incoming messages until cancelled.
    pub async fn run(self) -> ryvos_core::error::Result<()> {
        let (tx, mut rx) = mpsc::channel::<MessageEnvelope>(256);

        // Start all adapters
        for (name, adapter) in &self.adapters {
            info!(channel = %name, "Starting channel adapter");
            if let Err(e) = adapter.start(tx.clone()).await {
                error!(channel = %name, error = %e, "Failed to start adapter");
            }
        }

        // Drop our copy so the channel can close when adapters stop
        drop(tx);

        // Fire on_start hook
        if let Some(ref hooks) = self.hooks {
            ryvos_core::hooks::run_hooks(&hooks.on_start, &[]).await;
        }

        // Spawn a heartbeat alert router task
        {
            let mut hb_rx = self.event_bus.subscribe();
            let adapters = self.adapters.clone();
            let hb_cancel = self.cancel.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = hb_cancel.cancelled() => break,
                        event = hb_rx.recv() => {
                            if let Ok(AgentEvent::HeartbeatAlert { session_id, message, target_channel }) = event {
                                let content = MessageContent::Text(
                                    format!("[Heartbeat Alert] {}", message),
                                );
                                if let Some(ref channel) = target_channel {
                                    if let Some(adapter) = adapters.get(channel) {
                                        adapter.send(&session_id, &content).await.ok();
                                    }
                                } else {
                                    // Broadcast to all adapters
                                    for adapter in adapters.values() {
                                        adapter.send(&session_id, &content).await.ok();
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }

        info!(count = self.adapters.len(), "Channel dispatcher running");

        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    info!("Channel dispatcher shutting down");
                    break;
                }
                envelope = rx.recv() => {
                    match envelope {
                        Some(env) => {
                            // Intercept /approve and /deny text commands
                            if let Some(ref broker) = self.broker {
                                if env.text.starts_with("/approve ") || env.text.starts_with("/deny ") {
                                    let adapter = self.adapters.get(&env.channel).cloned();
                                    handle_approval_command(broker, adapter.as_deref(), &env).await;
                                    continue;
                                }
                            }

                            let adapter = self.adapters.get(&env.channel).cloned();
                            if let Some(adapter) = adapter {
                                let runtime = self.runtime.clone();
                                let event_bus = self.event_bus.clone();
                                let hooks = self.hooks.clone();
                                let broker = self.broker.clone();
                                tokio::spawn(run_channel_message(
                                    runtime, event_bus, adapter, env, hooks, broker,
                                ));
                            } else {
                                error!(channel = %env.channel, "No adapter for channel");
                            }
                        }
                        None => {
                            info!("All channel senders dropped, shutting down");
                            break;
                        }
                    }
                }
            }
        }

        // Stop all adapters
        for (name, adapter) in &self.adapters {
            info!(channel = %name, "Stopping channel adapter");
            if let Err(e) = adapter.stop().await {
                error!(channel = %name, error = %e, "Failed to stop adapter");
            }
        }

        Ok(())
    }
}

/// Handle /approve or /deny text commands from a channel.
async fn handle_approval_command(
    broker: &ApprovalBroker,
    adapter: Option<&dyn ChannelAdapter>,
    envelope: &MessageEnvelope,
) {
    let parts: Vec<&str> = envelope.text.split_whitespace().collect();
    let is_approve = parts[0] == "/approve";
    let prefix = match parts.get(1) {
        Some(p) => *p,
        None => {
            if let Some(adapter) = adapter {
                let usage = if is_approve {
                    "Usage: /approve <id-prefix>"
                } else {
                    "Usage: /deny <id-prefix> [reason]"
                };
                adapter.send(&envelope.session_id, &MessageContent::Text(usage.into())).await.ok();
            }
            return;
        }
    };

    let full_id = match broker.find_by_prefix(prefix).await {
        Some(id) => id,
        None => {
            if let Some(adapter) = adapter {
                let msg = format!("No pending request matching '{}'.", prefix);
                adapter.send(&envelope.session_id, &MessageContent::Text(msg)).await.ok();
            }
            return;
        }
    };

    let decision = if is_approve {
        ApprovalDecision::Approved
    } else {
        let reason = if parts.len() > 2 {
            parts[2..].join(" ")
        } else {
            "denied by user".to_string()
        };
        ApprovalDecision::Denied { reason }
    };

    let label = if is_approve { "Approved" } else { "Denied" };
    let short_id = &full_id[..8.min(full_id.len())];

    if broker.respond(&full_id, decision).await {
        if let Some(adapter) = adapter {
            let msg = format!("{}: {}", label, short_id);
            adapter.send(&envelope.session_id, &MessageContent::Text(msg)).await.ok();
        }
    } else if let Some(adapter) = adapter {
        let msg = "Request not found (may have timed out).".to_string();
        adapter.send(&envelope.session_id, &MessageContent::Text(msg)).await.ok();
    }
}

/// Handle a single channel message: run the agent, capture response via
/// EventBus, and send the collected text back through the adapter.
async fn run_channel_message(
    runtime: Arc<AgentRuntime>,
    event_bus: Arc<EventBus>,
    adapter: Arc<dyn ChannelAdapter>,
    envelope: MessageEnvelope,
    hooks: Option<HooksConfig>,
    _broker: Option<Arc<ApprovalBroker>>,
) {
    let session_id = envelope.session_id.clone();

    info!(
        channel = %envelope.channel,
        session = %session_id,
        sender = %envelope.sender,
        "Processing channel message"
    );

    // Fire on_session_start hook
    if let Some(ref hooks) = hooks {
        ryvos_core::hooks::run_hooks(&hooks.on_session_start, &[
            ("RYVOS_SESSION", &session_id.0),
        ]).await;
    }

    // Fire on_message hook
    if let Some(ref hooks) = hooks {
        ryvos_core::hooks::run_hooks(&hooks.on_message, &[
            ("RYVOS_SESSION", &session_id.0),
            ("RYVOS_TEXT", &envelope.text),
        ]).await;
    }

    // Subscribe to events BEFORE running so we capture all deltas
    let mut event_rx = event_bus.subscribe();

    // Clone hook commands for the event loop
    let on_tool_call_cmds = hooks
        .as_ref()
        .map(|h| h.on_tool_call.clone())
        .unwrap_or_default();
    let on_turn_complete_cmds = hooks
        .as_ref()
        .map(|h| h.on_turn_complete.clone())
        .unwrap_or_default();
    let on_tool_error_cmds = hooks
        .as_ref()
        .map(|h| h.on_tool_error.clone())
        .unwrap_or_default();
    let session_id_str = session_id.0.clone();

    // Run the agent in a background task
    let rt = runtime.clone();
    let sid = session_id.clone();
    let text = envelope.text.clone();
    let run_handle = tokio::spawn(async move { rt.run(&sid, &text).await });

    // Collect text deltas from the event stream
    let mut response_text = String::new();
    loop {
        match event_rx.recv().await {
            Ok(AgentEvent::TextDelta(delta)) => {
                response_text.push_str(&delta);
            }
            Ok(AgentEvent::ToolStart { ref name, .. }) => {
                if !on_tool_call_cmds.is_empty() {
                    let cmds = on_tool_call_cmds.clone();
                    let sid = session_id_str.clone();
                    let tool = name.clone();
                    tokio::spawn(async move {
                        ryvos_core::hooks::run_hooks(&cmds, &[
                            ("RYVOS_SESSION", &sid),
                            ("RYVOS_TOOL", &tool),
                        ]).await;
                    });
                }
            }
            Ok(AgentEvent::TurnComplete { turn }) => {
                if !on_turn_complete_cmds.is_empty() {
                    let cmds = on_turn_complete_cmds.clone();
                    let sid = session_id_str.clone();
                    let turn_str = turn.to_string();
                    tokio::spawn(async move {
                        ryvos_core::hooks::run_hooks(&cmds, &[
                            ("RYVOS_SESSION", &sid),
                            ("RYVOS_TURN", &turn_str),
                        ]).await;
                    });
                }
            }
            Ok(AgentEvent::ToolEnd { ref name, ref result }) if result.is_error => {
                if !on_tool_error_cmds.is_empty() {
                    let cmds = on_tool_error_cmds.clone();
                    let sid = session_id_str.clone();
                    let tool = name.clone();
                    let error = result.content.clone();
                    tokio::spawn(async move {
                        ryvos_core::hooks::run_hooks(&cmds, &[
                            ("RYVOS_SESSION", &sid),
                            ("RYVOS_TOOL", &tool),
                            ("RYVOS_ERROR", &error),
                        ]).await;
                    });
                }
            }
            Ok(AgentEvent::RunComplete { session_id: ref completed_sid, .. })
                if completed_sid.0 == session_id.0 =>
            {
                break;
            }
            Ok(AgentEvent::RunError { ref error }) => {
                response_text = format!("Error: {}", error);
                break;
            }
            Ok(AgentEvent::ApprovalRequested { ref request })
                if request.session_id == session_id.0 =>
            {
                let sent = adapter.send_approval(&session_id, request).await.unwrap_or(false);
                if !sent {
                    let short_id = &request.id[..8.min(request.id.len())];
                    let text = format!(
                        "[APPROVAL] {} ({}): \"{}\"\nReply /approve {} or /deny {}",
                        request.tool_name, request.tier, request.input_summary,
                        short_id, short_id,
                    );
                    adapter.send(&session_id, &MessageContent::Text(text)).await.ok();
                }
            }
            Ok(AgentEvent::ToolBlocked { ref name, ref tier, ref reason }) => {
                let text = format!("[BLOCKED] {} ({}): {}", name, tier, reason);
                adapter.send(&session_id, &MessageContent::Text(text)).await.ok();
            }
            Err(_) => break,
            _ => {}
        }
    }

    // Wait for the run task to finish
    if let Err(e) = run_handle.await {
        error!(error = %e, "Agent task panicked");
    }

    // Fire on_response hook
    if let Some(ref hooks) = hooks {
        ryvos_core::hooks::run_hooks(&hooks.on_response, &[
            ("RYVOS_SESSION", &session_id.0),
        ]).await;
    }

    // Send the collected response back through the adapter
    if !response_text.is_empty() {
        let content = MessageContent::Text(response_text);
        if let Err(e) = adapter.send(&session_id, &content).await {
            error!(error = %e, "Failed to send response to channel");
        }
    }

    // Fire on_session_end hook
    if let Some(ref hooks) = hooks {
        ryvos_core::hooks::run_hooks(&hooks.on_session_end, &[
            ("RYVOS_SESSION", &session_id.0),
        ]).await;
    }
}
