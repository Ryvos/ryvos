use serde::{Deserialize, Serialize};

use ryvos_core::goal::Goal;

/// A node in the execution graph.
///
/// Each node represents an independent agent run with its own system prompt,
/// tools, and optional goal. Input/output keys define what data flows in
/// and out of this node via the shared HandoffContext.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Unique identifier for this node.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// System prompt for this node's agent run.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Keys to pull from the HandoffContext as input.
    #[serde(default)]
    pub input_keys: Vec<String>,
    /// Keys to push into the HandoffContext from this node's output.
    #[serde(default)]
    pub output_keys: Vec<String>,
    /// Tool names available to this node (empty = all tools).
    #[serde(default)]
    pub tools: Vec<String>,
    /// Maximum turns for this node's agent run.
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    /// Optional goal for this node.
    #[serde(default)]
    pub goal: Option<Goal>,
}

fn default_max_turns() -> usize {
    10
}

impl Node {
    /// Create a new node with minimal configuration.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            system_prompt: None,
            input_keys: vec![],
            output_keys: vec![],
            tools: vec![],
            max_turns: default_max_turns(),
            goal: None,
        }
    }

    /// Set the system prompt.
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Set the input keys.
    pub fn with_inputs(mut self, keys: Vec<String>) -> Self {
        self.input_keys = keys;
        self
    }

    /// Set the output keys.
    pub fn with_outputs(mut self, keys: Vec<String>) -> Self {
        self.output_keys = keys;
        self
    }

    /// Set the goal.
    pub fn with_goal(mut self, goal: Goal) -> Self {
        self.goal = Some(goal);
        self
    }

    /// Set max turns.
    pub fn with_max_turns(mut self, turns: usize) -> Self {
        self.max_turns = turns;
        self
    }

    /// Build the prompt for this node by injecting context data.
    pub fn build_prompt(
        &self,
        base_prompt: &str,
        context_data: &std::collections::HashMap<String, serde_json::Value>,
    ) -> String {
        let mut prompt = String::new();

        // Include context data for input keys
        if !self.input_keys.is_empty() {
            prompt.push_str("## Context Data\n\n");
            for key in &self.input_keys {
                if let Some(value) = context_data.get(key) {
                    let display = match value {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    prompt.push_str(&format!("**{}**: {}\n", key, display));
                }
            }
            prompt.push_str("\n---\n\n");
        }

        // Add the base prompt / user task
        prompt.push_str(base_prompt);
        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_node_builder() {
        let node = Node::new("n1", "Research")
            .with_prompt("You are a researcher.")
            .with_inputs(vec!["topic".into()])
            .with_outputs(vec!["findings".into()])
            .with_max_turns(5);

        assert_eq!(node.id, "n1");
        assert_eq!(node.name, "Research");
        assert_eq!(node.system_prompt.as_deref(), Some("You are a researcher."));
        assert_eq!(node.input_keys, vec!["topic"]);
        assert_eq!(node.output_keys, vec!["findings"]);
        assert_eq!(node.max_turns, 5);
    }

    #[test]
    fn test_build_prompt_with_context() {
        let node = Node::new("n1", "Writer").with_inputs(vec!["topic".into(), "style".into()]);

        let mut ctx = HashMap::new();
        ctx.insert("topic".into(), serde_json::json!("Rust async patterns"));
        ctx.insert("style".into(), serde_json::json!("tutorial"));

        let prompt = node.build_prompt("Write an article.", &ctx);
        assert!(prompt.contains("**topic**: Rust async patterns"));
        assert!(prompt.contains("**style**: tutorial"));
        assert!(prompt.contains("Write an article."));
    }

    #[test]
    fn test_build_prompt_no_context() {
        let node = Node::new("n1", "Simple");
        let ctx = HashMap::new();
        let prompt = node.build_prompt("Do something.", &ctx);
        assert_eq!(prompt, "Do something.");
    }
}
