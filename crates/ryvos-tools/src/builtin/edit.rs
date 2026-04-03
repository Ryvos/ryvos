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

#[cfg(test)]
mod tests {
    use super::*;
    use ryvos_core::traits::Tool;
    use ryvos_test_utils::test_tool_context_with_dir;

    #[tokio::test]
    async fn edit_single_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("edit_me.txt");
        std::fs::write(&file_path, "hello world\ngoodbye world\n").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = EditTool;
        let input = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "hello world",
            "new_string": "hi world"
        });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Edited"));
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("hi world"));
        assert!(content.contains("goodbye world"));
    }

    #[tokio::test]
    async fn edit_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("multi.txt");
        std::fs::write(&file_path, "foo bar foo baz foo\n").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = EditTool;
        let input = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "foo",
            "new_string": "qux",
            "replace_all": true
        });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Replaced 3 occurrences"));
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(!content.contains("foo"));
        assert_eq!(content.matches("qux").count(), 3);
    }

    #[tokio::test]
    async fn edit_multiple_matches_without_replace_all_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("dup.txt");
        std::fs::write(&file_path, "aaa bbb aaa\n").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = EditTool;
        let input = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "aaa",
            "new_string": "ccc"
        });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("found 2 times"));
    }

    #[tokio::test]
    async fn edit_old_string_not_found_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("miss.txt");
        std::fs::write(&file_path, "some content").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = EditTool;
        let input = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "nonexistent",
            "new_string": "replacement"
        });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("not found"));
    }

    #[tokio::test]
    async fn edit_identical_strings_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("same.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = EditTool;
        let input = serde_json::json!({
            "file_path": file_path.to_str().unwrap(),
            "old_string": "hello",
            "new_string": "hello"
        });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("identical"));
    }
}
