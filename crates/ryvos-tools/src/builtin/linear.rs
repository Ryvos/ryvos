use futures::future::BoxFuture;
use serde_json::json;

use ryvos_core::error::Result;
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct LinearSearchTool;

impl Tool for LinearSearchTool {
    fn name(&self) -> &str {
        "linear_search"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Search Linear issues."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "limit": { "type": "integer", "default": 20 }
            },
            "required": ["query"]
        })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            Ok(ToolResult::error(
                "Linear not configured. Add [linear] section to config.toml.",
            ))
        })
    }
}

pub struct LinearCreateIssueTool;

impl Tool for LinearCreateIssueTool {
    fn name(&self) -> &str {
        "linear_create_issue"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Create a new Linear issue."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "description": { "type": "string" },
                "team_id": { "type": "string" },
                "priority": { "type": "integer", "description": "0-4 (0=none, 1=urgent, 4=low)" }
            },
            "required": ["title", "team_id"]
        })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move { Ok(ToolResult::error("Linear not configured.")) })
    }
}

pub struct LinearListProjectsTool;

impl Tool for LinearListProjectsTool {
    fn name(&self) -> &str {
        "linear_list_projects"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "List Linear projects and teams."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move { Ok(ToolResult::error("Linear not configured.")) })
    }
}
