use std::path::PathBuf;

use futures::future::BoxFuture;
use serde::Deserialize;
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct GlobTool;

#[derive(Deserialize)]
struct GlobInput {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T0
    }

    fn description(&self) -> &str {
        "Search for files matching a glob pattern (e.g. \"**/*.rs\"). \
         Returns matching file paths sorted by modification time (newest first)."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files (e.g. \"**/*.rs\", \"src/**/*.ts\")"
                },
                "path": {
                    "type": "string",
                    "description": "Base directory to search from (default: working directory)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: GlobInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            let base = match &params.path {
                Some(p) => {
                    let path = PathBuf::from(p);
                    if path.is_absolute() {
                        path
                    } else {
                        ctx.working_dir.join(path)
                    }
                }
                None => ctx.working_dir.clone(),
            };

            let full_pattern = base.join(&params.pattern);
            let pattern_str = full_pattern.to_string_lossy().to_string();

            debug!(pattern = %pattern_str, "Glob search");

            let paths: std::result::Result<Vec<PathBuf>, glob::PatternError> =
                glob::glob(&pattern_str).map(|entries| {
                    entries.filter_map(|e| e.ok()).collect()
                });

            let mut paths = paths.map_err(|e| RyvosError::ToolExecution {
                tool: "glob".to_string(),
                message: format!("Invalid pattern: {}", e),
            })?;

            // Sort by modification time (newest first)
            paths.sort_by(|a, b| {
                let mtime_a = a.metadata().and_then(|m| m.modified()).ok();
                let mtime_b = b.metadata().and_then(|m| m.modified()).ok();
                mtime_b.cmp(&mtime_a)
            });

            // Limit to 1000 results
            paths.truncate(1000);

            let output = if paths.is_empty() {
                "No files matched the pattern.".to_string()
            } else {
                let count = paths.len();
                let listing = paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{} files matched:\n{}", count, listing)
            };

            Ok(ToolResult::success(output))
        })
    }
}
