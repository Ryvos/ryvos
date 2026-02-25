use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use ryvos_agent::{AgentRuntime, ApprovalBroker, SessionManager};
use ryvos_core::event::EventBus;
use ryvos_core::security::ApprovalDecision;
use ryvos_core::traits::SessionStore;
use ryvos_core::types::{AgentEvent, SessionId};

use crate::lane::LaneQueue;
use crate::protocol::{ClientFrame, ServerEvent, ServerResponse};

/// Handle a single WebSocket connection (axum WebSocket).
pub async fn handle_connection(
    ws: WebSocket,
    runtime: Arc<AgentRuntime>,
    event_bus: Arc<EventBus>,
    store: Arc<dyn SessionStore>,
    session_mgr: Arc<SessionManager>,
    broker: Arc<ApprovalBroker>,
) {
    let (ws_tx, mut ws_rx) = ws.split();
    let ws_tx = Arc::new(Mutex::new(ws_tx));

    // Track which sessions this connection is subscribed to
    let subscribed_sessions: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // Subscribe to event bus and forward events
    let mut event_rx = event_bus.subscribe();
    let event_ws_tx = ws_tx.clone();
    let event_subs = subscribed_sessions.clone();
    let event_task = tokio::spawn(async move {
        while let Ok(event) = event_rx.recv().await {
            let server_event = match &event {
                AgentEvent::TextDelta(text) => {
                    let subs = event_subs.lock().await;
                    if subs.is_empty() {
                        continue;
                    }
                    let sid = subs.last().unwrap().clone();
                    Some(ServerEvent::new(sid, "text_delta").with_text(text.clone()))
                }
                AgentEvent::ToolStart { name, input } => {
                    let subs = event_subs.lock().await;
                    if subs.is_empty() {
                        continue;
                    }
                    let sid = subs.last().unwrap().clone();
                    Some(
                        ServerEvent::new(sid, "tool_start")
                            .with_tool(name.clone())
                            .with_data(input.clone()),
                    )
                }
                AgentEvent::ToolEnd { name, result } => {
                    let subs = event_subs.lock().await;
                    if subs.is_empty() {
                        continue;
                    }
                    let sid = subs.last().unwrap().clone();
                    Some(
                        ServerEvent::new(sid, "tool_end")
                            .with_tool(name.clone())
                            .with_data(serde_json::json!({
                                "content": result.content,
                                "is_error": result.is_error,
                            })),
                    )
                }
                AgentEvent::RunStarted { session_id } => {
                    let subs = event_subs.lock().await;
                    if !subs.contains(&session_id.to_string()) {
                        continue;
                    }
                    Some(ServerEvent::new(session_id.to_string(), "run_started"))
                }
                AgentEvent::RunComplete {
                    session_id,
                    total_turns,
                    input_tokens,
                    output_tokens,
                } => {
                    let subs = event_subs.lock().await;
                    if !subs.contains(&session_id.to_string()) {
                        continue;
                    }
                    Some(
                        ServerEvent::new(session_id.to_string(), "run_complete").with_data(
                            serde_json::json!({
                                "total_turns": total_turns,
                                "input_tokens": input_tokens,
                                "output_tokens": output_tokens,
                            }),
                        ),
                    )
                }
                AgentEvent::RunError { error } => {
                    let subs = event_subs.lock().await;
                    if subs.is_empty() {
                        continue;
                    }
                    let sid = subs.last().unwrap().clone();
                    Some(
                        ServerEvent::new(sid, "run_error")
                            .with_data(serde_json::json!({ "error": error })),
                    )
                }
                AgentEvent::ApprovalRequested { request } => {
                    let subs = event_subs.lock().await;
                    if subs.is_empty() {
                        continue;
                    }
                    let sid = subs.last().unwrap().clone();
                    Some(
                        ServerEvent::new(sid, "approval_requested")
                            .with_data(serde_json::json!({
                                "id": request.id,
                                "tool_name": request.tool_name,
                                "tier": request.tier.to_string(),
                                "input_summary": request.input_summary,
                                "session_id": request.session_id,
                            })),
                    )
                }
                AgentEvent::ToolBlocked { name, tier, reason } => {
                    let subs = event_subs.lock().await;
                    if subs.is_empty() {
                        continue;
                    }
                    let sid = subs.last().unwrap().clone();
                    Some(
                        ServerEvent::new(sid, "tool_blocked")
                            .with_tool(name.clone())
                            .with_data(serde_json::json!({
                                "tier": tier.to_string(),
                                "reason": reason,
                            })),
                    )
                }
                AgentEvent::TurnComplete { .. } => None,
                AgentEvent::CronFired { .. } => None,
                AgentEvent::ApprovalResolved { .. } => None,
                AgentEvent::GuardianStall { .. }
                | AgentEvent::GuardianDoomLoop { .. }
                | AgentEvent::GuardianBudgetAlert { .. }
                | AgentEvent::GuardianHint { .. }
                | AgentEvent::UsageUpdate { .. }
                | AgentEvent::GoalEvaluated { .. }
                | AgentEvent::DecisionMade { .. }
                | AgentEvent::JudgeVerdict { .. } => None,
            };

            if let Some(evt) = server_event {
                if let Ok(json) = serde_json::to_string(&evt) {
                    let mut tx = event_ws_tx.lock().await;
                    if tx.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Create a lane for serial request processing
    let (lane, mut lane_rx) = LaneQueue::new(32);

    // Spawn lane processor
    let lane_runtime = runtime.clone();
    let lane_store = store.clone();
    let lane_session_mgr = session_mgr.clone();
    let lane_subs = subscribed_sessions.clone();
    let lane_broker = broker.clone();
    let lane_task = tokio::spawn(async move {
        while let Some(item) = lane_rx.recv().await {
            let result = process_request(
                &item.method,
                &item.params,
                &lane_runtime,
                &lane_store,
                &lane_session_mgr,
                &lane_subs,
                &lane_broker,
            )
            .await;
            let _ = item.respond.send(result);
        }
    });

    // Read incoming frames
    while let Some(msg) = ws_rx.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                debug!(error = %e, "WebSocket read error");
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                let frame: ClientFrame = match serde_json::from_str(&text) {
                    Ok(f) => f,
                    Err(e) => {
                        let resp = ServerResponse::err(
                            "0".to_string(),
                            -32700,
                            format!("Parse error: {}", e),
                        );
                        let json = serde_json::to_string(&resp).unwrap();
                        let mut tx = ws_tx.lock().await;
                        let _ = tx.send(Message::Text(json.into())).await;
                        continue;
                    }
                };

                let id = frame.id.clone();
                match lane.send(frame.method, frame.params).await {
                    Some(result) => {
                        let resp = ServerResponse::ok(id, result);
                        let json = serde_json::to_string(&resp).unwrap();
                        let mut tx = ws_tx.lock().await;
                        let _ = tx.send(Message::Text(json.into())).await;
                    }
                    None => {
                        let resp =
                            ServerResponse::err(id, -32603, "Internal error".to_string());
                        let json = serde_json::to_string(&resp).unwrap();
                        let mut tx = ws_tx.lock().await;
                        let _ = tx.send(Message::Text(json.into())).await;
                    }
                }
            }
            Message::Close(_) => break,
            Message::Ping(data) => {
                let mut tx = ws_tx.lock().await;
                let _ = tx.send(Message::Pong(data)).await;
            }
            _ => {}
        }
    }

    event_task.abort();
    lane_task.abort();
    debug!("Connection closed");
}

async fn process_request(
    method: &str,
    params: &serde_json::Value,
    runtime: &AgentRuntime,
    store: &Arc<dyn SessionStore>,
    session_mgr: &SessionManager,
    subscribed: &Mutex<Vec<String>>,
    broker: &ApprovalBroker,
) -> serde_json::Value {
    match method {
        "agent.send" => {
            let session_id_str = params["session_id"].as_str().unwrap_or("");
            let message = params["message"].as_str().unwrap_or("");

            if message.is_empty() {
                return serde_json::json!({"error": "message is required"});
            }

            let session_id = if session_id_str.is_empty() {
                let sid = session_mgr.get_or_create("ws:default", "websocket");
                // Auto-subscribe
                let mut subs = subscribed.lock().await;
                if !subs.contains(&sid.to_string()) {
                    subs.push(sid.to_string());
                }
                sid
            } else {
                let sid = SessionId::from_string(session_id_str);
                let mut subs = subscribed.lock().await;
                if !subs.contains(&sid.to_string()) {
                    subs.push(sid.to_string());
                }
                sid
            };

            match runtime.run(&session_id, message).await {
                Ok(response) => serde_json::json!({
                    "session_id": session_id.to_string(),
                    "response": response,
                }),
                Err(e) => serde_json::json!({
                    "session_id": session_id.to_string(),
                    "error": e.to_string(),
                }),
            }
        }
        "agent.cancel" => {
            runtime.cancel_token().cancel();
            serde_json::json!({"cancelled": true})
        }
        "session.list" => {
            let keys = session_mgr.list();
            serde_json::json!({"sessions": keys})
        }
        "session.history" => {
            let session_id_str = params["session_id"].as_str().unwrap_or("");
            if session_id_str.is_empty() {
                return serde_json::json!({"error": "session_id is required"});
            }
            let limit = params["limit"].as_u64().unwrap_or(50) as usize;
            let session_id = SessionId::from_string(session_id_str);

            match store.load_history(&session_id, limit).await {
                Ok(messages) => {
                    let msgs: Vec<serde_json::Value> = messages
                        .iter()
                        .map(|m| {
                            serde_json::json!({
                                "role": m.role,
                                "text": m.text(),
                                "timestamp": m.timestamp,
                            })
                        })
                        .collect();
                    serde_json::json!({"messages": msgs})
                }
                Err(e) => serde_json::json!({"error": e.to_string()}),
            }
        }
        "approval.respond" => {
            let request_id = params["request_id"].as_str().unwrap_or("");
            if request_id.is_empty() {
                return serde_json::json!({"error": "request_id is required"});
            }
            let approved = params["approved"].as_bool().unwrap_or(false);
            let reason = params["reason"]
                .as_str()
                .unwrap_or("denied")
                .to_string();
            let decision = if approved {
                ApprovalDecision::Approved
            } else {
                ApprovalDecision::Denied { reason }
            };
            let resolved = broker.respond(request_id, decision).await;
            serde_json::json!({"resolved": resolved})
        }
        _ => {
            warn!(method, "Unknown method");
            serde_json::json!({"error": format!("Unknown method: {}", method)})
        }
    }
}
