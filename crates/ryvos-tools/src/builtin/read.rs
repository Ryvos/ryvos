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

            let content =
                tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "read".to_string(),
                        message: format!("{}: {}", path.display(), e),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ryvos_core::traits::Tool;
    use ryvos_test_utils::test_tool_context_with_dir;

    #[tokio::test]
    async fn read_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "line one\nline two\nline three\n").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = ReadTool;
        let input = serde_json::json!({ "file_path": file_path.to_str().unwrap() });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("line one"));
        assert!(result.content.contains("line two"));
        assert!(result.content.contains("line three"));
    }

    #[tokio::test]
    async fn read_with_offset_and_limit() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("big.txt");
        let content: String = (1..=10).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(&file_path, &content).unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = ReadTool;
        let input = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "offset": 3,
            "limit": 2
        });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("line 3"));
        assert!(result.content.contains("line 4"));
        assert!(!result.content.contains("line 5"));
        assert!(!result.content.contains("line 2"));
    }

    #[tokio::test]
    async fn read_nonexistent_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = ReadTool;
        let input =
            serde_json::json!({ "file_path": "/tmp/this_does_not_exist_ryvos_test_xyz.txt" });
        let result = tool.execute(input, ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("empty.txt");
        std::fs::write(&file_path, "").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = ReadTool;
        let input = serde_json::json!({ "file_path": file_path.to_str().unwrap() });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("(empty file)"));
    }

    #[tokio::test]
    async fn read_relative_path_resolves_to_working_dir() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("relative.txt");
        std::fs::write(&file_path, "relative content").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = ReadTool;
        let input = serde_json::json!({ "file_path": "relative.txt" });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("relative content"));
    }
}
