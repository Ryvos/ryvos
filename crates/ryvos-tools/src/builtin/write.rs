use futures::future::BoxFuture;
use serde::Deserialize;
use std::path::PathBuf;
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct WriteTool;

#[derive(Deserialize)]
struct WriteInput {
    file_path: String,
    content: String,
}

impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T1
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file and parent directories if they don't exist. Overwrites existing content."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: WriteInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            let path = resolve_path(&params.file_path, &ctx.working_dir);
            debug!(path = %path.display(), "Writing file");

            // Create parent directories
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "write".to_string(),
                        message: format!("Failed to create directories: {}", e),
                    })?;
            }

            tokio::fs::write(&path, &params.content)
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "write".to_string(),
                    message: format!("{}: {}", path.display(), e),
                })?;

            Ok(ToolResult::success(format!(
                "File written successfully: {}",
                path.display()
            )))
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

#[cfg(test)]
mod tests {
    use super::*;
    use ryvos_core::traits::Tool;
    use ryvos_test_utils::test_tool_context_with_dir;

    #[tokio::test]
    async fn write_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("output.txt");

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = WriteTool;
        let input = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "content": "hello from test"
        });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("File written successfully"));
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "hello from test"
        );
    }

    #[tokio::test]
    async fn write_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("a/b/c/deep.txt");

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = WriteTool;
        let input = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "content": "deep content"
        });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(file_path.exists());
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "deep content");
    }

    #[tokio::test]
    async fn write_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("overwrite.txt");
        std::fs::write(&file_path, "old content").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = WriteTool;
        let input = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "content": "new content"
        });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "new content");
    }
}
