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

// ── Monitoring dashboard endpoints ─────────────────────────────

#[derive(Deserialize)]
pub struct RunsQuery {
    #[serde(default = "default_runs_limit")]
    pub limit: u64,
    #[serde(default)]
    pub offset: u64,
}

fn default_runs_limit() -> u64 {
    50
}

#[derive(Deserialize)]
pub struct CostsQuery {
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default = "default_group_by")]
    pub group_by: String,
}

fn default_group_by() -> String {
    "model".to_string()
}

// GET /api/metrics — overview metrics for the dashboard
pub async fn metrics(
    Authenticated(auth_result): Authenticated,
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth::has_viewer_access(&auth_result.role) {
        return Err(StatusCode::FORBIDDEN);
    }

    let sessions = state.session_mgr.list();
    let uptime_secs = state.start_time.elapsed().as_secs();

    let (total_runs, total_tokens, total_cost_cents) =
        if let Some(ref cost_store) = state.cost_store {
            let (_runs, _) = cost_store
                .run_history(0, 0)
                .unwrap_or((vec![], 0));
            let monthly = cost_store.monthly_spend_cents().unwrap_or(0);
            // Sum tokens from run history total count
            let total_count = cost_store
                .run_history(1, 0)
                .map(|(_, t)| t)
                .unwrap_or(0);
            (total_count, 0u64, monthly)
        } else {
            (0, 0, 0)
        };

    let monthly_budget_cents = state
        .budget_config
        .as_ref()
        .map(|b| b.monthly_budget_cents)
        .unwrap_or(0);

    let budget_utilization_pct = if monthly_budget_cents > 0 {
        (total_cost_cents as f64 / monthly_budget_cents as f64 * 100.0) as u64
    } else {
        0
    };

    Ok(Json(serde_json::json!({
        "total_runs": total_runs,
        "active_sessions": sessions.len(),
        "total_tokens": total_tokens,
        "total_cost_cents": total_cost_cents,
        "monthly_budget_cents": monthly_budget_cents,
        "budget_utilization_pct": budget_utilization_pct,
        "uptime_secs": uptime_secs,
    })))
}

// GET /api/runs — paginated run history
pub async fn runs(
    Authenticated(auth_result): Authenticated,
    State(state): State<Arc<AppState>>,
    Query(q): Query<RunsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth::has_viewer_access(&auth_result.role) {
        return Err(StatusCode::FORBIDDEN);
    }

    let cost_store = state.cost_store.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    let (runs, total) = cost_store
        .run_history(q.limit, q.offset)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "runs": runs,
        "total": total,
        "offset": q.offset,
        "limit": q.limit,
    })))
}

// GET /api/costs — cost summary with breakdown
pub async fn costs(
    Authenticated(auth_result): Authenticated,
    State(state): State<Arc<AppState>>,
    Query(q): Query<CostsQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth::has_viewer_access(&auth_result.role) {
        return Err(StatusCode::FORBIDDEN);
    }

    let cost_store = state.cost_store.as_ref().ok_or(StatusCode::NOT_FOUND)?;

    let from = q
        .from
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|| chrono::Utc::now() - chrono::Duration::days(30));

    let to = q
        .to
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now);

    let summary = cost_store
        .cost_summary(&from, &to)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let breakdown = cost_store
        .cost_by_group(&from, &to, &q.group_by)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let breakdown_json: Vec<serde_json::Value> = breakdown
        .into_iter()
        .map(|(key, cost, input, output)| {
            serde_json::json!({
                "key": key,
                "cost_cents": cost,
                "input_tokens": input,
                "output_tokens": output,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "summary": summary,
        "breakdown": breakdown_json,
        "from": from.to_rfc3339(),
        "to": to.to_rfc3339(),
        "group_by": q.group_by,
    })))
}

// ── Webhook endpoint ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct WebhookPayload {
    pub prompt: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub callback_url: Option<String>,
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

    let callback_url = body.callback_url.clone();
    let channel = body.channel.clone();
    let metadata = body.metadata.clone();

    match state.runtime.run(&session_id, &body.prompt).await {
        Ok(response) => {
            // Fire callback if provided
            if let Some(url) = callback_url.clone() {
                let client = reqwest::Client::new();
                let md = metadata.clone();
                let cb_body = serde_json::json!({
                    "session_id": session_id.to_string(),
                    "response": response,
                    "metadata": md,
                });
                tokio::spawn(async move {
                    if let Err(e) = client.post(&url).json(&cb_body).send().await {
                        tracing::warn!(url = %url, error = %e, "Webhook callback failed");
                    }
                });
            }

            Ok(Json(serde_json::json!({
                "session_id": session_id.to_string(),
                "response": response,
                "channel": channel,
                "metadata": metadata,
            })))
        }
        Err(e) => {
            // Fire callback with error
            if let Some(url) = callback_url {
                let client = reqwest::Client::new();
                let cb_body = serde_json::json!({
                    "session_id": session_id.to_string(),
                    "error": e.to_string(),
                    "metadata": metadata,
                });
                tokio::spawn(async move {
                    if let Err(e) = client.post(&url).json(&cb_body).send().await {
                        tracing::warn!(url = %url, error = %e, "Webhook callback failed");
                    }
                });
            }

            Ok(Json(serde_json::json!({
                "session_id": session_id.to_string(),
                "error": e.to_string(),
            })))
        }
    }
}

// ── WhatsApp webhook endpoints ───────────────────────────────

#[derive(Deserialize)]
pub struct WhatsAppVerifyQuery {
    #[serde(rename = "hub.mode", default)]
    pub mode: String,
    #[serde(rename = "hub.verify_token", default)]
    pub verify_token: String,
    #[serde(rename = "hub.challenge", default)]
    pub challenge: String,
}

/// GET /api/whatsapp/webhook — Meta verification handshake.
pub async fn whatsapp_verify(
    State(state): State<Arc<AppState>>,
    Query(q): Query<WhatsAppVerifyQuery>,
) -> Result<String, StatusCode> {
    let handle = state
        .whatsapp_handle
        .as_ref()
        .ok_or(StatusCode::NOT_FOUND)?;

    handle
        .verify_webhook(&q.mode, &q.verify_token, &q.challenge)
        .ok_or(StatusCode::FORBIDDEN)
}

/// POST /api/whatsapp/webhook — Incoming messages from Meta.
pub async fn whatsapp_incoming(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> StatusCode {
    if let Some(ref handle) = state.whatsapp_handle {
        handle.process_webhook(body).await;
    }
    StatusCode::OK
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
