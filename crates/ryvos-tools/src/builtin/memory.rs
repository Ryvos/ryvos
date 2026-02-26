use std::path::PathBuf;

use futures::future::BoxFuture;
use serde::Deserialize;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

fn workspace_dir() -> PathBuf {
    dirs_home()
        .map(|h| h.join(".ryvos"))
        .unwrap_or_else(|| PathBuf::from("/tmp/.ryvos"))
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

// ── MemoryGetTool ───────────────────────────────────────────────

pub struct MemoryGetTool;

#[derive(Deserialize)]
struct MemoryGetInput {
    #[serde(default)]
    name: Option<String>,
}

impl Tool for MemoryGetTool {
    fn name(&self) -> &str {
        "memory_get"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Read a memory file. Without a name, reads MEMORY.md. With a name, reads memory/<name>.md."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Memory file name (without .md). Omit for MEMORY.md." }
            }
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: MemoryGetInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let ws = workspace_dir();
            let path = match &params.name {
                Some(name) => ws.join("memory").join(format!("{}.md", name)),
                None => ws.join("MEMORY.md"),
            };
            match tokio::fs::read_to_string(&path).await {
                Ok(content) => Ok(ToolResult::success(content)),
                Err(e) => Ok(ToolResult::error(format!(
                    "Cannot read {}: {}",
                    path.display(),
                    e
                ))),
            }
        })
    }
}

// ── DailyLogWriteTool ───────────────────────────────────────────

pub struct DailyLogWriteTool;

#[derive(Deserialize)]
struct DailyLogInput {
    entry: String,
}

impl Tool for DailyLogWriteTool {
    fn name(&self) -> &str {
        "daily_log_write"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T1
    }
    fn description(&self) -> &str {
        "Append a timestamped entry to today's daily log (memory/YYYY-MM-DD.md)."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "entry": { "type": "string", "description": "Log entry text" }
            },
            "required": ["entry"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: DailyLogInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let ws = workspace_dir();
            let memory_dir = ws.join("memory");
            tokio::fs::create_dir_all(&memory_dir).await.map_err(|e| {
                RyvosError::ToolExecution {
                    tool: "daily_log_write".into(),
                    message: e.to_string(),
                }
            })?;
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let path = memory_dir.join(format!("{}.md", today));
            let timestamp = chrono::Utc::now().format("%H:%M:%S UTC").to_string();
            let line = format!("\n- **{}** — {}\n", timestamp, params.entry);
            tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "daily_log_write".into(),
                    message: e.to_string(),
                })?;
            tokio::fs::write(&path, {
                let existing = tokio::fs::read_to_string(&path).await.unwrap_or_default();
                format!("{}{}", existing, line)
            })
            .await
            .map_err(|e| RyvosError::ToolExecution {
                tool: "daily_log_write".into(),
                message: e.to_string(),
            })?;
            Ok(ToolResult::success(format!("Logged to {}", path.display())))
        })
    }
}

// ── MemoryDeleteTool ────────────────────────────────────────────

pub struct MemoryDeleteTool;

#[derive(Deserialize)]
struct MemoryDeleteInput {
    heading: String,
}

impl Tool for MemoryDeleteTool {
    fn name(&self) -> &str {
        "memory_delete"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Remove a section from MEMORY.md by its heading text."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "heading": { "type": "string", "description": "Heading text to find and remove (with its content until the next heading)" }
            },
            "required": ["heading"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: MemoryDeleteInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let ws = workspace_dir();
            let path = ws.join("MEMORY.md");
            let content =
                tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "memory_delete".into(),
                        message: e.to_string(),
                    })?;

            let heading_pattern = format!("# {}", params.heading);
            let mut lines: Vec<&str> = content.lines().collect();
            let mut start = None;
            let mut end = None;

            for (i, line) in lines.iter().enumerate() {
                if line.contains(&heading_pattern)
                    || line
                        .trim_start_matches('#')
                        .trim()
                        .eq_ignore_ascii_case(&params.heading)
                {
                    start = Some(i);
                    // Find the end: next heading of same or higher level, or EOF
                    let level = line.chars().take_while(|c| *c == '#').count();
                    for (j, line_j) in lines.iter().enumerate().skip(i + 1) {
                        let next_level = line_j.chars().take_while(|c| *c == '#').count();
                        if next_level > 0 && next_level <= level {
                            end = Some(j);
                            break;
                        }
                    }
                    if end.is_none() {
                        end = Some(lines.len());
                    }
                    break;
                }
            }

            match (start, end) {
                (Some(s), Some(e)) => {
                    lines.drain(s..e);
                    let new_content = lines.join("\n");
                    tokio::fs::write(&path, &new_content).await.map_err(|e| {
                        RyvosError::ToolExecution {
                            tool: "memory_delete".into(),
                            message: e.to_string(),
                        }
                    })?;
                    Ok(ToolResult::success(format!(
                        "Removed section '{}' ({} lines)",
                        params.heading,
                        e - s
                    )))
                }
                _ => Ok(ToolResult::error(format!(
                    "Section '{}' not found in MEMORY.md",
                    params.heading
                ))),
            }
        })
    }
}
