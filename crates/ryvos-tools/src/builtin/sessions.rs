use futures::future::BoxFuture;
use serde::Deserialize;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

// ── SessionListTool ─────────────────────────────────────────────

pub struct SessionListTool;

impl Tool for SessionListTool {
    fn name(&self) -> &str {
        "session_list"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "List active sessions."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            Ok(ToolResult::success(format!(
                "Current session: {}",
                ctx.session_id
            )))
        })
    }
}

// ── SessionHistoryTool ──────────────────────────────────────────

pub struct SessionHistoryTool;

#[derive(Deserialize)]
struct HistoryInput {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize {
    20
}

impl Tool for SessionHistoryTool {
    fn name(&self) -> &str {
        "session_history"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Load conversation history for a session."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Session ID (default: current)" },
                "limit": { "type": "integer", "description": "Max messages to load (default: 20)" }
            }
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: HistoryInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let store = ctx.store.ok_or_else(|| RyvosError::ToolExecution {
                tool: "session_history".into(),
                message: "No session store available".into(),
            })?;
            let sid = params
                .session_id
                .map(|s| ryvos_core::types::SessionId::from_string(&s))
                .unwrap_or(ctx.session_id);
            let messages = store.load_history(&sid, params.limit).await?;
            let mut output = format!("Session {} — {} messages:\n", sid, messages.len());
            for msg in &messages {
                let text = msg.text();
                let preview = if text.len() > 200 {
                    &text[..200]
                } else {
                    &text
                };
                output.push_str(&format!("[{:?}] {}\n", msg.role, preview));
            }
            Ok(ToolResult::success(output))
        })
    }
}

// ── SessionSendTool ─────────────────────────────────────────────

pub struct SessionSendTool;

#[derive(Deserialize)]
struct SendInput {
    session_id: String,
    message: String,
}

impl Tool for SessionSendTool {
    fn name(&self) -> &str {
        "session_send"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Inject a message into a target session."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string", "description": "Target session ID" },
                "message": { "type": "string", "description": "Message to inject" }
            },
            "required": ["session_id", "message"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: SendInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let store = ctx.store.ok_or_else(|| RyvosError::ToolExecution {
                tool: "session_send".into(),
                message: "No session store available".into(),
            })?;
            let sid = ryvos_core::types::SessionId::from_string(&params.session_id);
            let msg = ryvos_core::types::ChatMessage::user(&params.message);
            store
                .append_messages(&sid, std::slice::from_ref(&msg))
                .await?;
            Ok(ToolResult::success(format!(
                "Message sent to session {}",
                params.session_id
            )))
        })
    }
}

// ── SessionSpawnTool ────────────────────────────────────────────

pub struct SessionSpawnTool;

#[derive(Deserialize)]
struct SpawnInput {
    prompt: String,
}

impl Tool for SessionSpawnTool {
    fn name(&self) -> &str {
        "session_spawn"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Spawn a new sub-agent session with a prompt."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": { "type": "string", "description": "Task prompt for the sub-agent" }
            },
            "required": ["prompt"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: SpawnInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let spawner = ctx.agent_spawner.ok_or_else(|| RyvosError::ToolExecution {
                tool: "session_spawn".into(),
                message: "Agent spawner not available".into(),
            })?;
            let result = spawner.spawn(params.prompt).await?;
            Ok(ToolResult::success(result))
        })
    }
}

// ── SessionStatusTool ───────────────────────────────────────────

pub struct SessionStatusTool;

impl Tool for SessionStatusTool {
    fn name(&self) -> &str {
        "session_status"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Get status of the current session."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            Ok(ToolResult::success(format!(
                "Session ID: {}\nWorking directory: {}",
                ctx.session_id,
                ctx.working_dir.display()
            )))
        })
    }
}
