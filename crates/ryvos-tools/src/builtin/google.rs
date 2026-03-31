use futures::future::BoxFuture;
use serde_json::json;

use ryvos_core::error::Result;
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

// ── Gmail Tools ───────────────────────────────────────────────

pub struct GmailInboxTool;

impl Tool for GmailInboxTool {
    fn name(&self) -> &str {
        "gmail_inbox"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T1
    }
    fn description(&self) -> &str {
        "List or search Gmail inbox. Supports query filters (from, to, subject, label)."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Gmail search query (e.g., 'from:alice subject:meeting')" },
                "limit": { "type": "integer", "description": "Max results (default 10)", "default": 10 }
            }
        })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            Ok(ToolResult::error(
                "Gmail not configured. Run `ryvos init` and select Google Workspace, \
                 or add [google] section to config.toml with client_secret_path.",
            ))
        })
    }
}

pub struct GmailReadTool;

impl Tool for GmailReadTool {
    fn name(&self) -> &str {
        "gmail_read"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Read a specific email by ID."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": { "message_id": { "type": "string" } }, "required": ["message_id"] })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move { Ok(ToolResult::error("Gmail not configured. Run `ryvos init`.")) })
    }
}

pub struct GmailSendTool;

impl Tool for GmailSendTool {
    fn name(&self) -> &str {
        "gmail_send"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Send an email via Gmail."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "to": { "type": "string" },
                "subject": { "type": "string" },
                "body": { "type": "string" }
            },
            "required": ["to", "subject", "body"]
        })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move { Ok(ToolResult::error("Gmail not configured. Run `ryvos init`.")) })
    }
}

// ── Calendar Tools ────────────────────────────────────────────

pub struct CalendarListTool;

impl Tool for CalendarListTool {
    fn name(&self) -> &str {
        "calendar_list"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "List upcoming Google Calendar events."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": { "days": { "type": "integer", "default": 7 } } })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            Ok(ToolResult::error(
                "Google Calendar not configured. Run `ryvos init`.",
            ))
        })
    }
}

pub struct CalendarCreateTool;

impl Tool for CalendarCreateTool {
    fn name(&self) -> &str {
        "calendar_create"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Create a new Google Calendar event."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "start": { "type": "string", "description": "ISO 8601 datetime" },
                "end": { "type": "string", "description": "ISO 8601 datetime" },
                "description": { "type": "string" }
            },
            "required": ["title", "start", "end"]
        })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            Ok(ToolResult::error(
                "Google Calendar not configured. Run `ryvos init`.",
            ))
        })
    }
}

// ── Drive Tools ───────────────────────────────────────────────

pub struct DriveSearchTool;

impl Tool for DriveSearchTool {
    fn name(&self) -> &str {
        "drive_search"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Search Google Drive files."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            Ok(ToolResult::error(
                "Google Drive not configured. Run `ryvos init`.",
            ))
        })
    }
}
