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
