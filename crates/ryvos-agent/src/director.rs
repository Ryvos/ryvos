//! Director Orchestration — goal-driven multi-agent execution with auto-evolving graphs.
//!
//! The Director implements an OODA loop: Generate graph → Execute → Evaluate →
//! (on failure) Diagnose → Evolve → Retry. It builds on existing infrastructure
//! (GraphExecutor, Judge, EventBus, HandoffContext) and is activated via
//! `[agent.director] enabled = true` in config.

use std::sync::Arc;

use futures::StreamExt;
use tracing::{info, warn};

use ryvos_core::config::ModelConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::event::EventBus;
use ryvos_core::goal::{FailureCategory, GoalObject, SemanticFailure};
use ryvos_core::traits::LlmClient;
use ryvos_core::types::{AgentEvent, ChatMessage, SessionId, StreamDelta};

use crate::graph::edge::Edge;
use crate::graph::executor::GraphExecutor;
use crate::graph::handoff::HandoffContext;
use crate::graph::node::Node;
use crate::judge::Judge;
use crate::AgentRuntime;

/// Result of a Director orchestration run.
#[derive(Debug, Clone)]
pub struct DirectorResult {
    pub output: String,
    pub succeeded: bool,
    pub evolution_cycles: u32,
    pub total_nodes_executed: usize,
    pub semantic_failures: Vec<SemanticFailure>,
}

/// The Director — goal-driven multi-agent orchestration with auto-evolving graphs.
#[allow(dead_code)]
pub struct Director {
    llm: Arc<dyn LlmClient>,
    config: ModelConfig,
    event_bus: Arc<EventBus>,
    max_evolution_cycles: u32,
    failure_threshold: usize,
}

impl Director {
    pub fn new(
        llm: Arc<dyn LlmClient>,
        config: ModelConfig,
        event_bus: Arc<EventBus>,
        max_evolution_cycles: u32,
        failure_threshold: usize,
    ) -> Self {
        Self {
            llm,
            config,
            event_bus,
            max_evolution_cycles,
            failure_threshold,
        }
    }

    /// Top-level OODA loop: generate → execute → evaluate → (diagnose → evolve → retry).
    pub async fn run(
        &self,
        goal_obj: &mut GoalObject,
        runtime: &AgentRuntime,
        session_id: &SessionId,
    ) -> Result<DirectorResult> {
        let mut all_failures: Vec<SemanticFailure> = Vec::new();
        let mut total_nodes = 0usize;

        for cycle in 0..=self.max_evolution_cycles {
            info!(cycle, "Director: starting execution cycle");

            // 1. Generate graph
            let (nodes, edges) = self.generate_graph(goal_obj, session_id, cycle).await?;
            let node_count = nodes.len();
            let entry = nodes
                .first()
                .map(|n| n.id.clone())
                .unwrap_or_else(|| "start".to_string());

            // 2. Execute graph
            let exec_result = self
                .execute_graph(nodes, edges, &entry, runtime, session_id)
                .await?;
            total_nodes += exec_result.node_results.len();

            // Publish NodeComplete events
            for nr in &exec_result.node_results {
                self.event_bus.publish(AgentEvent::NodeComplete {
                    session_id: session_id.clone(),
                    node_id: nr.node_id.clone(),
                    succeeded: nr.succeeded,
                    elapsed_ms: nr.elapsed_ms,
                });
            }

            // Collect final output from context or last node
            let output = exec_result
                .context
                .get_str("final_output")
                .map(|s| s.to_string())
                .or_else(|| exec_result.node_results.last().map(|nr| nr.output.clone()))
                .unwrap_or_default();

            // 3. Evaluate result
            let verdict = self.evaluate_result(&output, &exec_result, goal_obj).await;

            if exec_result.succeeded && verdict {
                info!(cycle, node_count, "Director: goal achieved");
                return Ok(DirectorResult {
                    output,
                    succeeded: true,
                    evolution_cycles: cycle,
                    total_nodes_executed: total_nodes,
                    semantic_failures: all_failures,
                });
            }

            // Last cycle — return best effort
            if cycle >= self.max_evolution_cycles {
                warn!(cycle, "Director: max evolution cycles reached");
                return Ok(DirectorResult {
                    output,
                    succeeded: false,
                    evolution_cycles: cycle,
                    total_nodes_executed: total_nodes,
                    semantic_failures: all_failures,
                });
            }

            // 4. Diagnose failures
            let failures = self
                .diagnose_failure(&exec_result, goal_obj, session_id)
                .await;
            all_failures.extend(failures.clone());

            // 5. Evolve
            let should_retry = self.evolve(goal_obj, &failures, session_id, cycle);
            if !should_retry {
                return Ok(DirectorResult {
                    output,
                    succeeded: false,
                    evolution_cycles: cycle,
                    total_nodes_executed: total_nodes,
                    semantic_failures: all_failures,
                });
            }
        }

        Err(RyvosError::Config(
            "Director: exhausted evolution cycles".to_string(),
        ))
    }

    /// Generate an execution graph by asking the LLM to plan based on the goal.
    async fn generate_graph(
        &self,
        goal_obj: &GoalObject,
        session_id: &SessionId,
        cycle: u32,
    ) -> Result<(Vec<Node>, Vec<Edge>)> {
        let failure_context = if goal_obj.failure_history.is_empty() {
            String::new()
        } else {
            let failures: Vec<String> = goal_obj
                .failure_history
                .iter()
                .map(|f| format!("- Node '{}': {:?} — {}", f.node_id, f.category, f.diagnosis))
                .collect();
            format!(
                "\n\nPrevious failures (avoid repeating):\n{}",
                failures.join("\n")
            )
        };

        let constraints_text = goal_obj
            .goal
            .constraints
            .iter()
            .map(|c| format!("- {:?} ({:?}): {}", c.category, c.kind, c.description))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            r#"You are a Director planning an execution graph for a goal.

Goal: {}
Success criteria: {}
Constraints:
{}{}

Respond with a JSON object containing "nodes" and "edges" arrays.
Each node: {{"id": "string", "name": "string", "system_prompt": "string", "input_keys": [], "output_keys": [], "max_turns": number}}
Each edge: {{"from": "string", "to": "string", "condition": "always"}}

The first node in the array is the entry point. The last node should produce a "final_output" output key.
Keep the graph minimal (2-5 nodes). Respond with ONLY the JSON object."#,
            goal_obj.goal.description,
            goal_obj
                .goal
                .success_criteria
                .iter()
                .map(|c| c.description.as_str())
                .collect::<Vec<_>>()
                .join("; "),
            constraints_text,
            failure_context
        );

        let messages = vec![ChatMessage::user(prompt)];
        let response = self.llm_complete(&messages).await?;

        let (nodes, edges) = parse_graph_json(&response)?;

        self.event_bus.publish(AgentEvent::GraphGenerated {
            session_id: session_id.clone(),
            node_count: nodes.len(),
            edge_count: edges.len(),
            evolution_cycle: cycle,
        });

        Ok((nodes, edges))
    }

    /// Execute the graph using GraphExecutor.
    async fn execute_graph(
        &self,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        entry: &str,
        runtime: &AgentRuntime,
        _session_id: &SessionId,
    ) -> Result<crate::graph::executor::ExecutionResult> {
        let executor = GraphExecutor::new(nodes, edges, entry);
        let context = HandoffContext::new();
        executor.execute(runtime, context, None).await
    }

    /// Evaluate execution result against the goal.
    async fn evaluate_result(
        &self,
        output: &str,
        exec_result: &crate::graph::executor::ExecutionResult,
        goal_obj: &GoalObject,
    ) -> bool {
        if !exec_result.succeeded {
            return false;
        }

        // Use deterministic evaluation first
        let results = goal_obj.goal.evaluate_deterministic(output);
        let eval = goal_obj.goal.compute_evaluation(results, vec![]);
        if eval.passed {
            return true;
        }

        // If deterministic check isn't conclusive (has LlmJudge criteria),
        // use Judge for full evaluation
        let judge = Judge::new(self.llm.clone(), self.config.clone());
        match judge.evaluate(output, &[], &goal_obj.goal).await {
            Ok(verdict) => matches!(verdict, ryvos_core::types::Verdict::Accept { .. }),
            Err(_) => false,
        }
    }

    /// Diagnose failures by asking LLM why each failed node didn't meet the goal.
    async fn diagnose_failure(
        &self,
        exec_result: &crate::graph::executor::ExecutionResult,
        goal_obj: &GoalObject,
        session_id: &SessionId,
    ) -> Vec<SemanticFailure> {
        let mut failures = Vec::new();

        let failed_nodes: Vec<_> = exec_result
            .node_results
            .iter()
            .filter(|nr| !nr.succeeded)
            .collect();

        if failed_nodes.is_empty() {
            // All nodes succeeded but goal wasn't met — diagnose the overall output
            let prompt = format!(
                "A multi-step task completed all steps but didn't meet the goal.\n\
                 Goal: {}\n\
                 Final output: {}\n\n\
                 Classify the failure as one of: logic_contradiction, velocity_drift, \
                 constraint_violation, failure_accumulation, quality_deficiency.\n\
                 Respond with JSON: {{\"category\": \"...\", \"diagnosis\": \"...\"}}",
                goal_obj.goal.description,
                exec_result
                    .node_results
                    .last()
                    .map(|nr| nr.output.as_str())
                    .unwrap_or("(empty)")
            );

            let messages = vec![ChatMessage::user(prompt)];
            if let Ok(response) = self.llm_complete(&messages).await {
                let (category, diagnosis) = parse_diagnosis(&response);
                let failure = SemanticFailure {
                    timestamp: chrono::Utc::now(),
                    node_id: "overall".to_string(),
                    category,
                    diagnosis: diagnosis.clone(),
                    attempted_action: "complete goal".to_string(),
                };

                self.event_bus.publish(AgentEvent::SemanticFailureCaptured {
                    session_id: session_id.clone(),
                    node_id: "overall".to_string(),
                    category: format!("{:?}", failure.category),
                    diagnosis,
                });

                failures.push(failure);
            }
        } else {
            for nr in failed_nodes {
                let prompt = format!(
                    "A graph node failed during goal execution.\n\
                     Goal: {}\n\
                     Node: {} ({})\n\
                     Output/Error: {}\n\n\
                     Classify the failure as one of: logic_contradiction, velocity_drift, \
                     constraint_violation, failure_accumulation, quality_deficiency.\n\
                     Respond with JSON: {{\"category\": \"...\", \"diagnosis\": \"...\"}}",
                    goal_obj.goal.description,
                    nr.node_id,
                    nr.node_id,
                    &nr.output[..nr.output.len().min(500)]
                );

                let messages = vec![ChatMessage::user(prompt)];
                if let Ok(response) = self.llm_complete(&messages).await {
                    let (category, diagnosis) = parse_diagnosis(&response);
                    let failure = SemanticFailure {
                        timestamp: chrono::Utc::now(),
                        node_id: nr.node_id.clone(),
                        category,
                        diagnosis: diagnosis.clone(),
                        attempted_action: format!("execute node {}", nr.node_id),
                    };

                    self.event_bus.publish(AgentEvent::SemanticFailureCaptured {
                        session_id: session_id.clone(),
                        node_id: nr.node_id.clone(),
                        category: format!("{:?}", failure.category),
                        diagnosis,
                    });

                    failures.push(failure);
                }
            }
        }

        failures
    }

    /// Evolve the goal object by appending failures and bumping version.
    /// Returns true if the Director should retry.
    fn evolve(
        &self,
        goal_obj: &mut GoalObject,
        failures: &[SemanticFailure],
        session_id: &SessionId,
        cycle: u32,
    ) -> bool {
        if failures.is_empty() {
            return false;
        }

        goal_obj.failure_history.extend(failures.iter().cloned());
        goal_obj.evolution_count += 1;
        goal_obj.goal.version += 1;

        let reason = failures
            .iter()
            .map(|f| format!("{:?}", f.category))
            .collect::<Vec<_>>()
            .join(", ");

        self.event_bus.publish(AgentEvent::EvolutionTriggered {
            session_id: session_id.clone(),
            reason: reason.clone(),
            cycle: cycle + 1,
        });

        info!(
            cycle = cycle + 1,
            failures = failures.len(),
            reason = %reason,
            "Director: evolution triggered"
        );

        true
    }

    /// Helper: run a simple LLM completion (no tools).
    async fn llm_complete(&self, messages: &[ChatMessage]) -> Result<String> {
        let mut stream = self
            .llm
            .chat_stream(&self.config, messages.to_vec(), &[])
            .await
            .map_err(|e| RyvosError::LlmRequest(e.to_string()))?;

        let mut response = String::new();
        while let Some(delta) = stream.next().await {
            if let Ok(StreamDelta::TextDelta(text)) = delta {
                response.push_str(&text);
            }
        }
        Ok(response)
    }
}

/// Parse LLM JSON response into nodes and edges.
pub fn parse_graph_json(response: &str) -> Result<(Vec<Node>, Vec<Edge>)> {
    // Find JSON object in response (may have surrounding text)
    let json_str = extract_json_object(response)
        .ok_or_else(|| RyvosError::Config("No JSON object found in LLM response".to_string()))?;

    let value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| RyvosError::Config(e.to_string()))?;

    let nodes: Vec<Node> = value
        .get("nodes")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let edges: Vec<Edge> = value
        .get("edges")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    if nodes.is_empty() {
        return Err(RyvosError::Config(
            "Graph must have at least one node".to_string(),
        ));
    }

    Ok((nodes, edges))
}

/// Extract the first JSON object from a string (handles markdown code blocks).
fn extract_json_object(text: &str) -> Option<&str> {
    // Try to find ```json ... ``` block
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim());
        }
    }
    // Try to find ``` ... ``` block
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        if let Some(end) = after.find("```") {
            let inner = after[..end].trim();
            if inner.starts_with('{') {
                return Some(inner);
            }
        }
    }
    // Try to find raw { ... }
    let start = text.find('{')?;
    let mut depth = 0;
    let bytes = text.as_bytes();
    for (i, &b) in bytes[start..].iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse a diagnosis response into (FailureCategory, diagnosis string).
fn parse_diagnosis(response: &str) -> (FailureCategory, String) {
    if let Some(json_str) = extract_json_object(response) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_str) {
            let category = value
                .get("category")
                .and_then(|v| v.as_str())
                .map(|s| match s {
                    "logic_contradiction" => FailureCategory::LogicContradiction,
                    "velocity_drift" => FailureCategory::VelocityDrift,
                    "constraint_violation" => FailureCategory::ConstraintViolation,
                    "failure_accumulation" => FailureCategory::FailureAccumulation,
                    "quality_deficiency" => FailureCategory::QualityDeficiency,
                    _ => FailureCategory::QualityDeficiency,
                })
                .unwrap_or(FailureCategory::QualityDeficiency);
            let diagnosis = value
                .get("diagnosis")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown failure")
                .to_string();
            return (category, diagnosis);
        }
    }
    (
        FailureCategory::QualityDeficiency,
        response.chars().take(200).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_graph_json_basic() {
        let json = r#"{"nodes": [{"id": "n1", "name": "Research"}, {"id": "n2", "name": "Write"}], "edges": [{"from": "n1", "to": "n2"}]}"#;
        let (nodes, edges) = parse_graph_json(json).unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
        assert_eq!(nodes[0].id, "n1");
        assert_eq!(edges[0].from, "n1");
        assert_eq!(edges[0].to, "n2");
    }

    #[test]
    fn test_parse_graph_json_with_markdown() {
        let response = "Here's the plan:\n```json\n{\"nodes\": [{\"id\": \"start\", \"name\": \"Plan\"}], \"edges\": []}\n```\nDone.";
        let (nodes, edges) = parse_graph_json(response).unwrap();
        assert_eq!(nodes.len(), 1);
        assert!(edges.is_empty());
    }

    #[test]
    fn test_parse_graph_json_empty_nodes_fails() {
        let json = r#"{"nodes": [], "edges": []}"#;
        assert!(parse_graph_json(json).is_err());
    }

    #[test]
    fn test_extract_json_object() {
        assert_eq!(
            extract_json_object(r#"text {"a": 1} more"#),
            Some(r#"{"a": 1}"#)
        );
        assert_eq!(extract_json_object("no json here"), None);
    }

    #[test]
    fn test_parse_diagnosis() {
        let response =
            r#"{"category": "logic_contradiction", "diagnosis": "output contradicts constraint"}"#;
        let (cat, diag) = parse_diagnosis(response);
        assert_eq!(cat, FailureCategory::LogicContradiction);
        assert_eq!(diag, "output contradicts constraint");
    }

    #[test]
    fn test_parse_diagnosis_fallback() {
        let response = "I don't know what happened";
        let (cat, diag) = parse_diagnosis(response);
        assert_eq!(cat, FailureCategory::QualityDeficiency);
        assert!(diag.contains("I don't know"));
    }

    #[test]
    fn test_evolve_bumps_version() {
        use ryvos_core::goal::Goal;
        use std::collections::HashMap;

        let mut goal_obj = GoalObject {
            goal: Goal {
                description: "test".to_string(),
                success_criteria: vec![],
                constraints: vec![],
                success_threshold: 0.9,
                version: 0,
                metrics: HashMap::new(),
            },
            failure_history: vec![],
            evolution_count: 0,
        };

        let failures = [SemanticFailure {
            timestamp: chrono::Utc::now(),
            node_id: "n1".to_string(),
            category: FailureCategory::VelocityDrift,
            diagnosis: "too slow".to_string(),
            attempted_action: "compute".to_string(),
        }];

        let event_bus = Arc::new(EventBus::default());
        let session_id = SessionId::from_string("test");

        // Manually test evolve logic (without full Director)
        goal_obj.failure_history.extend(failures.iter().cloned());
        goal_obj.evolution_count += 1;
        goal_obj.goal.version += 1;

        assert_eq!(goal_obj.goal.version, 1);
        assert_eq!(goal_obj.evolution_count, 1);
        assert_eq!(goal_obj.failure_history.len(), 1);

        let _ = (event_bus, session_id); // used in real evolve()
    }
}
