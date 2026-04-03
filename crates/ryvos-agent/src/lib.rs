//! Agent runtime, orchestration, and safety systems for Ryvos.
//!
//! This is the largest and most complex crate in the workspace. It contains:
//!
//! - **ReAct loop** ([`AgentRuntime`]): The core agent execution loop that
//!   streams from the LLM, executes tools, evaluates goals, and manages context.
//! - **Director** ([`Director`]): Goal-driven OODA loop orchestration that
//!   generates DAG workflows, executes them, evaluates results, and auto-evolves.
//! - **Guardian** ([`Guardian`]): Background watchdog that detects doom loops,
//!   stalls, and budget overruns, injecting corrective hints.
//! - **SecurityGate** ([`SecurityGate`]): Passthrough security layer that
//!   audits every tool call, consults safety lessons, and optionally pauses
//!   for approval, but never blocks execution.
//! - **SafetyMemory** ([`SafetyMemory`]): Self-learning safety database that
//!   detects destructive patterns, records lessons, and injects context.
//! - **FailureJournal** ([`FailureJournal`]): Tracks tool failures, detects
//!   patterns, and provides reflexion hints for self-healing.
//! - **Intelligence**: Token budgeting, message pruning, and summarization.
//! - **Graph**: DAG workflow engine with nodes, edges, and conditional routing.

pub mod agent_loop;
pub mod approval;
pub mod audit;
pub mod checkpoint;
pub mod context;
pub mod director;
pub mod evaluator;
pub mod gate;
pub mod graph;
pub mod guardian;
pub mod healing;
pub mod heartbeat;
pub mod intelligence;
pub mod judge;
pub mod orchestrator;
pub mod output_validator;
pub mod prime;
pub mod run_log;
pub mod safety_memory;
pub mod scheduler;
pub mod session;

pub use agent_loop::AgentRuntime;
pub use approval::ApprovalBroker;
pub use audit::AuditTrail;
pub use checkpoint::CheckpointStore;
pub use director::Director;
pub use evaluator::GoalEvaluator;
pub use gate::SecurityGate;
pub use graph::{
    Edge, EdgeCondition, ExecutionResult, GraphExecutor, HandoffContext, Node, NodeResult,
};
pub use guardian::{Guardian, GuardianAction};
pub use healing::FailureJournal;
pub use heartbeat::Heartbeat;
pub use judge::Judge;
pub use orchestrator::{AgentCapability, MultiAgentOrchestrator, OrchestratorBuilder};
pub use output_validator::{OutputCleaner, OutputValidator};
pub use prime::PrimeOrchestrator;
pub use run_log::RunLogger;
pub use safety_memory::SafetyMemory;
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
