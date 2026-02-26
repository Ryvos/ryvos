pub mod agent_loop;
pub mod approval;
pub mod checkpoint;
pub mod context;
pub mod evaluator;
pub mod gate;
pub mod graph;
pub mod guardian;
pub mod heartbeat;
pub mod healing;
pub mod intelligence;
pub mod judge;
pub mod orchestrator;
pub mod output_validator;
pub mod prime;
pub mod run_log;
pub mod scheduler;
pub mod session;

pub use agent_loop::AgentRuntime;
pub use approval::ApprovalBroker;
pub use checkpoint::CheckpointStore;
pub use gate::SecurityGate;
pub use graph::{Edge, EdgeCondition, ExecutionResult, GraphExecutor, HandoffContext, Node, NodeResult};
pub use guardian::{Guardian, GuardianAction};
pub use heartbeat::Heartbeat;
pub use prime::PrimeOrchestrator;
pub use healing::FailureJournal;
pub use evaluator::GoalEvaluator;
pub use judge::Judge;
pub use orchestrator::{AgentCapability, MultiAgentOrchestrator, OrchestratorBuilder};
pub use output_validator::{OutputCleaner, OutputValidator};
pub use run_log::RunLogger;
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
