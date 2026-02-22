use std::collections::HashMap;
use std::sync::Arc;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolDefinition, ToolResult};

/// Registry of available tools.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool.
    pub fn register(&mut self, tool: impl Tool) {
        let name = tool.name().to_string();
        self.tools.insert(name, Arc::new(tool));
    }

    /// Unregister a tool by name.
    pub fn unregister(&mut self, name: &str) -> bool {
        self.tools.remove(name).is_some()
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// List all registered tools.
    pub fn list(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Get tool definitions for sending to the LLM.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Execute a tool by name.
    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult> {
        let tool = self
            .get(name)
            .ok_or_else(|| RyvosError::ToolNotFound(name.to_string()))?;

        let timeout = std::time::Duration::from_secs(tool.timeout_secs());

        match tokio::time::timeout(timeout, tool.execute(input, ctx)).await {
            Ok(result) => result,
            Err(_) => Err(RyvosError::ToolTimeout {
                tool: name.to_string(),
                timeout_secs: tool.timeout_secs(),
            }),
        }
    }

    /// Create a registry with all built-in tools registered.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(crate::builtin::bash::BashTool);
        registry.register(crate::builtin::read::ReadTool);
        registry.register(crate::builtin::write::WriteTool);
        registry.register(crate::builtin::edit::EditTool);
        registry.register(crate::builtin::memory_search::MemorySearchTool);
        registry.register(crate::builtin::memory_write::MemoryWriteTool);
        registry.register(crate::builtin::spawn_agent::SpawnAgentTool);
        registry.register(crate::builtin::glob::GlobTool);
        registry.register(crate::builtin::grep::GrepTool);
        registry.register(crate::builtin::web_fetch::WebFetchTool);
        registry.register(crate::builtin::apply_patch::ApplyPatchTool);
        registry
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
