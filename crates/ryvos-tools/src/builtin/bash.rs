use futures::future::BoxFuture;
use futures::StreamExt;
use serde::Deserialize;
use tracing::debug;

use ryvos_core::config::SandboxConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct BashTool;

#[derive(Deserialize)]
struct BashInput {
    command: String,
    #[serde(default = "default_timeout")]
    timeout: u64,
}

fn default_timeout() -> u64 { 120 }

impl BashTool {
    async fn execute_sandboxed(
        command: &str,
        ctx: &ToolContext,
        config: &SandboxConfig,
    ) -> Result<ToolResult> {
        let docker = bollard::Docker::connect_with_local_defaults().map_err(|e| {
            RyvosError::ToolExecution {
                tool: "bash".into(),
                message: format!("Docker connect failed: {}", e),
            }
        })?;

        let mut binds = vec![];
        if config.mount_workspace {
            binds.push(format!("{}:/workspace", ctx.working_dir.display()));
        }

        let container_config = bollard::container::Config {
            image: Some(config.image.clone()),
            cmd: Some(vec![
                "bash".to_string(),
                "-c".to_string(),
                command.to_string(),
            ]),
            working_dir: Some("/workspace".to_string()),
            host_config: Some(bollard::models::HostConfig {
                memory: Some((config.memory_mb as i64) * 1024 * 1024),
                binds: Some(binds),
                network_mode: Some("none".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let container = docker
            .create_container::<&str, String>(None, container_config)
            .await
            .map_err(|e| RyvosError::ToolExecution {
                tool: "bash".into(),
                message: format!("Docker create failed: {}", e),
            })?;

        docker
            .start_container::<String>(&container.id, None)
            .await
            .map_err(|e| RyvosError::ToolExecution {
                tool: "bash".into(),
                message: format!("Docker start failed: {}", e),
            })?;

        // Wait with timeout
        let timeout = std::time::Duration::from_secs(config.timeout_secs);
        let wait_result = tokio::time::timeout(timeout, async {
            let mut stream = docker.wait_container::<String>(
                &container.id,
                None::<bollard::container::WaitContainerOptions<String>>,
            );
            stream.next().await
        })
        .await;

        // Collect logs
        let log_options = bollard::container::LogsOptions::<String> {
            stdout: true,
            stderr: true,
            ..Default::default()
        };
        let mut log_stream = docker.logs(&container.id, Some(log_options));
        let mut output = String::new();
        while let Some(Ok(log)) = log_stream.next().await {
            output.push_str(&log.to_string());
        }

        // Truncate if too long
        if output.len() > 30000 {
            output.truncate(30000);
            output.push_str("\n... (output truncated)");
        }

        if output.is_empty() {
            output = "(no output)".to_string();
        }

        // Cleanup container
        let remove_options = bollard::container::RemoveContainerOptions {
            force: true,
            ..Default::default()
        };
        docker
            .remove_container(&container.id, Some(remove_options))
            .await
            .ok();

        match wait_result {
            Ok(Some(Ok(exit))) => {
                let code = exit.status_code;
                if code == 0 {
                    Ok(ToolResult::success(output))
                } else {
                    Ok(ToolResult::error(format!(
                        "Exit code {}\n{}",
                        code, output
                    )))
                }
            }
            Ok(Some(Err(e))) => Err(RyvosError::ToolExecution {
                tool: "bash".into(),
                message: format!("Docker wait failed: {}", e),
            }),
            Ok(None) => Ok(ToolResult::success(output)),
            Err(_) => {
                // Timeout — kill the container
                docker
                    .kill_container::<String>(&container.id, None)
                    .await
                    .ok();
                Err(RyvosError::ToolTimeout {
                    tool: "bash".to_string(),
                    timeout_secs: config.timeout_secs,
                })
            }
        }
    }
}

impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T2
    }

    fn description(&self) -> &str {
        "Execute a bash command. Returns stdout and stderr. Use for system commands, git operations, builds, etc."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (default 120)",
                    "default": 120
                }
            },
            "required": ["command"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: BashInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            debug!(command = %params.command, "Executing bash command");

            // Check if sandbox is enabled
            if let Some(ref sandbox_config) = ctx.sandbox_config {
                if sandbox_config.enabled && sandbox_config.mode == "docker" {
                    return Self::execute_sandboxed(&params.command, &ctx, sandbox_config).await;
                }
            }

            // Unsandboxed execution (existing behavior)
            let timeout = std::time::Duration::from_secs(params.timeout);
            let result = tokio::time::timeout(timeout, async {
                tokio::process::Command::new("bash")
                    .arg("-c")
                    .arg(&params.command)
                    .current_dir(&ctx.working_dir)
                    .output()
                    .await
            })
            .await;

            match result {
                Ok(Ok(output)) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);

                    let mut content = String::new();
                    if !stdout.is_empty() {
                        content.push_str(&stdout);
                    }
                    if !stderr.is_empty() {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        content.push_str("STDERR:\n");
                        content.push_str(&stderr);
                    }

                    // Truncate if too long
                    if content.len() > 30000 {
                        content.truncate(30000);
                        content.push_str("\n... (output truncated)");
                    }

                    if content.is_empty() {
                        content = "(no output)".to_string();
                    }

                    if output.status.success() {
                        Ok(ToolResult::success(content))
                    } else {
                        let code = output.status.code().unwrap_or(-1);
                        Ok(ToolResult::error(format!(
                            "Exit code {}\n{}",
                            code, content
                        )))
                    }
                }
                Ok(Err(e)) => Err(RyvosError::ToolExecution {
                    tool: "bash".to_string(),
                    message: e.to_string(),
                }),
                Err(_) => Err(RyvosError::ToolTimeout {
                    tool: "bash".to_string(),
                    timeout_secs: params.timeout,
                }),
            }
        })
    }

    fn timeout_secs(&self) -> u64 {
        120
    }

    fn requires_sandbox(&self) -> bool {
        true
    }
}
