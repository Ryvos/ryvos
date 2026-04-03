use std::path::PathBuf;

use futures::future::BoxFuture;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};
use serde::Deserialize;

fn resolve(p: &str, wd: &std::path::Path) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        path
    } else {
        wd.join(path)
    }
}

// ── FileInfoTool ────────────────────────────────────────────────

pub struct FileInfoTool;

#[derive(Deserialize)]
struct FileInfoInput {
    path: String,
}

impl Tool for FileInfoTool {
    fn name(&self) -> &str {
        "file_info"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Get file metadata: size, permissions, modification time."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "File path" } },
            "required": ["path"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: FileInfoInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let path = resolve(&p.path, &ctx.working_dir);
            let meta = tokio::fs::metadata(&path)
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "file_info".into(),
                    message: format!("{}: {}", path.display(), e),
                })?;
            let mtime = meta
                .modified()
                .ok()
                .map(|t| format!("{:?}", t))
                .unwrap_or_else(|| "unknown".into());
            Ok(ToolResult::success(format!(
                "Path: {}\nSize: {} bytes\nIs file: {}\nIs dir: {}\nReadonly: {}\nModified: {}",
                path.display(),
                meta.len(),
                meta.is_file(),
                meta.is_dir(),
                meta.permissions().readonly(),
                mtime
            )))
        })
    }
}

// ── FileCopyTool ────────────────────────────────────────────────

pub struct FileCopyTool;

#[derive(Deserialize)]
struct FileCopyInput {
    source: String,
    destination: String,
    #[serde(default)]
    recursive: bool,
}

impl Tool for FileCopyTool {
    fn name(&self) -> &str {
        "file_copy"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T1
    }
    fn description(&self) -> &str {
        "Copy a file or directory."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "source": { "type": "string" },
                "destination": { "type": "string" },
                "recursive": { "type": "boolean", "description": "Recursive copy for directories" }
            },
            "required": ["source", "destination"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: FileCopyInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let src = resolve(&p.source, &ctx.working_dir);
            let dst = resolve(&p.destination, &ctx.working_dir);
            if p.recursive || src.is_dir() {
                let output = tokio::process::Command::new("cp")
                    .args(["-r", &src.to_string_lossy(), &dst.to_string_lossy()])
                    .output()
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "file_copy".into(),
                        message: e.to_string(),
                    })?;
                if !output.status.success() {
                    return Ok(ToolResult::error(
                        String::from_utf8_lossy(&output.stderr).to_string(),
                    ));
                }
            } else {
                tokio::fs::copy(&src, &dst)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "file_copy".into(),
                        message: e.to_string(),
                    })?;
            }
            Ok(ToolResult::success(format!(
                "Copied {} → {}",
                src.display(),
                dst.display()
            )))
        })
    }
}

// ── FileMoveTool ────────────────────────────────────────────────

pub struct FileMoveTool;

#[derive(Deserialize)]
struct FileMoveInput {
    source: String,
    destination: String,
}

impl Tool for FileMoveTool {
    fn name(&self) -> &str {
        "file_move"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T1
    }
    fn description(&self) -> &str {
        "Move or rename a file or directory."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "source": { "type": "string" }, "destination": { "type": "string" } },
            "required": ["source", "destination"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: FileMoveInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let src = resolve(&p.source, &ctx.working_dir);
            let dst = resolve(&p.destination, &ctx.working_dir);
            tokio::fs::rename(&src, &dst)
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "file_move".into(),
                    message: e.to_string(),
                })?;
            Ok(ToolResult::success(format!(
                "Moved {} → {}",
                src.display(),
                dst.display()
            )))
        })
    }
}

// ── FileDeleteTool ──────────────────────────────────────────────

pub struct FileDeleteTool;

#[derive(Deserialize)]
struct FileDeleteInput {
    path: String,
}

impl Tool for FileDeleteTool {
    fn name(&self) -> &str {
        "file_delete"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T3
    }
    fn description(&self) -> &str {
        "Delete a file or directory (recursively)."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: FileDeleteInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let path = resolve(&p.path, &ctx.working_dir);
            let meta = tokio::fs::metadata(&path)
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "file_delete".into(),
                    message: e.to_string(),
                })?;
            if meta.is_dir() {
                tokio::fs::remove_dir_all(&path).await
            } else {
                tokio::fs::remove_file(&path).await
            }
            .map_err(|e| RyvosError::ToolExecution {
                tool: "file_delete".into(),
                message: e.to_string(),
            })?;
            Ok(ToolResult::success(format!("Deleted {}", path.display())))
        })
    }
}

// ── DirListTool ─────────────────────────────────────────────────

pub struct DirListTool;

#[derive(Deserialize)]
struct DirListInput {
    #[serde(default)]
    path: Option<String>,
}

impl Tool for DirListTool {
    fn name(&self) -> &str {
        "dir_list"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "List directory contents with metadata."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "Directory path (default: working dir)" } }
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: DirListInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let dir = p
                .path
                .map(|d| resolve(&d, &ctx.working_dir))
                .unwrap_or_else(|| ctx.working_dir.clone());
            let mut entries =
                tokio::fs::read_dir(&dir)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "dir_list".into(),
                        message: e.to_string(),
                    })?;
            let mut output = format!("Directory: {}\n", dir.display());
            let mut count = 0;
            while let Some(entry) =
                entries
                    .next_entry()
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "dir_list".into(),
                        message: e.to_string(),
                    })?
            {
                let meta = entry.metadata().await.ok();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let kind = if meta.as_ref().is_some_and(|m| m.is_dir()) {
                    "dir "
                } else {
                    "file"
                };
                output.push_str(&format!(
                    "  {} {:>10}  {}\n",
                    kind,
                    size,
                    entry.file_name().to_string_lossy()
                ));
                count += 1;
                if count >= 500 {
                    output.push_str("  ... (truncated at 500 entries)\n");
                    break;
                }
            }
            Ok(ToolResult::success(output))
        })
    }
}

// ── DirCreateTool ───────────────────────────────────────────────

pub struct DirCreateTool;

#[derive(Deserialize)]
struct DirCreateInput {
    path: String,
}

impl Tool for DirCreateTool {
    fn name(&self) -> &str {
        "dir_create"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T1
    }
    fn description(&self) -> &str {
        "Create a directory (and parents)."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: DirCreateInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let path = resolve(&p.path, &ctx.working_dir);
            tokio::fs::create_dir_all(&path)
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "dir_create".into(),
                    message: e.to_string(),
                })?;
            Ok(ToolResult::success(format!("Created {}", path.display())))
        })
    }
}

// ── FileWatchTool ───────────────────────────────────────────────

pub struct FileWatchTool;

#[derive(Deserialize)]
struct FileWatchInput {
    path: String,
}

impl Tool for FileWatchTool {
    fn name(&self) -> &str {
        "file_watch"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T1
    }
    fn description(&self) -> &str {
        "Check file existence and get current state."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: FileWatchInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let path = resolve(&p.path, &ctx.working_dir);
            match tokio::fs::metadata(&path).await {
                Ok(meta) => {
                    let mtime = meta
                        .modified()
                        .ok()
                        .map(|t| format!("{:?}", t))
                        .unwrap_or_else(|| "unknown".into());
                    Ok(ToolResult::success(format!(
                        "Exists: true\nSize: {} bytes\nModified: {}",
                        meta.len(),
                        mtime
                    )))
                }
                Err(_) => Ok(ToolResult::success("Exists: false".to_string())),
            }
        })
    }
}

// ── ArchiveCreateTool ───────────────────────────────────────────

pub struct ArchiveCreateTool;

#[derive(Deserialize)]
struct ArchiveCreateInput {
    source: String,
    output: String,
    #[serde(default = "default_format")]
    format: String,
}
fn default_format() -> String {
    "tar.gz".into()
}

impl Tool for ArchiveCreateTool {
    fn name(&self) -> &str {
        "archive_create"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T1
    }
    fn description(&self) -> &str {
        "Create a .tar.gz or .zip archive."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "source": { "type": "string", "description": "Source file or directory" },
                "output": { "type": "string", "description": "Output archive path" },
                "format": { "type": "string", "description": "Archive format: tar.gz or zip (default: tar.gz)" }
            },
            "required": ["source", "output"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: ArchiveCreateInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let src = resolve(&p.source, &ctx.working_dir);
            let out = resolve(&p.output, &ctx.working_dir);
            let output = if p.format == "zip" {
                tokio::process::Command::new("zip")
                    .args(["-r", &out.to_string_lossy(), &src.to_string_lossy()])
                    .output()
                    .await
            } else {
                tokio::process::Command::new("tar")
                    .args([
                        "czf",
                        &out.to_string_lossy(),
                        "-C",
                        &src.parent()
                            .unwrap_or(std::path::Path::new("."))
                            .to_string_lossy(),
                        &src.file_name().unwrap_or_default().to_string_lossy(),
                    ])
                    .output()
                    .await
            }
            .map_err(|e| RyvosError::ToolExecution {
                tool: "archive_create".into(),
                message: e.to_string(),
            })?;
            if output.status.success() {
                Ok(ToolResult::success(format!(
                    "Archive created: {}",
                    out.display()
                )))
            } else {
                Ok(ToolResult::error(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ))
            }
        })
    }
}

// ── ArchiveExtractTool ──────────────────────────────────────────

pub struct ArchiveExtractTool;

#[derive(Deserialize)]
struct ArchiveExtractInput {
    archive: String,
    #[serde(default)]
    destination: Option<String>,
}

impl Tool for ArchiveExtractTool {
    fn name(&self) -> &str {
        "archive_extract"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T1
    }
    fn description(&self) -> &str {
        "Extract a .tar.gz or .zip archive."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "archive": { "type": "string", "description": "Archive file path" },
                "destination": { "type": "string", "description": "Extraction directory (default: working dir)" }
            },
            "required": ["archive"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: ArchiveExtractInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let archive = resolve(&p.archive, &ctx.working_dir);
            let dest = p
                .destination
                .map(|d| resolve(&d, &ctx.working_dir))
                .unwrap_or_else(|| ctx.working_dir.clone());
            let ext = archive.to_string_lossy();
            let output = if ext.ends_with(".zip") {
                tokio::process::Command::new("unzip")
                    .args([&archive.to_string_lossy(), "-d", &dest.to_string_lossy()])
                    .output()
                    .await
            } else {
                tokio::process::Command::new("tar")
                    .args([
                        "xzf",
                        &archive.to_string_lossy(),
                        "-C",
                        &dest.to_string_lossy(),
                    ])
                    .output()
                    .await
            }
            .map_err(|e| RyvosError::ToolExecution {
                tool: "archive_extract".into(),
                message: e.to_string(),
            })?;
            if output.status.success() {
                Ok(ToolResult::success(format!(
                    "Extracted to {}",
                    dest.display()
                )))
            } else {
                Ok(ToolResult::error(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryvos_core::traits::Tool;
    use ryvos_test_utils::test_tool_context_with_dir;

    // ── FileInfoTool tests ─────────────────────────────────────

    #[tokio::test]
    async fn file_info_returns_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("info.txt");
        std::fs::write(&file_path, "twelve chars").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = FileInfoTool;
        let input = serde_json::json!({ "path": file_path.to_str().unwrap() });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Size: 12 bytes"));
        assert!(result.content.contains("Is file: true"));
        assert!(result.content.contains("Is dir: false"));
    }

    #[tokio::test]
    async fn file_info_directory() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = FileInfoTool;
        let input = serde_json::json!({ "path": dir.path().to_str().unwrap() });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Is dir: true"));
        assert!(result.content.contains("Is file: false"));
    }

    #[tokio::test]
    async fn file_info_nonexistent_errors() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = FileInfoTool;
        let input = serde_json::json!({ "path": "/tmp/ryvos_no_such_file_xyz_test" });
        let result = tool.execute(input, ctx).await;
        assert!(result.is_err());
    }

    // ── FileCopyTool tests ─────────────────────────────────────

    #[tokio::test]
    async fn file_copy_basic() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        std::fs::write(&src, "copy me").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = FileCopyTool;
        let input = serde_json::json!({
            "source": src.to_str().unwrap(),
            "destination": dst.to_str().unwrap()
        });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Copied"));
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "copy me");
        // Source should still exist
        assert!(src.exists());
    }

    // ── FileMoveTool tests ─────────────────────────────────────

    #[tokio::test]
    async fn file_move_basic() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("move_src.txt");
        let dst = dir.path().join("move_dst.txt");
        std::fs::write(&src, "move me").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = FileMoveTool;
        let input = serde_json::json!({
            "source": src.to_str().unwrap(),
            "destination": dst.to_str().unwrap()
        });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Moved"));
        assert!(!src.exists());
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "move me");
    }

    // ── FileDeleteTool tests ───────────────────────────────────

    #[tokio::test]
    async fn file_delete_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("delete_me.txt");
        std::fs::write(&file_path, "bye").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = FileDeleteTool;
        let input = serde_json::json!({ "path": file_path.to_str().unwrap() });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Deleted"));
        assert!(!file_path.exists());
    }

    #[tokio::test]
    async fn file_delete_directory() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub_dir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("child.txt"), "x").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = FileDeleteTool;
        let input = serde_json::json!({ "path": sub.to_str().unwrap() });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(!sub.exists());
    }

    // ── DirListTool tests ──────────────────────────────────────

    #[tokio::test]
    async fn dir_list_shows_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("alpha.txt"), "a").unwrap();
        std::fs::create_dir(dir.path().join("beta_dir")).unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = DirListTool;
        let input = serde_json::json!({ "path": dir.path().to_str().unwrap() });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("alpha.txt"));
        assert!(result.content.contains("beta_dir"));
        assert!(result.content.contains("dir "));
        assert!(result.content.contains("file"));
    }

    #[tokio::test]
    async fn dir_list_defaults_to_working_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("inwd.txt"), "").unwrap();

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = DirListTool;
        let input = serde_json::json!({});
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("inwd.txt"));
    }

    // ── DirCreateTool tests ────────────────────────────────────

    #[tokio::test]
    async fn dir_create_basic() {
        let dir = tempfile::tempdir().unwrap();
        let new_dir = dir.path().join("new_folder");

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = DirCreateTool;
        let input = serde_json::json!({ "path": new_dir.to_str().unwrap() });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("Created"));
        assert!(new_dir.is_dir());
    }

    #[tokio::test]
    async fn dir_create_nested() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a/b/c/d");

        let ctx = test_tool_context_with_dir(dir.path().to_path_buf());
        let tool = DirCreateTool;
        let input = serde_json::json!({ "path": deep.to_str().unwrap() });
        let result = tool.execute(input, ctx).await.unwrap();
        assert!(!result.is_error);
        assert!(deep.is_dir());
    }
}
