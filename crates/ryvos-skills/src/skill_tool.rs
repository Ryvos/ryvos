use std::path::PathBuf;

use futures::future::BoxFuture;
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

use crate::manifest::SkillManifest;

/// A tool backed by a shell command from a skill manifest.
pub struct SkillTool {
    manifest: SkillManifest,
    skill_dir: PathBuf,
    schema: serde_json::Value,
}

impl SkillTool {
    pub fn new(manifest: SkillManifest, skill_dir: PathBuf) -> Result<Self> {
        let schema: serde_json::Value =
            serde_json::from_str(&manifest.input_schema_json).map_err(|e| {
                RyvosError::Config(format!(
                    "Invalid input_schema_json for skill '{}': {}",
                    manifest.name, e
                ))
            })?;

        Ok(Self {
            manifest,
            skill_dir,
            schema,
        })
    }
}

impl Tool for SkillTool {
    fn name(&self) -> &str {
        &self.manifest.name
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        self.manifest
            .tier
            .parse()
            .unwrap_or(ryvos_core::security::SecurityTier::T2)
    }

    fn description(&self) -> &str {
        &self.manifest.description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        let command = self
            .manifest
            .command
            .replace("$SKILL_DIR", &self.skill_dir.display().to_string());
        let timeout_secs = self.manifest.timeout_secs;
        let input_bytes = serde_json::to_vec(&input).unwrap_or_default();
        let working_dir = ctx.working_dir.clone();

        Box::pin(async move {
            debug!(command = %command, "Executing skill command");

            let timeout = std::time::Duration::from_secs(timeout_secs);
            let result = tokio::time::timeout(timeout, async {
                let mut cmd = if cfg!(windows) {
                    let mut c = tokio::process::Command::new("cmd");
                    c.arg("/C").arg(&command);
                    c
                } else {
                    let mut c = tokio::process::Command::new("bash");
                    c.arg("-c").arg(&command);
                    c
                };
                let mut child = cmd
                    .current_dir(&working_dir)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()?;

                // Write JSON input to stdin
                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    stdin.write_all(&input_bytes).await.ok();
                    // Drop stdin to close it so the child can read EOF
                }

                child.wait_with_output().await
            })
            .await;

            match result {
                Ok(Ok(output)) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                    if output.status.success() {
                        Ok(ToolResult::success(if stdout.is_empty() {
                            "(no output)".to_string()
                        } else {
                            stdout
                        }))
                    } else {
                        let msg = if stderr.is_empty() { stdout } else { stderr };
                        Ok(ToolResult::error(format!(
                            "Exit code {}\n{}",
                            output.status.code().unwrap_or(-1),
                            msg
                        )))
                    }
                }
                Ok(Err(e)) => Err(RyvosError::ToolExecution {
                    tool: command,
                    message: e.to_string(),
                }),
                Err(_) => Err(RyvosError::ToolTimeout {
                    tool: command,
                    timeout_secs,
                }),
            }
        })
    }

    fn timeout_secs(&self) -> u64 {
        self.manifest.timeout_secs
    }

    fn requires_sandbox(&self) -> bool {
        self.manifest.requires_sandbox
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::manifest::SkillManifest;
    use ryvos_core::types::SessionId;

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: SessionId::new(),
            working_dir: std::env::temp_dir(),
            store: None,
            agent_spawner: None,
            sandbox_config: None,
            config_path: None,
        }
    }

    #[tokio::test]
    async fn skill_tool_echo() {
        let manifest = SkillManifest {
            name: "echo_test".into(),
            description: "Echo test".into(),
            command: "cat".into(),
            timeout_secs: 5,
            requires_sandbox: false,
            input_schema_json: r#"{"type":"object","properties":{"text":{"type":"string"}}}"#
                .into(),
            tier: "t2".into(),
            prerequisites: Default::default(),
        };
        let tool = SkillTool::new(manifest, std::env::temp_dir()).unwrap();
        let input = serde_json::json!({"text": "hello"});
        let result = tool.execute(input.clone(), test_ctx()).await.unwrap();
        assert!(!result.is_error);
        // cat echoes back the JSON input
        let echoed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(echoed, input);
    }

    #[tokio::test]
    async fn skill_tool_failure() {
        let manifest = SkillManifest {
            name: "fail_test".into(),
            description: "Fail test".into(),
            command: "exit 42".into(),
            timeout_secs: 5,
            requires_sandbox: false,
            input_schema_json: r#"{"type":"object"}"#.into(),
            tier: "t2".into(),
            prerequisites: Default::default(),
        };
        let tool = SkillTool::new(manifest, std::env::temp_dir()).unwrap();
        let result = tool
            .execute(serde_json::json!({}), test_ctx())
            .await
            .unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("Exit code 42"));
    }
}
