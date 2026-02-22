pub mod agent_loop;
pub mod approval;
pub mod context;
pub mod evaluator;
pub mod gate;
pub mod healing;
pub mod intelligence;
pub mod prime;
pub mod scheduler;
pub mod session;

pub use agent_loop::AgentRuntime;
pub use approval::ApprovalBroker;
pub use gate::SecurityGate;
pub use prime::PrimeOrchestrator;
pub use healing::FailureJournal;
pub use scheduler::CronScheduler;
pub use session::SessionManager;

use futures::future::BoxFuture;
use ryvos_core::error::Result;
use ryvos_core::types::{AgentSpawner, SessionId};

impl AgentSpawner for AgentRuntime {
    fn spawn(&self, prompt: String) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            let sub_session = SessionId::new();
            self.run(&sub_session, &prompt).await
        })
    }
}
