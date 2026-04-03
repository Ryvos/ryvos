//! WebSocket connection handler for the Ryvos Web UI.
//!
//! Each WebSocket connection gets:
//!
//! - **Event stream**: A background task subscribes to the EventBus and
//!   converts ~23 AgentEvent types into ServerEvent frames, forwarding them
//!   to the client in real time. Auto-subscribes to system events (heartbeat,
//!   cron, budget, guardian alerts).
//!
//! - **Lane queue**: A per-session FIFO queue (buffer size 32) that serializes
//!   incoming RPC requests to prevent concurrent mutations on the same session.
//!
//! - **RPC methods**: `agent.send` (send message), `agent.cancel` (cancel run),
//!   `session.list`, `session.history`, `approval.respond` (approve/deny).
//!
//! The WebSocket protocol uses JSON frames:
//! - Client sends: `{ "type": "request", "id": "...", "method": "...", "params": {...} }`
//! - Server responds: `{ "type": "response", "id": "...", "result": {...} }`
//! - Server pushes: `{ "type": "event", "session_id": "...", "event": {...} }`

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
    // Auto-subscribe to "*" so system events (heartbeat, cron, budget) are always forwarded
    let subscribed_sessions: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec!["*".to_string()]));

    // Subscribe to event bus and forward events
    let mut event_rx = event_bus.subscribe();
    let event_ws_tx = ws_tx.clone();
    let event_subs = subscribed_sessions.clone();
    let event_task =
        tokio::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                let server_event =
                    match &event {
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
                            Some(ServerEvent::new(session_id.to_string(), "run_started"))
                        }
                        AgentEvent::RunComplete {
                            session_id,
                            total_turns,
                            input_tokens,
                            output_tokens,
                        } => Some(
                            ServerEvent::new(session_id.to_string(), "run_complete").with_data(
                                serde_json::json!({
                                    "total_turns": total_turns,
                                    "input_tokens": input_tokens,
                                    "output_tokens": output_tokens,
                                }),
                            ),
                        ),
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
                            Some(ServerEvent::new(sid, "approval_requested").with_data(
                                serde_json::json!({
                                    "id": request.id,
                                    "tool_name": request.tool_name,
                                    "tier": request.tier.to_string(),
                                    "input_summary": request.input_summary,
                                    "session_id": request.session_id,
                                }),
                            ))
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
                        AgentEvent::UsageUpdate {
                            input_tokens,
                            output_tokens,
                        } => {
                            let subs = event_subs.lock().await;
                            if subs.is_empty() {
                                continue;
                            }
                            let sid = subs.last().unwrap().clone();
                            Some(ServerEvent::new(sid, "usage_update").with_data(
                                serde_json::json!({
                                    "input_tokens": input_tokens,
                                    "output_tokens": output_tokens,
                                }),
                            ))
                        }
                        AgentEvent::BudgetWarning {
                            session_id,
                            spent_cents,
                            budget_cents,
                            utilization_pct,
                        } => {
                            let subs = event_subs.lock().await;
                            if subs.is_empty() {
                                continue;
                            }
                            Some(
                                ServerEvent::new(session_id.to_string(), "budget_warning")
                                    .with_data(serde_json::json!({
                                        "spent_cents": spent_cents,
                                        "budget_cents": budget_cents,
                                        "utilization_pct": utilization_pct,
                                    })),
                            )
                        }
                        AgentEvent::BudgetExceeded {
                            session_id,
                            spent_cents,
                            budget_cents,
                        } => {
                            let subs = event_subs.lock().await;
                            if subs.is_empty() {
                                continue;
                            }
                            Some(
                                ServerEvent::new(session_id.to_string(), "budget_exceeded")
                                    .with_data(serde_json::json!({
                                        "spent_cents": spent_cents,
                                        "budget_cents": budget_cents,
                                    })),
                            )
                        }
                        AgentEvent::HeartbeatFired { timestamp } => Some(
                            ServerEvent::new("system".to_string(), "heartbeat_fired").with_data(
                                serde_json::json!({ "timestamp": timestamp.to_rfc3339() }),
                            ),
                        ),
                        AgentEvent::HeartbeatOk {
                            session_id,
                            response_chars,
                        } => Some(
                            ServerEvent::new(session_id.to_string(), "heartbeat_ok")
                                .with_data(serde_json::json!({ "response_chars": response_chars })),
                        ),
                        AgentEvent::HeartbeatAlert {
                            session_id,
                            message,
                            target_channel,
                        } => Some(
                            ServerEvent::new(session_id.to_string(), "heartbeat_alert").with_data(
                                serde_json::json!({
                                    "message": message,
                                    "target_channel": target_channel,
                                }),
                            ),
                        ),
                        AgentEvent::CronFired { job_id, .. } => Some(
                            ServerEvent::new("system".to_string(), "cron_fired")
                                .with_data(serde_json::json!({ "job_name": job_id })),
                        ),
                        AgentEvent::CronJobComplete { name, .. } => Some(
                            ServerEvent::new("system".to_string(), "cron_complete")
                                .with_data(serde_json::json!({ "job_name": name })),
                        ),
                        AgentEvent::GuardianStall { session_id, .. } => {
                            Some(ServerEvent::new(session_id.to_string(), "guardian_stall"))
                        }
                        AgentEvent::GuardianDoomLoop { session_id, .. } => Some(ServerEvent::new(
                            session_id.to_string(),
                            "guardian_doom_loop",
                        )),
                        AgentEvent::GuardianBudgetAlert { session_id, .. } => Some(
                            ServerEvent::new(session_id.to_string(), "guardian_budget_alert"),
                        ),
                        AgentEvent::GraphGenerated {
                            session_id,
                            node_count,
                            edge_count,
                            evolution_cycle,
                        } => Some(
                            ServerEvent::new(session_id.to_string(), "graph_generated").with_data(
                                serde_json::json!({
                                    "node_count": node_count,
                                    "edge_count": edge_count,
                                    "evolution_cycle": evolution_cycle,
                                }),
                            ),
                        ),
                        AgentEvent::NodeComplete {
                            session_id,
                            node_id,
                            succeeded,
                            elapsed_ms,
                        } => Some(
                            ServerEvent::new(session_id.to_string(), "node_complete").with_data(
                                serde_json::json!({
                                    "node_id": node_id,
                                    "succeeded": succeeded,
                                    "elapsed_ms": elapsed_ms,
                                }),
                            ),
                        ),
                        AgentEvent::EvolutionTriggered {
                            session_id,
                            reason,
                            cycle,
                        } => Some(
                            ServerEvent::new(session_id.to_string(), "evolution_triggered")
                                .with_data(serde_json::json!({
                                    "reason": reason,
                                    "cycle": cycle,
                                })),
                        ),
                        AgentEvent::SemanticFailureCaptured {
                            session_id,
                            node_id,
                            category,
                            diagnosis,
                        } => Some(
                            ServerEvent::new(session_id.to_string(), "semantic_failure").with_data(
                                serde_json::json!({
                                    "node_id": node_id,
                                    "category": category,
                                    "diagnosis": diagnosis,
                                }),
                            ),
                        ),
                        AgentEvent::TurnComplete { .. } => None,
                        AgentEvent::ApprovalResolved { .. } => None,
                        AgentEvent::GuardianHint { .. }
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
                        let resp = ServerResponse::err(id, -32603, "Internal error".to_string());
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
                let sid = session_mgr.get_or_create(session_id_str, "webui");
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
            let reason = params["reason"].as_str().unwrap_or("denied").to_string();
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
