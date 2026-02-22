use futures::future::BoxFuture;
use serde_json::json;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct MemoryWriteTool;

impl Tool for MemoryWriteTool {
    fn name(&self) -> &str {
        "memory_write"
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T1
    }

    fn description(&self) -> &str {
        "Write a note to persistent memory (MEMORY.md in the workspace). \
         Use this to remember important facts, decisions, or context across sessions."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "note": {
                    "type": "string",
                    "description": "The note to save to memory"
                }
            },
            "required": ["note"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let note = input["note"]
                .as_str()
                .ok_or_else(|| RyvosError::ToolValidation("'note' must be a string".into()))?;

            let memory_path = ctx.working_dir.join("MEMORY.md");

            // Read existing content
            let existing = tokio::fs::read_to_string(&memory_path)
                .await
                .unwrap_or_default();

            // Append with timestamp header
            let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC");
            let entry = format!("\n## {}\n{}\n", timestamp, note);

            let new_content = if existing.is_empty() {
                format!("# Agent Memory\n{}", entry)
            } else {
                format!("{}{}", existing, entry)
            };

            tokio::fs::write(&memory_path, new_content)
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "memory_write".into(),
                    message: e.to_string(),
                })?;

            Ok(ToolResult::success(format!(
                "Note saved to {}",
                memory_path.display()
            )))
        })
    }
}
