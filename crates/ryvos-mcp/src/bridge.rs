//! Bridge between MCP tools and the Ryvos tool system.
//!
//! When Ryvos connects to an external MCP server (e.g., a filesystem server
//! or a GitHub server), the server exposes tools in the MCP format. This
//! module wraps each MCP tool as a Ryvos [`Tool`] trait implementation,
//! allowing it to be registered in the [`ToolRegistry`] and used by the agent.
//!
//! Tool names are prefixed as `mcp__{server_name}__{tool_name}` to avoid
//! collisions with built-in tools. For example, a tool called "read_file"
//! from a server called "filesystem" becomes "mcp__filesystem__read_file".
//!
//! Security tiers can be overridden per-server via the `tier_override`
//! config field. The default tier for MCP tools is T1 (workspace writes).

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

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{JsonObject, Tool as McpTool};
    use ryvos_core::security::SecurityTier;

    /// Helper: build a minimal MCP tool definition for tests.
    fn make_mcp_tool(name: &str, description: Option<&str>) -> McpTool {
        let mut schema = JsonObject::new();
        schema.insert("type".to_string(), serde_json::json!("object"));
        match description {
            Some(desc) => McpTool::new(name.to_string(), desc.to_string(), schema),
            None => {
                // Tool::new always sets description to Some, so we build
                // a tool then clear the description to test the fallback path.
                let mut tool = McpTool::new(name.to_string(), String::new(), schema);
                tool.description = None;
                tool
            }
        }
    }

    #[test]
    fn tool_name_follows_mcp_format() {
        let manager = Arc::new(McpClientManager::new());
        let mcp_tools = vec![make_mcp_tool("search", Some("Search tool"))];
        let mut registry = ToolRegistry::new();

        register_mcp_tools(&mut registry, &manager, "gmail", &mcp_tools, 60, None);

        let names = registry.list();
        assert!(names.contains(&"mcp__gmail__search"));
    }

    #[test]
    fn tool_name_multiple_tools_registered() {
        let manager = Arc::new(McpClientManager::new());
        let mcp_tools = vec![
            make_mcp_tool("read", Some("Read data")),
            make_mcp_tool("write", Some("Write data")),
            make_mcp_tool("delete", Some("Delete data")),
        ];
        let mut registry = ToolRegistry::new();

        register_mcp_tools(&mut registry, &manager, "notion", &mcp_tools, 30, None);

        let mut names = registry.list();
        names.sort();
        assert_eq!(
            names,
            vec![
                "mcp__notion__delete",
                "mcp__notion__read",
                "mcp__notion__write"
            ]
        );
    }

    #[test]
    fn schema_passthrough() {
        let manager = Arc::new(McpClientManager::new());
        let mcp_tools = vec![make_mcp_tool("fetch", Some("Fetch resource"))];
        let mut registry = ToolRegistry::new();

        register_mcp_tools(&mut registry, &manager, "api", &mcp_tools, 60, None);

        let tool = registry.get("mcp__api__fetch").unwrap();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
    }

    #[test]
    fn security_tier_default_is_t1() {
        let manager = Arc::new(McpClientManager::new());
        let mcp_tools = vec![make_mcp_tool("ping", None)];
        let mut registry = ToolRegistry::new();

        register_mcp_tools(&mut registry, &manager, "srv", &mcp_tools, 30, None);

        let tool = registry.get("mcp__srv__ping").unwrap();
        assert_eq!(tool.tier(), SecurityTier::T1);
    }

    #[test]
    fn security_tier_override_t3() {
        let manager = Arc::new(McpClientManager::new());
        let mcp_tools = vec![make_mcp_tool("destroy", Some("Dangerous"))];
        let mut registry = ToolRegistry::new();

        register_mcp_tools(
            &mut registry,
            &manager,
            "admin",
            &mcp_tools,
            120,
            Some("T3"),
        );

        let tool = registry.get("mcp__admin__destroy").unwrap();
        assert_eq!(tool.tier(), SecurityTier::T3);
    }

    #[test]
    fn security_tier_invalid_override_falls_back_to_t1() {
        let manager = Arc::new(McpClientManager::new());
        let mcp_tools = vec![make_mcp_tool("test", None)];
        let mut registry = ToolRegistry::new();

        register_mcp_tools(
            &mut registry,
            &manager,
            "x",
            &mcp_tools,
            30,
            Some("INVALID"),
        );

        let tool = registry.get("mcp__x__test").unwrap();
        assert_eq!(tool.tier(), SecurityTier::T1);
    }

    #[test]
    fn description_from_mcp_tool() {
        let manager = Arc::new(McpClientManager::new());
        let mcp_tools = vec![make_mcp_tool("query", Some("Run a database query"))];
        let mut registry = ToolRegistry::new();

        register_mcp_tools(&mut registry, &manager, "db", &mcp_tools, 30, None);

        let tool = registry.get("mcp__db__query").unwrap();
        assert_eq!(tool.description(), "Run a database query");
    }

    #[test]
    fn description_fallback_when_none() {
        let manager = Arc::new(McpClientManager::new());
        let mcp_tools = vec![make_mcp_tool("mystery", None)];
        let mut registry = ToolRegistry::new();

        register_mcp_tools(&mut registry, &manager, "srv", &mcp_tools, 30, None);

        let tool = registry.get("mcp__srv__mystery").unwrap();
        assert_eq!(tool.description(), "MCP tool: mystery");
    }

    #[test]
    fn timeout_passthrough() {
        let manager = Arc::new(McpClientManager::new());
        let mcp_tools = vec![make_mcp_tool("slow", Some("Slow op"))];
        let mut registry = ToolRegistry::new();

        register_mcp_tools(&mut registry, &manager, "srv", &mcp_tools, 300, None);

        let tool = registry.get("mcp__srv__slow").unwrap();
        assert_eq!(tool.timeout_secs(), 300);
    }
}
