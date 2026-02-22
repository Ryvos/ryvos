use std::sync::Arc;

use futures::future::BoxFuture;
use tracing::debug;

use rmcp::model::Tool as McpTool;

use ryvos_core::error::Result;
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};
use ryvos_tools::ToolRegistry;

use crate::McpClientManager;

/// A tool that bridges to an MCP server tool.
/// Name format: mcp__{server}__{tool}
pub struct McpBridgedTool {
    display_name: String,
    server_name: String,
    tool_name: String,
    description: String,
    schema: serde_json::Value,
    manager: Arc<McpClientManager>,
    timeout: u64,
    security_tier: SecurityTier,
}

impl Tool for McpBridgedTool {
    fn name(&self) -> &str {
        &self.display_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        let server = self.server_name.clone();
        let tool = self.tool_name.clone();
        let manager = self.manager.clone();

        Box::pin(async move {
            let arguments = input.as_object().cloned();

            debug!(server = %server, tool = %tool, "Calling MCP tool");

            match manager.call_tool(&server, &tool, arguments).await {
                Ok(content) => Ok(ToolResult::success(content)),
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        })
    }

    fn timeout_secs(&self) -> u64 {
        self.timeout
    }

    fn tier(&self) -> SecurityTier {
        self.security_tier
    }
}

/// Register all tools from an MCP server into the tool registry.
pub fn register_mcp_tools(
    registry: &mut ToolRegistry,
    manager: &Arc<McpClientManager>,
    server_name: &str,
    tools: &[McpTool],
    timeout_secs: u64,
    tier_override: Option<&str>,
) {
    let security_tier = tier_override
        .and_then(|t| match t.to_uppercase().as_str() {
            "T0" => Some(SecurityTier::T0),
            "T1" => Some(SecurityTier::T1),
            "T2" => Some(SecurityTier::T2),
            "T3" => Some(SecurityTier::T3),
            "T4" => Some(SecurityTier::T4),
            _ => None,
        })
        .unwrap_or(SecurityTier::T1);

    for tool in tools {
        let display_name = format!("mcp__{}__{}", server_name, tool.name);
        let description = tool
            .description
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_else(|| format!("MCP tool: {}", tool.name));

        // Convert the MCP tool's input_schema to a serde_json::Value
        let schema = serde_json::to_value(&*tool.input_schema)
            .unwrap_or(serde_json::json!({"type": "object"}));

        let bridged = McpBridgedTool {
            display_name: display_name.clone(),
            server_name: server_name.to_string(),
            tool_name: tool.name.to_string(),
            description,
            schema,
            manager: manager.clone(),
            timeout: timeout_secs,
            security_tier,
        };

        registry.register(bridged);
        debug!(name = %display_name, "Registered MCP bridged tool");
    }
}
