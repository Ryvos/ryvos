use futures::future::BoxFuture;
use serde_json::json;

use ryvos_core::error::Result;
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct JiraSearchTool;

impl Tool for JiraSearchTool {
    fn name(&self) -> &str {
        "jira_search"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Search Jira issues with JQL."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "jql": { "type": "string", "description": "JQL query (e.g., 'project = PROJ AND status = Open')" },
                "limit": { "type": "integer", "default": 20 }
            },
            "required": ["jql"]
        })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            Ok(ToolResult::error(
                "Jira not configured. Add [jira] section to config.toml.",
            ))
        })
    }
}

pub struct JiraCreateIssueTool;

impl Tool for JiraCreateIssueTool {
    fn name(&self) -> &str {
        "jira_create_issue"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Create a new Jira issue."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "project": { "type": "string" },
                "summary": { "type": "string" },
                "description": { "type": "string" },
                "issue_type": { "type": "string", "default": "Task" }
            },
            "required": ["project", "summary"]
        })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move { Ok(ToolResult::error("Jira not configured.")) })
    }
}

pub struct JiraUpdateIssueTool;

impl Tool for JiraUpdateIssueTool {
    fn name(&self) -> &str {
        "jira_update_issue"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Update a Jira issue (status, assignee, fields)."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "issue_key": { "type": "string", "description": "e.g., PROJ-123" },
                "fields": { "type": "object", "description": "Fields to update" }
            },
            "required": ["issue_key"]
        })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move { Ok(ToolResult::error("Jira not configured.")) })
    }
}
