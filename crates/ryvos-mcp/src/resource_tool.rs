use std::sync::Arc;

use futures::future::BoxFuture;

use ryvos_core::error::Result;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

use crate::McpClientManager;

/// Tool that reads MCP resources on behalf of the agent.
/// Input: { "server": "server_name", "uri": "resource://..." }
pub struct McpReadResourceTool {
    manager: Arc<McpClientManager>,
}

impl McpReadResourceTool {
    pub fn new(manager: Arc<McpClientManager>) -> Self {
        Self { manager }
    }
}

impl Tool for McpReadResourceTool {
    fn name(&self) -> &str {
        "mcp_read_resource"
    }

    fn description(&self) -> &str {
        "Read a resource from an MCP server. Provide the server name and resource URI."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "string",
                    "description": "Name of the MCP server"
                },
                "uri": {
                    "type": "string",
                    "description": "URI of the resource to read"
                }
            },
            "required": ["server", "uri"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let server = input
                .get("server")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let uri = input
                .get("uri")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            if server.is_empty() || uri.is_empty() {
                return Ok(ToolResult::error("Both 'server' and 'uri' are required"));
            }

            match self.manager.read_resource(server, uri).await {
                Ok(content) => Ok(ToolResult::success(content)),
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        })
    }

    fn timeout_secs(&self) -> u64 {
        120
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T0 // read-only
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryvos_core::security::SecurityTier;
    use ryvos_core::traits::Tool;

    fn make_resource_tool() -> McpReadResourceTool {
        let manager = Arc::new(McpClientManager::new());
        McpReadResourceTool::new(manager)
    }

    #[test]
    fn tool_name_is_mcp_read_resource() {
        let tool = make_resource_tool();
        assert_eq!(tool.name(), "mcp_read_resource");
    }

    #[test]
    fn schema_has_server_and_uri_required() {
        let tool = make_resource_tool();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");

        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("server"));
        assert!(props.contains_key("uri"));

        let required = schema["required"].as_array().unwrap();
        let req_strs: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(req_strs.contains(&"server"));
        assert!(req_strs.contains(&"uri"));
    }

    #[test]
    fn security_tier_is_t0_readonly() {
        let tool = make_resource_tool();
        assert_eq!(tool.tier(), SecurityTier::T0);
    }

    #[test]
    fn timeout_is_120_seconds() {
        let tool = make_resource_tool();
        assert_eq!(tool.timeout_secs(), 120);
    }

    #[test]
    fn description_mentions_resource() {
        let tool = make_resource_tool();
        assert!(tool.description().contains("resource"));
    }

    #[tokio::test]
    async fn execute_rejects_empty_server() {
        let tool = make_resource_tool();
        let ctx = ryvos_test_utils::test_tool_context();
        let input = serde_json::json!({"server": "", "uri": "resource://foo"});
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("required"));
    }

    #[tokio::test]
    async fn execute_rejects_empty_uri() {
        let tool = make_resource_tool();
        let ctx = ryvos_test_utils::test_tool_context();
        let input = serde_json::json!({"server": "myserver", "uri": ""});
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("required"));
    }

    #[tokio::test]
    async fn execute_rejects_missing_fields() {
        let tool = make_resource_tool();
        let ctx = ryvos_test_utils::test_tool_context();
        let input = serde_json::json!({});
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.is_error);
    }
}
