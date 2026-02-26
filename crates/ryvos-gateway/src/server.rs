use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tracing::info;

use ryvos_agent::{AgentRuntime, ApprovalBroker, SessionManager};
use ryvos_core::config::GatewayConfig;
use ryvos_core::event::EventBus;
use ryvos_core::traits::SessionStore;

use crate::routes;
use crate::state::AppState;
use crate::static_files;

/// WebSocket + HTTP gateway server built on axum.
pub struct GatewayServer {
    config: GatewayConfig,
    runtime: Arc<AgentRuntime>,
    event_bus: Arc<EventBus>,
    store: Arc<dyn SessionStore>,
    session_mgr: Arc<SessionManager>,
    broker: Arc<ApprovalBroker>,
}

impl GatewayServer {
    pub fn new(
        config: GatewayConfig,
        runtime: Arc<AgentRuntime>,
        event_bus: Arc<EventBus>,
        store: Arc<dyn SessionStore>,
        session_mgr: Arc<SessionManager>,
        broker: Arc<ApprovalBroker>,
    ) -> Self {
        Self {
            config,
            runtime,
            event_bus,
            store,
            session_mgr,
            broker,
        }
    }

    /// Run the gateway server until the cancellation token is triggered.
    pub async fn run(&self, shutdown: CancellationToken) -> anyhow::Result<()> {
        let state = Arc::new(AppState {
            config: self.config.clone(),
            runtime: self.runtime.clone(),
            event_bus: self.event_bus.clone(),
            store: self.store.clone(),
            session_mgr: self.session_mgr.clone(),
            broker: self.broker.clone(),
        });

        let app = Router::new()
            // WebSocket
            .route("/ws", get(routes::ws_handler))
            // REST API
            .route("/api/health", get(routes::health))
            .route("/api/sessions", get(routes::list_sessions))
            .route("/api/sessions/{id}/history", get(routes::session_history))
            .route("/api/sessions/{id}/messages", post(routes::send_message))
            // Webhooks
            .route("/api/hooks/wake", post(routes::webhook_wake))
            // Embedded Web UI
            .route("/", get(static_files::index))
            .route("/assets/{*path}", get(static_files::static_file))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let listener = TcpListener::bind(&self.config.bind).await?;
        info!(bind = %self.config.bind, "Gateway listening");

        axum::serve(listener, app)
            .with_graceful_shutdown(async move { shutdown.cancelled().await })
            .await?;

        info!("Gateway shut down");
        Ok(())
    }
}
