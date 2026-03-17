//! Lightweight Viking-compatible HTTP server (Rust-native).
//!
//! Implements the same REST API that `VikingClient` expects, backed by SQLite+FTS5.
//! No Python, no Docker — pure Rust, runs on ARM64 with ~5-15MB RAM.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tracing::info;

use ryvos_memory::viking::{VikingDirEntry, VikingMeta, VikingResult};
use ryvos_memory::viking_store::VikingStore;

type AppState = Arc<VikingStore>;

// ── Request/Response types ──────────────────────────────────────

#[derive(Deserialize)]
struct WriteRequest {
    user_id: String,
    path: String,
    content: String,
    #[serde(default)]
    metadata: VikingMeta,
}

#[derive(Deserialize)]
struct ReadQuery {
    user_id: String,
    path: String,
    #[serde(default = "default_level")]
    level: String,
}

fn default_level() -> String {
    "L1".to_string()
}

#[derive(Deserialize)]
struct SearchQuery {
    user_id: String,
    query: String,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    10
}

#[derive(Deserialize)]
struct ListQuery {
    user_id: String,
    path: String,
}

#[derive(Deserialize)]
struct DeleteQuery {
    user_id: String,
    path: String,
}

#[derive(Deserialize)]
struct IterateRequest {
    user_id: String,
    transcript: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    entries: u64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct IterateResponse {
    extracted: usize,
}

// ── Handlers ────────────────────────────────────────────────────

async fn health(State(store): State<AppState>) -> Json<HealthResponse> {
    let entries = store.count("ryvos-default").unwrap_or(0);
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        entries,
    })
}

async fn write_memory(
    State(store): State<AppState>,
    Json(req): Json<WriteRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    store.write(&req.user_id, &req.path, &req.content, &req.metadata)
        .map(|_| StatusCode::OK)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e })))
}

async fn read_memory(
    State(store): State<AppState>,
    Query(q): Query<ReadQuery>,
) -> Result<Json<VikingResult>, (StatusCode, Json<ErrorResponse>)> {
    let level = match q.level.as_str() {
        "L0" => ryvos_memory::viking::ContextLevel::L0,
        "L2" => ryvos_memory::viking::ContextLevel::L2,
        _ => ryvos_memory::viking::ContextLevel::L1,
    };
    store.read(&q.user_id, &q.path, level)
        .map(Json)
        .map_err(|e| (StatusCode::NOT_FOUND, Json(ErrorResponse { error: e })))
}

async fn search_memory(
    State(store): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Vec<VikingResult>>, (StatusCode, Json<ErrorResponse>)> {
    store.search(&q.user_id, &q.query, q.directory.as_deref(), q.limit)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e })))
}

async fn list_directory(
    State(store): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<VikingDirEntry>>, (StatusCode, Json<ErrorResponse>)> {
    store.list_directory(&q.user_id, &q.path)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e })))
}

async fn delete_memory(
    State(store): State<AppState>,
    Query(q): Query<DeleteQuery>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    store.delete(&q.user_id, &q.path)
        .map(|_| StatusCode::OK)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e })))
}

async fn iterate(
    State(store): State<AppState>,
    Json(req): Json<IterateRequest>,
) -> Result<Json<IterateResponse>, (StatusCode, Json<ErrorResponse>)> {
    store.iterate(&req.user_id, &req.transcript)
        .map(|count| Json(IterateResponse { extracted: count }))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e })))
}

// ── Server ──────────────────────────────────────────────────────

fn build_router(store: Arc<VikingStore>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/memory/write", post(write_memory))
        .route("/api/memory/read", get(read_memory))
        .route("/api/memory/search", get(search_memory))
        .route("/api/memory/list", get(list_directory))
        .route("/api/memory/delete", delete(delete_memory))
        .route("/api/memory/iterate", post(iterate))
        .layer(CorsLayer::permissive())
        .with_state(store)
}

/// Run the Viking server as a standalone process.
pub async fn run_standalone(bind: &str, db_path: &std::path::Path) -> anyhow::Result<()> {
    let store = Arc::new(VikingStore::open(db_path).map_err(|e| anyhow::anyhow!(e))?);
    let entries = store.count("ryvos-default").unwrap_or(0);
    info!(bind = bind, entries = entries, "Viking server starting");

    let app = build_router(store);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    info!(bind = bind, "Viking server listening");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Spawn the Viking server as a background task (for daemon integration).
/// Returns a join handle.
pub fn spawn_background(
    bind: String,
    db_path: std::path::PathBuf,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let store = match VikingStore::open(&db_path) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                tracing::error!(error = %e, "Failed to open Viking store");
                return;
            }
        };

        let entries = store.count("ryvos-default").unwrap_or(0);
        info!(bind = %bind, entries = entries, "Viking server starting (background)");

        let app = build_router(store);
        let listener = match tokio::net::TcpListener::bind(&bind).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(bind = %bind, error = %e, "Viking server failed to bind");
                return;
            }
        };

        info!(bind = %bind, "Viking server listening (background)");

        tokio::select! {
            result = axum::serve(listener, app) => {
                if let Err(e) = result {
                    tracing::error!(error = %e, "Viking server error");
                }
            }
            _ = cancel.cancelled() => {
                info!("Viking server shutting down");
            }
        }
    })
}
