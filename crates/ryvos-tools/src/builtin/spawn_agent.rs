use futures::future::BoxFuture;
use serde_json::json;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct SpawnAgentTool;

impl Tool for SpawnAgentTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T3
    }

    fn description(&self) -> &str {
        "Spawn a background sub-agent to handle a task in parallel. \
         Returns the sub-agent's response when complete."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Task for the sub-agent"
                }
            },
            "required": ["prompt"]
        })
    }

    fn timeout_secs(&self) -> u64 {
        300 // 5 min for sub-agent
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let spawner = ctx
                .agent_spawner
                .as_ref()
                .ok_or_else(|| RyvosError::ToolExecution {
                    tool: "spawn_agent".into(),
                    message: "Agent spawning not available".into(),
                })?;

            let prompt = input["prompt"]
                .as_str()
                .ok_or_else(|| RyvosError::ToolValidation("'prompt' must be a string".into()))?
                .to_string();

            match spawner.spawn(prompt).await {
                Ok(result) => Ok(ToolResult::success(result)),
                Err(e) => Ok(ToolResult::error(format!("Sub-agent failed: {}", e))),
            }
        })
    }
}
