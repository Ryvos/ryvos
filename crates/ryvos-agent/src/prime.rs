use std::sync::Arc;

use futures::future::BoxFuture;

use ryvos_core::config::AppConfig;
use ryvos_core::error::Result;
use ryvos_core::event::EventBus;
use ryvos_core::security::SecurityPolicy;
use ryvos_core::traits::{LlmClient, SessionStore};
use ryvos_core::types::{AgentSpawner, SessionId};
use ryvos_tools::ToolRegistry;

use crate::approval::ApprovalBroker;
use crate::gate::SecurityGate;
use crate::AgentRuntime;

/// Holds everything needed to build a restricted AgentRuntime.
pub struct PrimeRuntimeBuilder {
    pub config: AppConfig,
    pub llm: Arc<dyn LlmClient>,
    pub tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
    pub store: Arc<dyn SessionStore>,
    pub event_bus: Arc<EventBus>,
    pub broker: Arc<ApprovalBroker>,
}

/// PRIME Orchestrator — supervises sub-agents with restricted policies.
pub struct PrimeOrchestrator {
    sub_agent_policy: SecurityPolicy,
    builder: PrimeRuntimeBuilder,
}

impl PrimeOrchestrator {
    pub fn new(
        sub_agent_policy: SecurityPolicy,
        builder: PrimeRuntimeBuilder,
    ) -> Self {
        Self {
            sub_agent_policy,
            builder,
        }
    }

    /// Spawn a sub-agent with restricted security policy.
    pub async fn spawn_restricted(&self, prompt: &str) -> Result<String> {
        let gate = Arc::new(SecurityGate::new(
            self.sub_agent_policy.clone(),
            self.builder.tools.clone(),
            self.builder.broker.clone(),
            self.builder.event_bus.clone(),
        ));

        let runtime = AgentRuntime::new_with_gate(
            self.builder.config.clone(),
            self.builder.llm.clone(),
            gate,
            self.builder.store.clone(),
            self.builder.event_bus.clone(),
        );

        let session = SessionId::new();
        runtime.run(&session, prompt).await
    }
}

impl AgentSpawner for PrimeOrchestrator {
    fn spawn(&self, prompt: String) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move { self.spawn_restricted(&prompt).await })
    }
}
