use futures::future::BoxFuture;
use serde_json::json;

use ryvos_core::error::Result;
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct NotionSearchTool;

impl Tool for NotionSearchTool {
    fn name(&self) -> &str { "notion_search" }
    fn tier(&self) -> SecurityTier { SecurityTier::T0 }
    fn description(&self) -> &str { "Search Notion pages and databases." }
    fn input_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] })
    }
    fn execute(&self, _input: serde_json::Value, _ctx: ToolContext) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move { Ok(ToolResult::error("Notion not configured. Add [notion] section to config.toml with api_key.")) })
    }
}

pub struct NotionReadPageTool;

impl Tool for NotionReadPageTool {
    fn name(&self) -> &str { "notion_read_page" }
    fn tier(&self) -> SecurityTier { SecurityTier::T0 }
    fn description(&self) -> &str { "Read a Notion page by ID." }
    fn input_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": { "page_id": { "type": "string" } }, "required": ["page_id"] })
    }
    fn execute(&self, _input: serde_json::Value, _ctx: ToolContext) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move { Ok(ToolResult::error("Notion not configured.")) })
    }
}

pub struct NotionCreatePageTool;

impl Tool for NotionCreatePageTool {
    fn name(&self) -> &str { "notion_create_page" }
    fn tier(&self) -> SecurityTier { SecurityTier::T2 }
    fn description(&self) -> &str { "Create a new Notion page." }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "parent_id": { "type": "string", "description": "Parent page or database ID" },
                "title": { "type": "string" },
                "content": { "type": "string" }
            },
            "required": ["parent_id", "title"]
        })
    }
    fn execute(&self, _input: serde_json::Value, _ctx: ToolContext) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move { Ok(ToolResult::error("Notion not configured.")) })
    }
}

pub struct NotionQueryDatabaseTool;

impl Tool for NotionQueryDatabaseTool {
    fn name(&self) -> &str { "notion_query_database" }
    fn tier(&self) -> SecurityTier { SecurityTier::T0 }
    fn description(&self) -> &str { "Query a Notion database with filters." }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "database_id": { "type": "string" },
                "filter": { "type": "object", "description": "Notion filter object" }
            },
            "required": ["database_id"]
        })
    }
    fn execute(&self, _input: serde_json::Value, _ctx: ToolContext) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move { Ok(ToolResult::error("Notion not configured.")) })
    }
}
