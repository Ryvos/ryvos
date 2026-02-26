use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use tracing::{debug, info, warn};

use ryvos_core::config::AppConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::event::EventBus;
use ryvos_core::goal::Goal;
use ryvos_core::security::SecurityPolicy;
use ryvos_core::traits::{LlmClient, SessionStore};
use ryvos_core::types::{AgentSpawner, SessionId};
use ryvos_tools::ToolRegistry;

use crate::approval::ApprovalBroker;
use crate::gate::SecurityGate;
use crate::AgentRuntime;

/// Describes an agent's capabilities for routing decisions.
#[derive(Debug, Clone)]
pub struct AgentCapability {
    /// Unique agent identifier.
    pub agent_id: String,
    /// Human-readable name.
    pub name: String,
    /// Tool names this agent has access to.
    pub tools: Vec<String>,
    /// Domain specializations (e.g., "code", "research", "writing").
    pub specializations: Vec<String>,
    /// Optional security policy override for this agent.
    pub policy: Option<SecurityPolicy>,
    /// Optional goal for this agent.
    pub goal: Option<Goal>,
    /// Optional model override for this agent (per-agent model routing).
    pub model: Option<ryvos_core::config::ModelConfig>,
}

impl AgentCapability {
    /// Create a new agent capability descriptor.
    pub fn new(agent_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            name: name.into(),
            tools: vec![],
            specializations: vec![],
            policy: None,
            goal: None,
            model: None,
        }
    }

    /// Set a model override for this agent.
    pub fn with_model(mut self, model: ryvos_core::config::ModelConfig) -> Self {
        self.model = Some(model);
        self
    }

    /// Set the tools this agent can use.
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = tools;
        self
    }

    /// Set the specializations.
    pub fn with_specializations(mut self, specs: Vec<String>) -> Self {
        self.specializations = specs;
        self
    }

    /// Set a security policy override.
    pub fn with_policy(mut self, policy: SecurityPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Set a goal for this agent.
    pub fn with_goal(mut self, goal: Goal) -> Self {
        self.goal = Some(goal);
        self
    }

    /// Score how well this agent matches a task description.
    /// Higher score = better match.
    pub fn match_score(&self, task: &str, required_tools: &[String]) -> f64 {
        let mut score = 0.0;
        let task_lower = task.to_lowercase();

        // Check specialization matches
        for spec in &self.specializations {
            if task_lower.contains(&spec.to_lowercase()) {
                score += 10.0;
            }
        }

        // Check tool availability
        if !required_tools.is_empty() {
            let matched_tools = required_tools
                .iter()
                .filter(|t| self.tools.contains(t))
                .count();
            if matched_tools == required_tools.len() {
                score += 20.0; // All required tools available
            } else {
                score += matched_tools as f64 * 5.0;
            }
        }

        score
    }
}

/// Dispatch mode for multi-agent orchestration.
#[derive(Debug, Clone)]
pub enum DispatchMode {
    /// Run tasks in parallel across agents, collect all results.
    Parallel,
    /// Chain tasks: output of one agent feeds into the next.
    Relay,
    /// Send the same task to all agents, collect all results.
    Broadcast,
}

/// Result from a single agent dispatch.
#[derive(Debug, Clone)]
pub struct AgentDispatchResult {
    /// Which agent handled this.
    pub agent_id: String,
    /// The agent's output.
    pub output: String,
    /// Whether the agent succeeded.
    pub succeeded: bool,
}

/// Shared infrastructure for building agent runtimes.
pub struct OrchestratorBuilder {
    pub config: AppConfig,
    pub llm: Arc<dyn LlmClient>,
    pub tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
    pub store: Arc<dyn SessionStore>,
    pub event_bus: Arc<EventBus>,
    pub broker: Arc<ApprovalBroker>,
}

/// Multi-Agent Orchestrator with capability-based routing.
///
/// Extends the existing PrimeOrchestrator pattern with:
/// - Agent registry with capability descriptors
/// - Task routing to best-matching agent
/// - Parallel, relay, and broadcast dispatch modes
pub struct MultiAgentOrchestrator {
    agents: HashMap<String, AgentCapability>,
    default_policy: SecurityPolicy,
    builder: OrchestratorBuilder,
}

impl MultiAgentOrchestrator {
    /// Create a new orchestrator.
    pub fn new(default_policy: SecurityPolicy, builder: OrchestratorBuilder) -> Self {
        Self {
            agents: HashMap::new(),
            default_policy,
            builder,
        }
    }

    /// Register an agent with its capabilities.
    pub fn register(&mut self, capability: AgentCapability) {
        self.agents.insert(capability.agent_id.clone(), capability);
    }

    /// Unregister an agent.
    pub fn unregister(&mut self, agent_id: &str) -> Option<AgentCapability> {
        self.agents.remove(agent_id)
    }

    /// List all registered agents.
    pub fn agents(&self) -> Vec<&AgentCapability> {
        self.agents.values().collect()
    }

    /// Find the best agent for a given task.
    /// Returns None if no agents are registered.
    pub fn route(&self, task: &str, required_tools: &[String]) -> Option<&AgentCapability> {
        self.agents
            .values()
            .map(|a| (a, a.match_score(task, required_tools)))
            .max_by(|(_, sa), (_, sb)| sa.partial_cmp(sb).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(a, _)| a)
    }

    /// Dispatch a task to the best-matching agent.
    pub async fn dispatch(&self, task: &str) -> Result<AgentDispatchResult> {
        let agent = self
            .route(task, &[])
            .ok_or_else(|| RyvosError::Config("No agents registered".into()))?;

        info!(agent_id = %agent.agent_id, name = %agent.name, "Routing task to agent");
        self.run_agent(agent, task).await
    }

    /// Dispatch a task to a specific agent by id.
    pub async fn dispatch_to(&self, agent_id: &str, task: &str) -> Result<AgentDispatchResult> {
        let agent = self
            .agents
            .get(agent_id)
            .ok_or_else(|| RyvosError::Config(format!("Agent '{}' not found", agent_id)))?;

        info!(agent_id = %agent.agent_id, name = %agent.name, "Dispatching task to specific agent");
        self.run_agent(agent, task).await
    }

    /// Dispatch tasks across multiple agents according to the dispatch mode.
    pub async fn dispatch_multi(
        &self,
        tasks: Vec<(String, String)>, // (agent_id, task)
        mode: DispatchMode,
    ) -> Result<Vec<AgentDispatchResult>> {
        match mode {
            DispatchMode::Parallel => self.dispatch_parallel(tasks).await,
            DispatchMode::Relay => self.dispatch_relay(tasks).await,
            DispatchMode::Broadcast => {
                if let Some((_, task)) = tasks.first() {
                    self.dispatch_broadcast(task).await
                } else {
                    Ok(vec![])
                }
            }
        }
    }

    /// Run tasks in parallel across specified agents.
    async fn dispatch_parallel(
        &self,
        tasks: Vec<(String, String)>,
    ) -> Result<Vec<AgentDispatchResult>> {
        let futs: Vec<_> = tasks
            .iter()
            .filter_map(|(agent_id, task)| {
                let agent = self.agents.get(agent_id)?;
                Some(self.run_agent(agent, task))
            })
            .collect();

        let results: Vec<Result<AgentDispatchResult>> = futures::future::join_all(futs).await;

        let mut outputs = Vec::new();
        for result in results {
            match result {
                Ok(r) => outputs.push(r),
                Err(e) => {
                    warn!(error = %e, "Parallel agent dispatch failed");
                    outputs.push(AgentDispatchResult {
                        agent_id: "unknown".into(),
                        output: e.to_string(),
                        succeeded: false,
                    });
                }
            }
        }

        Ok(outputs)
    }

    /// Chain tasks: output of agent N becomes input prefix for agent N+1.
    async fn dispatch_relay(
        &self,
        tasks: Vec<(String, String)>,
    ) -> Result<Vec<AgentDispatchResult>> {
        let mut results = Vec::new();
        let mut previous_output: Option<String> = None;

        for (agent_id, task) in &tasks {
            let agent = self
                .agents
                .get(agent_id)
                .ok_or_else(|| RyvosError::Config(format!("Agent '{}' not found", agent_id)))?;

            let prompt = if let Some(ref prev) = previous_output {
                format!(
                    "Previous agent output:\n---\n{}\n---\n\nYour task: {}",
                    prev, task
                )
            } else {
                task.clone()
            };

            let result = self.run_agent(agent, &prompt).await?;
            previous_output = Some(result.output.clone());
            results.push(result);
        }

        Ok(results)
    }

    /// Send the same task to all registered agents.
    async fn dispatch_broadcast(&self, task: &str) -> Result<Vec<AgentDispatchResult>> {
        let futs: Vec<_> = self
            .agents
            .values()
            .map(|agent| self.run_agent(agent, task))
            .collect();

        let results: Vec<Result<AgentDispatchResult>> = futures::future::join_all(futs).await;

        let mut outputs = Vec::new();
        for result in results {
            match result {
                Ok(r) => outputs.push(r),
                Err(e) => {
                    warn!(error = %e, "Broadcast agent dispatch failed");
                }
            }
        }

        Ok(outputs)
    }

    /// Run a single agent with its configured policy and optional goal.
    async fn run_agent(
        &self,
        capability: &AgentCapability,
        task: &str,
    ) -> Result<AgentDispatchResult> {
        let policy = capability
            .policy
            .clone()
            .unwrap_or_else(|| self.default_policy.clone());

        let gate = Arc::new(SecurityGate::new(
            policy,
            self.builder.tools.clone(),
            self.builder.broker.clone(),
            self.builder.event_bus.clone(),
        ));

        // Use per-agent model override if configured
        let llm: Arc<dyn LlmClient> = if let Some(ref model_config) = capability.model {
            Arc::from(ryvos_llm::create_client(model_config))
        } else {
            self.builder.llm.clone()
        };

        let mut config = self.builder.config.clone();
        if let Some(ref model_config) = capability.model {
            config.model = model_config.clone();
        }

        let runtime = AgentRuntime::new_with_gate(
            config,
            llm,
            gate,
            self.builder.store.clone(),
            self.builder.event_bus.clone(),
        );

        let session = SessionId::new();
        let result = if let Some(ref goal) = capability.goal {
            runtime.run_with_goal(&session, task, Some(goal)).await
        } else {
            runtime.run(&session, task).await
        };

        match result {
            Ok(output) => {
                debug!(agent_id = %capability.agent_id, "Agent completed successfully");
                Ok(AgentDispatchResult {
                    agent_id: capability.agent_id.clone(),
                    output,
                    succeeded: true,
                })
            }
            Err(e) => {
                warn!(agent_id = %capability.agent_id, error = %e, "Agent failed");
                Ok(AgentDispatchResult {
                    agent_id: capability.agent_id.clone(),
                    output: e.to_string(),
                    succeeded: false,
                })
            }
        }
    }
}

impl AgentSpawner for MultiAgentOrchestrator {
    fn spawn(&self, prompt: String) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            let result = self.dispatch(&prompt).await?;
            Ok(result.output)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_builder() {
        let cap = AgentCapability::new("coder", "Code Agent")
            .with_tools(vec!["bash".into(), "write_file".into()])
            .with_specializations(vec!["code".into(), "debug".into()]);

        assert_eq!(cap.agent_id, "coder");
        assert_eq!(cap.name, "Code Agent");
        assert_eq!(cap.tools.len(), 2);
        assert_eq!(cap.specializations.len(), 2);
    }

    #[test]
    fn test_match_score_specialization() {
        let cap = AgentCapability::new("writer", "Writer")
            .with_specializations(vec!["writing".into(), "editing".into()]);

        let score = cap.match_score("I need help writing an article", &[]);
        assert!(score > 0.0);

        let score_no_match = cap.match_score("Fix the database query", &[]);
        assert_eq!(score_no_match, 0.0);
    }

    #[test]
    fn test_match_score_tools() {
        let cap = AgentCapability::new("coder", "Coder")
            .with_tools(vec!["bash".into(), "write_file".into()]);

        let required = vec!["bash".into(), "write_file".into()];
        let score = cap.match_score("task", &required);
        assert!(score >= 20.0); // Full tool match

        let partial = vec!["bash".into(), "search".into()];
        let partial_score = cap.match_score("task", &partial);
        assert!(partial_score > 0.0);
        assert!(partial_score < score);
    }

    #[test]
    fn test_match_score_combined() {
        let cap = AgentCapability::new("coder", "Coder")
            .with_tools(vec!["bash".into()])
            .with_specializations(vec!["code".into()]);

        let score = cap.match_score("write some code", &["bash".into()]);
        // specialization match (10) + full tool match (20)
        assert!(score >= 30.0);
    }

    #[test]
    fn test_dispatch_result() {
        let result = AgentDispatchResult {
            agent_id: "coder".into(),
            output: "Done!".into(),
            succeeded: true,
        };
        assert!(result.succeeded);
        assert_eq!(result.agent_id, "coder");
    }

    #[test]
    fn test_route_best_match() {
        // We can't build a full MultiAgentOrchestrator in a unit test without
        // all the runtime deps, but we can test the routing logic directly.
        let agents = [
            AgentCapability::new("writer", "Writer").with_specializations(vec!["writing".into()]),
            AgentCapability::new("coder", "Coder")
                .with_specializations(vec!["code".into(), "debug".into()])
                .with_tools(vec!["bash".into()]),
        ];

        // Simulate routing
        let task = "debug this code issue";
        let required: Vec<String> = vec![];
        let best = agents
            .iter()
            .map(|a| (a, a.match_score(task, &required)))
            .max_by(|(_, sa), (_, sb)| sa.partial_cmp(sb).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(a, _)| a);

        assert!(best.is_some());
        assert_eq!(best.unwrap().agent_id, "coder");
    }
}
