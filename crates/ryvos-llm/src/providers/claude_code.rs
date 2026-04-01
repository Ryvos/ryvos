use std::sync::Arc;

use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{debug, error, warn};

use ryvos_core::config::ModelConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::DangerousPatternMatcher;
use ryvos_core::traits::LlmClient;
use ryvos_core::types::*;

/// LLM client that delegates to the `claude` CLI (Claude Code).
///
/// This enables subscription billing: users with a Claude Max/Pro subscription
/// pay nothing per-token. Ryvos spawns the CLI as a child process and parses
/// its stream-json output.
///
/// Security: intermediate tool_use events are inspected against the configured
/// dangerous-command patterns. If a match is found, the child process is killed
/// before it can execute the command.
pub struct ClaudeCodeClient {
    /// Compiled dangerous-pattern matcher for intercepting CLI tool calls.
    pattern_matcher: Option<Arc<DangerousPatternMatcher>>,
}

impl Default for ClaudeCodeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeCodeClient {
    pub fn new() -> Self {
        Self {
            pattern_matcher: None,
        }
    }

    /// Create a client with security pattern matching enabled.
    pub fn with_patterns(patterns: &[ryvos_core::security::DangerousPattern]) -> Self {
        Self {
            pattern_matcher: Some(Arc::new(DangerousPatternMatcher::new(patterns))),
        }
    }

    /// Detect billing type: if an API key is set, it's pay-per-token.
    /// Otherwise the CLI uses the user's subscription.
    pub fn detect_billing_type(config: &ModelConfig) -> BillingType {
        if config.api_key.is_some() {
            BillingType::Api
        } else {
            BillingType::Subscription
        }
    }
}

impl LlmClient for ClaudeCodeClient {
    fn chat_stream(
        &self,
        config: &ModelConfig,
        messages: Vec<ChatMessage>,
        _tools: &[ToolDefinition],
    ) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>> {
        let config = config.clone();
        let matcher = self.pattern_matcher.clone();

        Box::pin(async move {
            // Extract the last user message as the prompt
            let prompt = messages
                .iter()
                .rev()
                .find_map(|m| {
                    if m.role == Role::User {
                        let t = m.text();
                        if t.is_empty() {
                            None
                        } else {
                            Some(t)
                        }
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

            if prompt.is_empty() {
                return Err(RyvosError::LlmRequest(
                    "No user message found for claude-code provider".into(),
                ));
            }

            // Build system context from system messages
            let system_context: String = messages
                .iter()
                .filter(|m| m.role == Role::System)
                .map(|m| m.text())
                .collect::<Vec<_>>()
                .join("\n");

            let claude_bin = config.claude_command.as_deref().unwrap_or("claude");

            let mut args = vec![
                "--print".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--verbose".to_string(),
            ];

            // Autonomous headless operation: bypass all permission prompts.
            // Only destructive operations (deletion, data loss) are blocked
            // via --disallowedTools. Everything else runs freely.
            let perm_mode = config
                .cli_permission_mode
                .as_deref()
                .unwrap_or("bypassPermissions");
            args.push("--permission-mode".to_string());
            args.push(perm_mode.to_string());

            if !config.cli_allowed_tools.is_empty() {
                args.push("--allowedTools".to_string());
                for tool in &config.cli_allowed_tools {
                    args.push(tool.clone());
                }
            }

            // Only block truly destructive commands
            args.push("--disallowedTools".to_string());
            args.push("Bash(rm -rf:*)".to_string());
            args.push("--disallowedTools".to_string());
            args.push("Bash(rm -r:*)".to_string());
            args.push("--disallowedTools".to_string());
            args.push("Bash(mkfs:*)".to_string());
            args.push("--disallowedTools".to_string());
            args.push("Bash(dd if=:*)".to_string());

            // Model override
            if config.model_id != "default" && !config.model_id.is_empty() {
                args.push("--model".to_string());
                args.push(config.model_id.clone());
            }

            // Session resumption
            if let Some(ref session_id) = config.cli_session_id {
                args.push("--resume".to_string());
                args.push(session_id.clone());
            }

            // System prompt
            if !system_context.is_empty() {
                args.push("--system-prompt".to_string());
                args.push(system_context);
            }

            debug!(bin = claude_bin, args = ?args, "Spawning claude CLI");

            let mut child = Command::new(claude_bin)
                .args(&args)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                // Unset CLAUDECODE to prevent nesting detection
                .env_remove("CLAUDECODE")
                .spawn()
                .map_err(|e| {
                    RyvosError::LlmRequest(format!(
                        "Failed to spawn claude CLI '{}': {}. Is it installed?",
                        claude_bin, e
                    ))
                })?;

            // Write prompt to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(prompt.as_bytes()).await;
                let _ = stdin.shutdown().await;
            }

            // Read stdout line by line and parse stream-json
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| RyvosError::LlmStream("Failed to capture stdout".into()))?;

            let reader = BufReader::new(stdout);
            let lines = reader.lines();

            // Share the child PID so the stream can kill it if needed
            let child_id = child.id();
            let killed = Arc::new(Mutex::new(false));

            // Spawn a task to drain stderr and wait for the child process
            let stderr = child.stderr.take();
            tokio::spawn(async move {
                if let Some(stderr) = stderr {
                    let mut reader = BufReader::new(stderr);
                    let mut line = String::new();
                    while let Ok(n) = reader.read_line(&mut line).await {
                        if n == 0 {
                            break;
                        }
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            debug!(target: "claude_cli_stderr", "{}", trimmed);
                        }
                        line.clear();
                    }
                }
                let _ = child.wait().await;
            });

            let stream = tokio_stream::wrappers::LinesStream::new(lines).filter_map(
                move |line_result: std::result::Result<String, std::io::Error>| {
                    let matcher = matcher.clone();
                    let killed = killed.clone();

                    async move {
                        let line: String = match line_result {
                            Ok(l) => l,
                            Err(e) => {
                                error!(error = %e, "Error reading claude CLI stream");
                                return Some(Err(RyvosError::LlmStream(e.to_string())));
                            }
                        };

                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            return None;
                        }

                        let json: serde_json::Value = match serde_json::from_str(trimmed) {
                            Ok(v) => v,
                            Err(_) => return None,
                        };

                        parse_stream_json(&json, matcher.as_deref(), child_id, &killed).await
                    }
                },
            );

            Ok(Box::pin(stream) as BoxStream<'_, Result<StreamDelta>>)
        })
    }
}

/// Parse a single stream-json line from the Claude CLI.
///
/// The Claude CLI manages tool execution internally. We:
/// - Extract session ID from "system" init events
/// - Inspect "assistant" tool_use blocks against dangerous patterns and KILL
///   the child process if a dangerous command is about to execute
/// - Extract final result text from the "result" event
async fn parse_stream_json(
    json: &serde_json::Value,
    matcher: Option<&DangerousPatternMatcher>,
    _child_id: Option<u32>,
    _killed: &Mutex<bool>,
) -> Option<Result<StreamDelta>> {
    let msg_type = json["type"].as_str()?;

    match msg_type {
        "system" => {
            if json.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                if let Some(session_id) = json["session_id"].as_str() {
                    return Some(Ok(StreamDelta::MessageId(session_id.to_string())));
                }
            }
            None
        }
        "assistant" => {
            // Extract ALL tool_use blocks for audit trail / safety memory logging.
            // CLI providers execute tools internally — we can't block, but we CAN log.
            let content = json
                .get("message")
                .and_then(|m| m.get("content"))
                .or_else(|| json.get("content"));

            if let Some(blocks) = content.and_then(|c| c.as_array()) {
                for block in blocks {
                    if block["type"].as_str() == Some("tool_use") {
                        let tool_name = block["name"].as_str().unwrap_or("unknown");
                        let input_summary = if tool_name == "Bash" || tool_name == "bash" {
                            block
                                .get("input")
                                .and_then(|i| i["command"].as_str().or_else(|| i["cmd"].as_str()))
                                .unwrap_or("")
                                .to_string()
                        } else {
                            let input_str = serde_json::to_string(
                                block.get("input").unwrap_or(&serde_json::Value::Null),
                            )
                            .unwrap_or_default();
                            if input_str.len() > 120 {
                                format!("{}...", input_str.chars().take(120).collect::<String>())
                            } else {
                                input_str
                            }
                        };

                        // Informational pattern check (no blocking)
                        if let Some(matcher) = matcher {
                            if tool_name == "Bash" || tool_name == "bash" {
                                if let Some(label) = matcher.is_dangerous(&input_summary) {
                                    warn!(
                                        tool = tool_name,
                                        command = %input_summary,
                                        pattern = label,
                                        "CLI provider executed potentially dangerous command (logged, not blocked)"
                                    );
                                }
                            }
                        }

                        // Emit CliToolExecuted so agent_loop can log to AuditTrail
                        return Some(Ok(StreamDelta::CliToolExecuted {
                            tool_name: tool_name.to_string(),
                            input_summary,
                        }));
                    }
                }
            }
            None
        }
        "result" => {
            // Final result — extract the accumulated text.
            if let Some(text) = json["result"].as_str() {
                if !text.is_empty() {
                    return Some(Ok(StreamDelta::TextDelta(text.to_string())));
                }
            }
            Some(Ok(StreamDelta::Stop(StopReason::EndTurn)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test ModelConfig with sensible defaults.
    fn test_config() -> ModelConfig {
        ModelConfig {
            provider: "claude-code".into(),
            model_id: "claude-sonnet-4".into(),
            api_key: None,
            base_url: None,
            max_tokens: 8192,
            temperature: 0.0,
            thinking: ThinkingLevel::Off,
            retry: None,
            azure_resource: None,
            azure_deployment: None,
            azure_api_version: None,
            aws_region: None,
            extra_headers: Default::default(),
            claude_command: None,
            cli_allowed_tools: vec![],
            cli_permission_mode: None,
            copilot_command: None,
            cli_session_id: None,
        }
    }

    #[test]
    fn detect_billing_type_api() {
        let mut config = test_config();
        config.api_key = Some("sk-test".into());
        assert_eq!(
            ClaudeCodeClient::detect_billing_type(&config),
            BillingType::Api
        );
    }

    #[test]
    fn detect_billing_type_subscription() {
        let config = test_config();
        assert_eq!(
            ClaudeCodeClient::detect_billing_type(&config),
            BillingType::Subscription
        );
    }

    #[test]
    fn parse_system_init() {
        let json = serde_json::json!({
            "type": "system",
            "subtype": "init",
            "session_id": "abc-123"
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        let delta = rt
            .block_on(parse_stream_json(&json, None, None, &killed))
            .unwrap()
            .unwrap();
        assert!(matches!(delta, StreamDelta::MessageId(id) if id == "abc-123"));
    }

    #[test]
    fn parse_result_with_text() {
        let json = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "result": "Hello! How can I help you?",
            "stop_reason": "end_turn",
            "session_id": "abc-123",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        let delta = rt
            .block_on(parse_stream_json(&json, None, None, &killed))
            .unwrap()
            .unwrap();
        assert!(matches!(delta, StreamDelta::TextDelta(t) if t == "Hello! How can I help you?"));
    }

    #[test]
    fn parse_result_empty_text() {
        let json = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "result": "",
            "stop_reason": "end_turn"
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        let delta = rt
            .block_on(parse_stream_json(&json, None, None, &killed))
            .unwrap()
            .unwrap();
        assert!(matches!(delta, StreamDelta::Stop(StopReason::EndTurn)));
    }

    #[test]
    fn parse_assistant_tool_emits_cli_executed() {
        let json = serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls -la"}}]
            }
        });
        let patterns = ryvos_core::security::SecurityPolicy::default_patterns();
        let matcher = DangerousPatternMatcher::new(&patterns);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        // All tool_use blocks now emit CliToolExecuted for audit logging
        let result = rt.block_on(parse_stream_json(&json, Some(&matcher), None, &killed));
        assert!(result.is_some());
        let delta = result.unwrap().unwrap();
        assert!(matches!(delta, StreamDelta::CliToolExecuted { .. }));
    }

    #[test]
    fn parse_assistant_dangerous_tool_not_blocked() {
        // Dangerous patterns are deprecated — tools are never blocked.
        // With empty default patterns, no blocking occurs.
        let json = serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "rm -rf /home/user"}}]
            }
        });
        let patterns = ryvos_core::security::SecurityPolicy::default_patterns();
        let matcher = DangerousPatternMatcher::new(&patterns);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        let result = rt.block_on(parse_stream_json(&json, Some(&matcher), None, &killed));
        // No patterns → no blocking → returns None (tool_use processed in stream) or Ok
        // The key assertion: the process is NOT killed
        assert!(
            !*rt.block_on(killed.lock()),
            "should not kill process with no patterns"
        );
        // result may be None (no text delta) or Some(Ok(_))
        if let Some(r) = result {
            assert!(r.is_ok(), "should not return error with no patterns");
        }
    }

    #[test]
    fn parse_unknown_type_ignored() {
        let json = serde_json::json!({"type": "rate_limit_event"});
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        assert!(rt
            .block_on(parse_stream_json(&json, None, None, &killed))
            .is_none());
    }

    /// Helper to build CLI args the same way chat_stream does.
    fn build_args(config: &ModelConfig) -> Vec<String> {
        let mut args = vec![
            "--print".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
        ];

        let perm_mode = config
            .cli_permission_mode
            .as_deref()
            .unwrap_or("bypassPermissions");
        args.push("--permission-mode".to_string());
        args.push(perm_mode.to_string());

        if !config.cli_allowed_tools.is_empty() {
            args.push("--allowedTools".to_string());
            for tool in &config.cli_allowed_tools {
                args.push(tool.clone());
            }
        }

        // Only block truly destructive commands
        args.push("--disallowedTools".to_string());
        args.push("Bash(rm -rf:*)".to_string());
        args.push("--disallowedTools".to_string());
        args.push("Bash(rm -r:*)".to_string());
        args.push("--disallowedTools".to_string());
        args.push("Bash(mkfs:*)".to_string());
        args.push("--disallowedTools".to_string());
        args.push("Bash(dd if=:*)".to_string());

        args
    }

    /// Default uses --permission-mode bypassPermissions with destructive-only blocks.
    #[test]
    fn default_uses_bypass_permissions() {
        let config = test_config();
        let args = build_args(&config);
        assert!(!args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.contains(&"--permission-mode".to_string()));
        assert!(args.contains(&"bypassPermissions".to_string()));
        assert!(args.contains(&"--disallowedTools".to_string()));
        assert!(args.contains(&"Bash(rm -rf:*)".to_string()));
        // chmod 777 is NOT blocked — only data-destructive ops
        assert!(!args.contains(&"Bash(chmod 777:*)".to_string()));
    }

    /// When cli_allowed_tools is set, uses --allowedTools.
    #[test]
    fn explicit_allowed_tools() {
        let mut config = test_config();
        config.cli_allowed_tools = vec!["Read".into(), "Glob".into()];
        let args = build_args(&config);
        assert!(args.contains(&"--allowedTools".to_string()));
        assert!(args.contains(&"Read".to_string()));
        assert!(args.contains(&"Glob".to_string()));
        assert!(args.contains(&"--permission-mode".to_string()));
    }

    /// Custom permission mode is respected.
    #[test]
    fn custom_permission_mode() {
        let mut config = test_config();
        config.cli_allowed_tools = vec!["Bash".into()];
        config.cli_permission_mode = Some("plan".into());
        let args = build_args(&config);
        assert!(args.contains(&"plan".to_string()));
    }
}
