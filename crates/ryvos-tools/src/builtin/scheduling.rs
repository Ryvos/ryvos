use futures::future::BoxFuture;
use serde::Deserialize;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

// ── CronListTool ────────────────────────────────────────────────

pub struct CronListTool;

impl Tool for CronListTool {
    fn name(&self) -> &str {
        "cron_list"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "List configured cron jobs from ryvos.toml."
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
            let config_path = ctx.config_path.ok_or_else(|| RyvosError::ToolExecution {
                tool: "cron_list".into(),
                message: "Config path not available".into(),
            })?;
            let content = tokio::fs::read_to_string(&config_path).await.map_err(|e| {
                RyvosError::ToolExecution {
                    tool: "cron_list".into(),
                    message: e.to_string(),
                }
            })?;
            let config: toml::Value =
                toml::from_str(&content).map_err(|e| RyvosError::ToolExecution {
                    tool: "cron_list".into(),
                    message: e.to_string(),
                })?;
            let jobs = config
                .get("cron")
                .and_then(|c| c.get("jobs"))
                .and_then(|j| j.as_array());
            match jobs {
                Some(jobs) => {
                    let mut output = format!("{} cron jobs configured:\n", jobs.len());
                    for job in jobs {
                        let name = job
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unnamed");
                        let schedule = job.get("schedule").and_then(|s| s.as_str()).unwrap_or("?");
                        let prompt = job.get("prompt").and_then(|p| p.as_str()).unwrap_or("?");
                        output.push_str(&format!("  - {} [{}]: {}\n", name, schedule, prompt));
                    }
                    Ok(ToolResult::success(output))
                }
                None => Ok(ToolResult::success("No cron jobs configured.".to_string())),
            }
        })
    }
}

// ── CronAddTool ─────────────────────────────────────────────────

pub struct CronAddTool;

#[derive(Deserialize)]
struct CronAddInput {
    name: String,
    schedule: String,
    prompt: String,
    #[serde(default)]
    channel: Option<String>,
}

impl Tool for CronAddTool {
    fn name(&self) -> &str {
        "cron_add"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Add a cron job to ryvos.toml."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Job name" },
                "schedule": { "type": "string", "description": "Cron expression (e.g. '0 9 * * *')" },
                "prompt": { "type": "string", "description": "Prompt to execute" },
                "channel": { "type": "string", "description": "Target channel" }
            },
            "required": ["name", "schedule", "prompt"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: CronAddInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let config_path = ctx.config_path.ok_or_else(|| RyvosError::ToolExecution {
                tool: "cron_add".into(),
                message: "Config path not available".into(),
            })?;
            let content = tokio::fs::read_to_string(&config_path).await.map_err(|e| {
                RyvosError::ToolExecution {
                    tool: "cron_add".into(),
                    message: e.to_string(),
                }
            })?;

            let mut job_toml = format!(
                "\n[[cron.jobs]]\nname = \"{}\"\nschedule = \"{}\"\nprompt = \"{}\"",
                p.name, p.schedule, p.prompt
            );
            if let Some(ch) = p.channel {
                job_toml.push_str(&format!("\nchannel = \"{}\"", ch));
            }
            job_toml.push('\n');

            let new_content = format!("{}{}", content, job_toml);
            tokio::fs::write(&config_path, &new_content)
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "cron_add".into(),
                    message: e.to_string(),
                })?;
            Ok(ToolResult::success(format!("Added cron job '{}'", p.name)))
        })
    }
}

// ── CronRemoveTool ──────────────────────────────────────────────

pub struct CronRemoveTool;

#[derive(Deserialize)]
struct CronRemoveInput {
    name: String,
}

impl Tool for CronRemoveTool {
    fn name(&self) -> &str {
        "cron_remove"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Remove a cron job from ryvos.toml by name."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "name": { "type": "string", "description": "Job name to remove" } },
            "required": ["name"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: CronRemoveInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let config_path = ctx.config_path.ok_or_else(|| RyvosError::ToolExecution {
                tool: "cron_remove".into(),
                message: "Config path not available".into(),
            })?;
            let content = tokio::fs::read_to_string(&config_path).await.map_err(|e| {
                RyvosError::ToolExecution {
                    tool: "cron_remove".into(),
                    message: e.to_string(),
                }
            })?;

            let mut config: toml::Value =
                toml::from_str(&content).map_err(|e| RyvosError::ToolExecution {
                    tool: "cron_remove".into(),
                    message: e.to_string(),
                })?;

            let removed = if let Some(cron) = config.get_mut("cron").and_then(|c| c.as_table_mut())
            {
                if let Some(jobs) = cron.get_mut("jobs").and_then(|j| j.as_array_mut()) {
                    let before = jobs.len();
                    jobs.retain(|j| j.get("name").and_then(|n| n.as_str()) != Some(&p.name));
                    before - jobs.len()
                } else {
                    0
                }
            } else {
                0
            };

            if removed > 0 {
                let new_content =
                    toml::to_string_pretty(&config).map_err(|e| RyvosError::ToolExecution {
                        tool: "cron_remove".into(),
                        message: e.to_string(),
                    })?;
                tokio::fs::write(&config_path, &new_content)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "cron_remove".into(),
                        message: e.to_string(),
                    })?;
                Ok(ToolResult::success(format!(
                    "Removed cron job '{}'",
                    p.name
                )))
            } else {
                Ok(ToolResult::error(format!(
                    "Cron job '{}' not found",
                    p.name
                )))
            }
        })
    }
}
