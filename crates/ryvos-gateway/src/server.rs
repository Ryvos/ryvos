use std::sync::Arc;
use std::time::Instant;

use axum::routing::{get, post};
use axum::Router;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tracing::info;

use ryvos_agent::{AgentRuntime, ApprovalBroker, SessionManager};
use ryvos_channels::WhatsAppWebhookHandle;
use ryvos_core::config::{BudgetConfig, GatewayConfig};
use ryvos_core::event::EventBus;
use ryvos_core::traits::SessionStore;
use ryvos_memory::CostStore;

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
    whatsapp_handle: Option<WhatsAppWebhookHandle>,
    cost_store: Option<Arc<CostStore>>,
    budget_config: Option<BudgetConfig>,
    start_time: Instant,
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
            whatsapp_handle: None,
            cost_store: None,
            budget_config: None,
            start_time: Instant::now(),
        }
    }

    /// Set the WhatsApp webhook handle for routing incoming messages.
    pub fn set_whatsapp_handle(&mut self, handle: WhatsAppWebhookHandle) {
        self.whatsapp_handle = Some(handle);
    }

    /// Set the cost store and budget config for monitoring dashboard.
    pub fn set_cost_store(&mut self, cost_store: Arc<CostStore>, budget_config: Option<BudgetConfig>) {
        self.cost_store = Some(cost_store);
        self.budget_config = budget_config;
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
            whatsapp_handle: self.whatsapp_handle.clone(),
            cost_store: self.cost_store.clone(),
            budget_config: self.budget_config.clone(),
            start_time: self.start_time,
        });

        let app = Router::new()
            // WebSocket
            .route("/ws", get(routes::ws_handler))
            // REST API
            .route("/api/health", get(routes::health))
            .route("/api/sessions", get(routes::list_sessions))
            .route("/api/sessions/{id}/history", get(routes::session_history))
            .route("/api/sessions/{id}/messages", post(routes::send_message))
            // Monitoring dashboard API
            .route("/api/metrics", get(routes::metrics))
            .route("/api/runs", get(routes::runs))
            .route("/api/costs", get(routes::costs))
            // Webhooks
            .route("/api/hooks/wake", post(routes::webhook_wake))
            // WhatsApp Cloud API webhooks
            .route("/api/whatsapp/webhook", get(routes::whatsapp_verify))
            .route("/api/whatsapp/webhook", post(routes::whatsapp_incoming))
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
