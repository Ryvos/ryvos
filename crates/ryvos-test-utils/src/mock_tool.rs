use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;

use ryvos_core::error::Result;
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

/// A mock tool for testing. Returns a fixed result and records every invocation.
pub struct MockTool {
    tool_name: String,
    tool_description: String,
    schema: serde_json::Value,
    result: ToolResult,
    tier: SecurityTier,
    invocations: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl MockTool {
    /// Create a mock tool that always returns a successful result.
    pub fn new(name: &str) -> Self {
        Self {
            tool_name: name.to_string(),
            tool_description: format!("Mock tool: {}", name),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            result: ToolResult::success("mock output"),
            tier: SecurityTier::T0,
            invocations: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Set the result this tool returns.
    pub fn with_result(mut self, result: ToolResult) -> Self {
        self.result = result;
        self
    }

    /// Set the description.
    pub fn with_description(mut self, desc: &str) -> Self {
        self.tool_description = desc.to_string();
        self
    }

    /// Set the JSON schema.
    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.schema = schema;
        self
    }

    /// Set the security tier.
    pub fn with_tier(mut self, tier: SecurityTier) -> Self {
        self.tier = tier;
        self
    }

    /// How many times this tool was invoked.
    pub fn invocation_count(&self) -> usize {
        self.invocations.lock().unwrap().len()
    }

    /// Get the input from invocation N (0-indexed).
    pub fn invocation_input(&self, n: usize) -> serde_json::Value {
        self.invocations.lock().unwrap()[n].clone()
    }
}

impl Tool for MockTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        self.invocations.lock().unwrap().push(input);
        let result = self.result.clone();
        Box::pin(async move { Ok(result) })
    }

    fn tier(&self) -> SecurityTier {
        self.tier
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::test_tool_context;

    #[tokio::test]
    async fn test_mock_tool_success() {
        let tool = MockTool::new("test_tool");
        let ctx = test_tool_context();
        let result = tool
            .execute(serde_json::json!({"key": "value"}), ctx)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content, "mock output");
        assert_eq!(tool.invocation_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_tool_error() {
        let tool = MockTool::new("fail_tool").with_result(ToolResult::error("something broke"));
        let ctx = test_tool_context();
        let result = tool.execute(serde_json::json!({}), ctx).await.unwrap();
        assert!(result.is_error);
        assert_eq!(result.content, "something broke");
    }
}
