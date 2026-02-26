use futures::future::BoxFuture;
use serde::Deserialize;
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct ApplyPatchTool;

#[derive(Deserialize)]
struct ApplyPatchInput {
    patch: String,
    #[serde(default)]
    dry_run: Option<bool>,
}

impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T1
    }

    fn description(&self) -> &str {
        "Apply a unified diff patch to files. Supports dry_run mode for validation without writing."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "Unified diff text to apply"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "If true, validate the patch without applying it (default: false)"
                }
            },
            "required": ["patch"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: ApplyPatchInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            let dry_run = params.dry_run.unwrap_or(false);

            debug!(dry_run, "Applying patch");

            let mut cmd = tokio::process::Command::new("patch");
            cmd.arg("-p1");
            if dry_run {
                cmd.arg("--dry-run");
            }
            cmd.current_dir(&ctx.working_dir);
            cmd.stdin(std::process::Stdio::piped());
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| RyvosError::ToolExecution {
                tool: "apply_patch".to_string(),
                message: format!("Failed to run patch command: {}", e),
            })?;

            // Write patch to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                stdin
                    .write_all(params.patch.as_bytes())
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "apply_patch".to_string(),
                        message: format!("Failed to write patch to stdin: {}", e),
                    })?;
            }

            let output = child
                .wait_with_output()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "apply_patch".to_string(),
                    message: format!("Failed to wait for patch: {}", e),
                })?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if output.status.success() {
                let prefix = if dry_run {
                    "Dry run OK"
                } else {
                    "Patch applied"
                };
                let msg = if stdout.is_empty() {
                    prefix.to_string()
                } else {
                    format!("{}:\n{}", prefix, stdout)
                };
                Ok(ToolResult::success(msg))
            } else {
                let msg = format!(
                    "Patch failed (exit {}):\n{}\n{}",
                    output.status.code().unwrap_or(-1),
                    stdout,
                    stderr
                );
                Ok(ToolResult::error(msg))
            }
        })
    }
}
