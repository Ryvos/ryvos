use futures::future::BoxFuture;
use serde::Deserialize;
use std::path::PathBuf;
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct EditTool;

#[derive(Deserialize)]
struct EditInput {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T1
    }

    fn description(&self) -> &str {
        "Perform exact string replacements in files. The old_string must be unique in the file unless replace_all is true."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to edit"
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact text to find and replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement text"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default: false)",
                    "default": false
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: EditInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            if params.old_string == params.new_string {
                return Ok(ToolResult::error("old_string and new_string are identical"));
            }

            let path = resolve_path(&params.file_path, &ctx.working_dir);
            debug!(path = %path.display(), "Editing file");

            let content =
                tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "edit".to_string(),
                        message: format!("{}: {}", path.display(), e),
                    })?;

            let count = content.matches(&params.old_string).count();

            if count == 0 {
                return Ok(ToolResult::error(format!(
                    "old_string not found in {}",
                    path.display()
                )));
            }

            if count > 1 && !params.replace_all {
                return Ok(ToolResult::error(format!(
                    "old_string found {} times in {}. Use replace_all: true to replace all, or provide a more specific string.",
                    count,
                    path.display()
                )));
            }

            let new_content = if params.replace_all {
                content.replace(&params.old_string, &params.new_string)
            } else {
                content.replacen(&params.old_string, &params.new_string, 1)
            };

            tokio::fs::write(&path, &new_content)
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "edit".to_string(),
                    message: format!("{}: {}", path.display(), e),
                })?;

            let msg = if params.replace_all {
                format!("Replaced {} occurrences in {}", count, path.display())
            } else {
                format!("Edited {}", path.display())
            };

            Ok(ToolResult::success(msg))
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
