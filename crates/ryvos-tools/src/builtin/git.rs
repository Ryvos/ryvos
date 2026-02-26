use std::path::PathBuf;

use futures::future::BoxFuture;
use serde::Deserialize;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

fn resolve(p: &str, wd: &std::path::Path) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        path
    } else {
        wd.join(path)
    }
}

async fn run_git(args: &[&str], cwd: &std::path::Path) -> std::result::Result<String, String> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| format!("Failed to run git: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

// ── GitStatusTool ───────────────────────────────────────────────

pub struct GitStatusTool;

impl Tool for GitStatusTool {
    fn name(&self) -> &str {
        "git_status"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Show git repository status."
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
            match run_git(&["status", "--porcelain=v2", "--branch"], &ctx.working_dir).await {
                Ok(out) => Ok(ToolResult::success(out)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

// ── GitDiffTool ─────────────────────────────────────────────────

pub struct GitDiffTool;

#[derive(Deserialize)]
struct GitDiffInput {
    #[serde(default)]
    cached: bool,
}

impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Show git diff. Use cached=true for staged changes."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "cached": { "type": "boolean", "description": "Show staged changes only" }
            }
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: GitDiffInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let args = if p.cached {
                vec!["diff", "--cached"]
            } else {
                vec!["diff"]
            };
            match run_git(&args, &ctx.working_dir).await {
                Ok(out) => Ok(ToolResult::success(if out.is_empty() {
                    "No changes.".into()
                } else {
                    out
                })),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

// ── GitLogTool ──────────────────────────────────────────────────

pub struct GitLogTool;

#[derive(Deserialize)]
struct GitLogInput {
    #[serde(default = "default_n")]
    n: usize,
}
fn default_n() -> usize {
    10
}

impl Tool for GitLogTool {
    fn name(&self) -> &str {
        "git_log"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Show recent git log entries."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "n": { "type": "integer", "description": "Number of commits (default: 10)" } }
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: GitLogInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let n_str = p.n.to_string();
            match run_git(&["log", "--oneline", "-n", &n_str], &ctx.working_dir).await {
                Ok(out) => Ok(ToolResult::success(out)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

// ── GitCommitTool ───────────────────────────────────────────────

pub struct GitCommitTool;

#[derive(Deserialize)]
struct GitCommitInput {
    message: String,
    #[serde(default)]
    files: Vec<String>,
}

impl Tool for GitCommitTool {
    fn name(&self) -> &str {
        "git_commit"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Stage files and create a git commit."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "Commit message" },
                "files": { "type": "array", "items": { "type": "string" }, "description": "Files to stage (default: all changed)" }
            },
            "required": ["message"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: GitCommitInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            // Stage files
            if p.files.is_empty() {
                run_git(&["add", "-A"], &ctx.working_dir)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "git_commit".into(),
                        message: e,
                    })?;
            } else {
                let mut args = vec!["add"];
                let files: Vec<&str> = p.files.iter().map(|s| s.as_str()).collect();
                args.extend(files);
                run_git(&args, &ctx.working_dir)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "git_commit".into(),
                        message: e,
                    })?;
            }
            // Commit
            match run_git(&["commit", "-m", &p.message], &ctx.working_dir).await {
                Ok(out) => Ok(ToolResult::success(out)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

// ── GitBranchTool ───────────────────────────────────────────────

pub struct GitBranchTool;

#[derive(Deserialize)]
struct GitBranchInput {
    #[serde(default = "default_action")]
    action: String,
    #[serde(default)]
    name: Option<String>,
}
fn default_action() -> String {
    "list".into()
}

impl Tool for GitBranchTool {
    fn name(&self) -> &str {
        "git_branch"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Manage git branches. Actions: list, create, delete, switch."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "description": "list, create, delete, or switch" },
                "name": { "type": "string", "description": "Branch name (for create/delete/switch)" }
            }
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: GitBranchInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let result =
                match p.action.as_str() {
                    "list" => run_git(&["branch", "-a"], &ctx.working_dir).await,
                    "create" => {
                        let name = p.name.as_deref().ok_or_else(|| {
                            RyvosError::ToolValidation("Branch name required".into())
                        })?;
                        run_git(&["branch", name], &ctx.working_dir).await
                    }
                    "delete" => {
                        let name = p.name.as_deref().ok_or_else(|| {
                            RyvosError::ToolValidation("Branch name required".into())
                        })?;
                        run_git(&["branch", "-d", name], &ctx.working_dir).await
                    }
                    "switch" => {
                        let name = p.name.as_deref().ok_or_else(|| {
                            RyvosError::ToolValidation("Branch name required".into())
                        })?;
                        run_git(&["checkout", name], &ctx.working_dir).await
                    }
                    other => Err(format!("Unknown action: {}", other)),
                };
            match result {
                Ok(out) => Ok(ToolResult::success(out)),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}

// ── GitCloneTool ────────────────────────────────────────────────

pub struct GitCloneTool;

#[derive(Deserialize)]
struct GitCloneInput {
    url: String,
    #[serde(default)]
    directory: Option<String>,
}

impl Tool for GitCloneTool {
    fn name(&self) -> &str {
        "git_clone"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn timeout_secs(&self) -> u64 {
        120
    }
    fn description(&self) -> &str {
        "Clone a git repository."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Repository URL" },
                "directory": { "type": "string", "description": "Target directory" }
            },
            "required": ["url"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: GitCloneInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let mut args = vec!["clone", &p.url];
            let dir_str;
            if let Some(ref dir) = p.directory {
                dir_str = resolve(dir, &ctx.working_dir).to_string_lossy().to_string();
                args.push(&dir_str);
            }
            match run_git(
                &args.iter().map(|s| s.as_ref()).collect::<Vec<&str>>(),
                &ctx.working_dir,
            )
            .await
            {
                Ok(out) => Ok(ToolResult::success(format!("Cloned {}\n{}", p.url, out))),
                Err(e) => Ok(ToolResult::error(e)),
            }
        })
    }
}
