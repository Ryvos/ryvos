use futures::future::BoxFuture;
use serde::Deserialize;
use std::path::PathBuf;
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct ReadTool;

#[derive(Deserialize)]
struct ReadInput {
    file_path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T0
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Supports line offset and limit for large files."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read (default: 2000)"
                }
            },
            "required": ["file_path"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: ReadInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            let path = resolve_path(&params.file_path, &ctx.working_dir);
            debug!(path = %path.display(), "Reading file");

            let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
                RyvosError::ToolExecution {
                    tool: "read".to_string(),
                    message: format!("{}: {}", path.display(), e),
                }
            })?;

            let lines: Vec<&str> = content.lines().collect();
            let total_lines = lines.len();

            let offset = params.offset.unwrap_or(1).max(1) - 1; // Convert to 0-indexed
            let limit = params.limit.unwrap_or(2000);

            let end = (offset + limit).min(total_lines);
            let selected_lines = &lines[offset.min(total_lines)..end];

            let mut output = String::new();
            for (i, line) in selected_lines.iter().enumerate() {
                let line_num = offset + i + 1;
                // Truncate long lines
                let display_line = if line.len() > 2000 {
                    &line[..2000]
                } else {
                    line
                };
                output.push_str(&format!("{:>6}\t{}\n", line_num, display_line));
            }

            if output.is_empty() {
                output = "(empty file)".to_string();
            }

            Ok(ToolResult::success(output))
        })
    }
}

fn resolve_path(file_path: &str, working_dir: &std::path::Path) -> PathBuf {
    let path = PathBuf::from(file_path);
    if path.is_absolute() {
        path
    } else {
        working_dir.join(path)
    }
}
