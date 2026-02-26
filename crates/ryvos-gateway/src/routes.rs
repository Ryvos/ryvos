use std::sync::Arc;

use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use tracing::{debug, info};

use ryvos_core::types::SessionId;

use crate::auth;
use crate::connection;
use crate::middleware::Authenticated;
use crate::state::AppState;

// GET /api/health — no auth required
pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

// GET /api/sessions — requires Viewer+
pub async fn list_sessions(
    Authenticated(auth_result): Authenticated,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth::has_viewer_access(&auth_result.role) {
        return Err(StatusCode::FORBIDDEN);
    }
    let keys = state.session_mgr.list();
    Ok(Json(serde_json::json!({ "sessions": keys })))
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

// GET /api/sessions/:id/history?limit=50 — requires Viewer+
pub async fn session_history(
    Authenticated(auth_result): Authenticated,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth::has_viewer_access(&auth_result.role) {
        return Err(StatusCode::FORBIDDEN);
    }

    let session_id = SessionId::from_string(&id);
    match state.store.load_history(&session_id, q.limit).await {
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
            Ok(Json(serde_json::json!({ "messages": msgs })))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Deserialize)]
pub struct SendMessageBody {
    pub message: String,
}

// POST /api/sessions/:id/messages — requires Operator+
pub async fn send_message(
    Authenticated(auth_result): Authenticated,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<SendMessageBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth::has_operator_access(&auth_result.role) {
        return Err(StatusCode::FORBIDDEN);
    }

    if body.message.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let session_id = SessionId::from_string(&id);
    match state.runtime.run(&session_id, &body.message).await {
        Ok(response) => Ok(Json(serde_json::json!({
            "session_id": session_id.to_string(),
            "response": response,
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "session_id": session_id.to_string(),
            "error": e.to_string(),
        }))),
    }
}

// ── Webhook endpoint ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct WebhookPayload {
    pub prompt: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
}

/// POST /api/hooks/wake — webhook endpoint for external triggers.
/// Authenticated via Bearer token from gateway.webhooks.token config.
pub async fn webhook_wake(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<WebhookPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Validate webhook token
    let webhook_config = state
        .config
        .webhooks
        .as_ref()
        .filter(|w| w.enabled)
        .ok_or(StatusCode::NOT_FOUND)?;

    if let Some(ref expected_token) = webhook_config.token {
        let auth_header = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if auth_header != expected_token {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    if body.prompt.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let session_id = body
        .session_id
        .map(|s| SessionId::from_string(&s))
        .unwrap_or_else(SessionId::new);

    info!(session_id = %session_id, "Webhook wake triggered");

    match state.runtime.run(&session_id, &body.prompt).await {
        Ok(response) => Ok(Json(serde_json::json!({
            "session_id": session_id.to_string(),
            "response": response,
            "channel": body.channel,
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "session_id": session_id.to_string(),
            "error": e.to_string(),
        }))),
    }
}

// GET /ws — WebSocket upgrade, requires auth
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Authenticated(_auth_result): Authenticated,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: Arc<AppState>) {
    info!("WebSocket client connected");
    connection::handle_connection(
        socket,
        state.runtime.clone(),
        state.event_bus.clone(),
        state.store.clone(),
        state.session_mgr.clone(),
        state.broker.clone(),
    )
    .await;
    debug!("WebSocket client disconnected");
}
