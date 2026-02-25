use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use tracing::{debug, error, info, warn};

use ryvos_core::config::ModelConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::LlmClient;
use ryvos_core::types::{ChatMessage, SessionId, StreamDelta};

use super::edge::{evaluate_condition, Edge, EdgeCondition};
use super::handoff::HandoffContext;
use super::node::Node;
use crate::AgentRuntime;

/// Result of executing a single node.
#[derive(Debug, Clone)]
pub struct NodeResult {
    /// Which node was executed.
    pub node_id: String,
    /// The agent output text.
    pub output: String,
    /// Whether the node succeeded.
    pub succeeded: bool,
    /// Execution time in milliseconds.
    pub elapsed_ms: u64,
}

/// Result of executing an entire graph.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Per-node results in execution order.
    pub node_results: Vec<NodeResult>,
    /// The final HandoffContext with all accumulated data.
    pub context: HandoffContext,
    /// Total execution time in milliseconds.
    pub total_elapsed_ms: u64,
    /// Whether the graph completed successfully (all executed nodes succeeded).
    pub succeeded: bool,
}

/// Executes a DAG workflow.
///
/// The executor maintains a set of nodes and edges. Starting from `entry_node`,
/// it runs each node as an `AgentRuntime::run()` call, evaluates outgoing edge
/// conditions, and follows the first matching edge to the next node.
pub struct GraphExecutor {
    nodes: HashMap<String, Node>,
    edges: Vec<Edge>,
    entry_node: String,
}

impl GraphExecutor {
    /// Create a new graph executor.
    ///
    /// `entry_node` must be the id of a node in `nodes`.
    pub fn new(nodes: Vec<Node>, edges: Vec<Edge>, entry_node: impl Into<String>) -> Self {
        let node_map = nodes.into_iter().map(|n| (n.id.clone(), n)).collect();
        Self {
            nodes: node_map,
            edges,
            entry_node: entry_node.into(),
        }
    }

    /// Execute the graph with a given agent runtime and initial context.
    ///
    /// The runtime is used for each node's agent run. The initial context
    /// provides starting data (e.g., the user's request).
    pub async fn execute(
        &self,
        runtime: &AgentRuntime,
        initial_context: HandoffContext,
        llm: Option<&(Arc<dyn LlmClient>, ModelConfig)>,
    ) -> Result<ExecutionResult> {
        let start = Instant::now();
        let mut context = initial_context;
        let mut node_results = Vec::new();
        let mut current_node_id = self.entry_node.clone();
        let mut visited: Vec<String> = Vec::new();

        loop {
            // Prevent infinite loops
            if visited.iter().filter(|id| **id == current_node_id).count() > 5 {
                warn!(
                    node_id = %current_node_id,
                    "Node visited more than 5 times, terminating graph"
                );
                break;
            }
            visited.push(current_node_id.clone());

            let node = match self.nodes.get(&current_node_id) {
                Some(n) => n,
                None => {
                    return Err(RyvosError::Config(format!(
                        "Node '{}' not found in graph",
                        current_node_id
                    )));
                }
            };

            info!(node_id = %node.id, node_name = %node.name, "Executing graph node");

            // Build the prompt from context
            let base_prompt = node
                .system_prompt
                .as_deref()
                .unwrap_or("Complete the task.");
            let prompt = node.build_prompt(base_prompt, context.data());

            // Execute node
            let node_start = Instant::now();
            let session = SessionId::new();
            let result = if let Some(ref goal) = node.goal {
                runtime.run_with_goal(&session, &prompt, Some(goal)).await
            } else {
                runtime.run(&session, &prompt).await
            };

            let elapsed_ms = node_start.elapsed().as_millis() as u64;
            let (output, succeeded) = match result {
                Ok(text) => (text, true),
                Err(e) => {
                    error!(node_id = %node.id, error = %e, "Graph node failed");
                    (e.to_string(), false)
                }
            };

            // Ingest output into context
            context.ingest_output(&node.output_keys, &output);

            // Store a status key for conditional edges
            context.set_str(
                format!("{}_status", node.id),
                if succeeded { "success" } else { "failure" },
            );

            node_results.push(NodeResult {
                node_id: node.id.clone(),
                output: output.clone(),
                succeeded,
                elapsed_ms,
            });

            debug!(
                node_id = %node.id,
                succeeded,
                elapsed_ms,
                "Node execution complete"
            );

            // Find outgoing edges and evaluate conditions
            let outgoing: Vec<&Edge> = self
                .edges
                .iter()
                .filter(|e| e.from == current_node_id)
                .collect();

            if outgoing.is_empty() {
                debug!(node_id = %current_node_id, "No outgoing edges, graph complete");
                break;
            }

            let mut next_node: Option<String> = None;

            for edge in &outgoing {
                let matches = match &edge.condition {
                    EdgeCondition::Always => true,
                    EdgeCondition::OnSuccess => succeeded,
                    EdgeCondition::OnFailure => !succeeded,
                    EdgeCondition::Conditional { expr } => {
                        evaluate_condition(expr, context.data())
                    }
                    EdgeCondition::LlmDecide { prompt: decide_prompt } => {
                        // Use LLM to decide
                        if let Some((llm_client, config)) = llm {
                            evaluate_llm_edge(llm_client, config, decide_prompt, context.data())
                                .await
                        } else {
                            warn!("LlmDecide edge but no LLM configured, skipping");
                            false
                        }
                    }
                };

                if matches {
                    next_node = Some(edge.to.clone());
                    break; // Take the first matching edge
                }
            }

            match next_node {
                Some(next) => {
                    current_node_id = next;
                }
                None => {
                    debug!(
                        node_id = %current_node_id,
                        "No edge conditions matched, graph complete"
                    );
                    break;
                }
            }
        }

        let total_elapsed_ms = start.elapsed().as_millis() as u64;
        let all_succeeded = node_results.iter().all(|r| r.succeeded);

        Ok(ExecutionResult {
            node_results,
            context,
            total_elapsed_ms,
            succeeded: all_succeeded,
        })
    }
}

/// Ask an LLM whether to traverse an edge.
async fn evaluate_llm_edge(
    llm: &Arc<dyn LlmClient>,
    config: &ModelConfig,
    prompt: &str,
    context_data: &HashMap<String, serde_json::Value>,
) -> bool {
    let context_str = context_data
        .iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect::<Vec<_>>()
        .join("\n");

    let full_prompt = format!(
        "{}\n\nContext:\n{}\n\nRespond with ONLY \"yes\" or \"no\".",
        prompt, context_str
    );

    let messages = vec![ChatMessage::user(full_prompt)];

    match llm.chat_stream(config, messages, &[]).await {
        Ok(mut stream) => {
            let mut response = String::new();
            while let Some(delta) = stream.next().await {
                if let Ok(StreamDelta::TextDelta(text)) = delta {
                    response.push_str(&text);
                }
            }
            let answer = response.trim().to_lowercase();
            answer.contains("yes")
        }
        Err(e) => {
            warn!(error = %e, "LlmDecide edge evaluation failed, defaulting to no");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_construction() {
        let nodes = vec![
            Node::new("research", "Research Phase"),
            Node::new("write", "Writing Phase"),
            Node::new("review", "Review Phase"),
        ];

        let edges = vec![
            Edge::always("research", "write"),
            Edge::on_success("write", "review"),
        ];

        let executor = GraphExecutor::new(nodes, edges, "research");
        assert_eq!(executor.entry_node, "research");
        assert_eq!(executor.nodes.len(), 3);
        assert_eq!(executor.edges.len(), 2);
    }

    #[test]
    fn test_node_result() {
        let result = NodeResult {
            node_id: "n1".to_string(),
            output: "done".to_string(),
            succeeded: true,
            elapsed_ms: 42,
        };
        assert!(result.succeeded);
        assert_eq!(result.elapsed_ms, 42);
    }

    #[test]
    fn test_execution_result() {
        let result = ExecutionResult {
            node_results: vec![
                NodeResult {
                    node_id: "n1".into(),
                    output: "ok".into(),
                    succeeded: true,
                    elapsed_ms: 10,
                },
                NodeResult {
                    node_id: "n2".into(),
                    output: "ok".into(),
                    succeeded: true,
                    elapsed_ms: 20,
                },
            ],
            context: HandoffContext::new(),
            total_elapsed_ms: 30,
            succeeded: true,
        };
        assert!(result.succeeded);
        assert_eq!(result.node_results.len(), 2);
    }

    #[test]
    fn test_handoff_status_key() {
        let mut ctx = HandoffContext::new();
        ctx.set_str("research_status", "success");

        assert!(evaluate_condition(
            r#"research_status == "success""#,
            ctx.data()
        ));
    }
}
