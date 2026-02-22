use std::sync::Arc;

use ryvos_agent::{AgentRuntime, ApprovalBroker, SessionManager};
use ryvos_core::config::GatewayConfig;
use ryvos_core::event::EventBus;
use ryvos_core::traits::SessionStore;

/// Shared application state for axum handlers.
pub struct AppState {
    pub config: GatewayConfig,
    pub runtime: Arc<AgentRuntime>,
    pub event_bus: Arc<EventBus>,
    pub store: Arc<dyn SessionStore>,
    pub session_mgr: Arc<SessionManager>,
    pub broker: Arc<ApprovalBroker>,
}
