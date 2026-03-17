use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ryvos_agent::{AgentRuntime, ApprovalBroker, AuditTrail, SessionManager};
use ryvos_channels::WhatsAppWebhookHandle;
use ryvos_core::config::{BudgetConfig, GatewayConfig};
use ryvos_core::event::EventBus;
use ryvos_core::traits::SessionStore;
use ryvos_memory::{CostStore, VikingClient};

/// Shared application state for axum handlers.
pub struct AppState {
    pub config: GatewayConfig,
    pub runtime: Arc<AgentRuntime>,
    pub event_bus: Arc<EventBus>,
    pub store: Arc<dyn SessionStore>,
    pub session_mgr: Arc<SessionManager>,
    pub broker: Arc<ApprovalBroker>,
    pub whatsapp_handle: Option<WhatsAppWebhookHandle>,
    pub cost_store: Option<Arc<CostStore>>,
    pub budget_config: Option<BudgetConfig>,
    pub start_time: Instant,
    pub audit_trail: Option<Arc<AuditTrail>>,
    pub viking_client: Option<Arc<VikingClient>>,
    pub config_path: Option<PathBuf>,
}
