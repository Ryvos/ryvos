use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{debug, error};

use ryvos_core::config::ModelConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::LlmClient;
use ryvos_core::types::*;

/// LLM client that delegates to the `claude` CLI (Claude Code).
///
/// This enables subscription billing: users with a Claude Max/Pro subscription
/// pay nothing per-token. Ryvos spawns the CLI as a child process and parses
/// its stream-json output.
pub struct ClaudeCodeClient;

impl ClaudeCodeClient {
    pub fn new() -> Self {
        Self
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
        let messages = messages;

        Box::pin(async move {
            // Extract the last user message as the prompt
            let prompt = messages
                .iter()
                .rev()
                .find_map(|m| {
                    if m.role == Role::User {
                        Some(m.text())
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

            let claude_bin = config
                .claude_command
                .as_deref()
                .unwrap_or("claude");

            let mut args = vec![
                "--print".to_string(),
                "-".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--verbose".to_string(),
                "--dangerously-skip-permissions".to_string(),
            ];

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

            // Spawn a task to wait for the child process and log stderr
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

            let stream = tokio_stream::wrappers::LinesStream::new(lines)
                .filter_map(|line_result: std::result::Result<String, std::io::Error>| async move {
                    let line: String = match line_result {
                        Ok(l) => l,
                        Err(e) => {
                            error!(error = %e, "Error reading claude CLI stdout");
                            return Some(Err(RyvosError::LlmStream(e.to_string())));
                        }
                    };

                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        return None;
                    }

                    let json: serde_json::Value = match serde_json::from_str(trimmed) {
                        Ok(v) => v,
                        Err(_) => {
                            // Not all lines are JSON (progress indicators, etc.)
                            return None;
                        }
                    };

                    parse_stream_json(&json)
                });

            Ok(Box::pin(stream) as BoxStream<'_, Result<StreamDelta>>)
        })
    }
}

/// Parse a single stream-json line from the claude CLI.
fn parse_stream_json(json: &serde_json::Value) -> Option<Result<StreamDelta>> {
    let msg_type = json["type"].as_str()?;

    match msg_type {
        "system" => {
            // System init message contains the session ID
            if json.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                if let Some(session_id) = json["session_id"].as_str() {
                    return Some(Ok(StreamDelta::MessageId(session_id.to_string())));
                }
            }
            None
        }
        "assistant" => {
            // Assistant message with content blocks
            let content = json.get("content")?;
            let blocks = content.as_array()?;

            // We return deltas for each block — caller accumulates
            for block in blocks {
                let block_type = block["type"].as_str().unwrap_or("");
                match block_type {
                    "text" => {
                        if let Some(text) = block["text"].as_str() {
                            if !text.is_empty() {
                                return Some(Ok(StreamDelta::TextDelta(text.to_string())));
                            }
                        }
                    }
                    "tool_use" => {
                        let id = block["id"].as_str().unwrap_or("").to_string();
                        let name = block["name"].as_str().unwrap_or("").to_string();
                        let _input = block.get("input").cloned().unwrap_or(serde_json::Value::Null);
                        // Emit ToolUseStart — input arrives via content_block_delta events
                        return Some(Ok(StreamDelta::ToolUseStart {
                            index: 0,
                            id,
                            name,
                        }));
                    }
                    "thinking" => {
                        if let Some(thinking) = block["thinking"].as_str() {
                            if !thinking.is_empty() {
                                return Some(Ok(StreamDelta::ThinkingDelta(
                                    thinking.to_string(),
                                )));
                            }
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        "result" => {
            // Final result with usage stats
            let mut deltas = vec![];

            if let Some(usage) = json.get("usage") {
                let input = usage["input_tokens"].as_u64().unwrap_or(0);
                let output = usage["output_tokens"].as_u64().unwrap_or(0);
                if input > 0 || output > 0 {
                    deltas.push(StreamDelta::Usage {
                        input_tokens: input,
                        output_tokens: output,
                    });
                }
            }

            // Also extract session_id from result if present
            if let Some(session_id) = json["session_id"].as_str() {
                return Some(Ok(StreamDelta::MessageId(session_id.to_string())));
            }

            // Return Stop + Usage (Stop takes precedence as the final signal)
            let stop_reason = json["stop_reason"]
                .as_str()
                .map(|s| match s {
                    "end_turn" => StopReason::EndTurn,
                    "tool_use" => StopReason::ToolUse,
                    "max_tokens" => StopReason::MaxTokens,
                    _ => StopReason::EndTurn,
                })
                .unwrap_or(StopReason::EndTurn);

            Some(Ok(StreamDelta::Stop(stop_reason)))
        }
        "content_block_delta" => {
            // Streaming content delta
            if let Some(delta) = json.get("delta") {
                let delta_type = delta["type"].as_str().unwrap_or("");
                match delta_type {
                    "text_delta" => {
                        if let Some(text) = delta["text"].as_str() {
                            return Some(Ok(StreamDelta::TextDelta(text.to_string())));
                        }
                    }
                    "thinking_delta" => {
                        if let Some(thinking) = delta["thinking"].as_str() {
                            return Some(Ok(StreamDelta::ThinkingDelta(thinking.to_string())));
                        }
                    }
                    "input_json_delta" => {
                        if let Some(partial) = delta["partial_json"].as_str() {
                            let index = json["index"].as_u64().unwrap_or(0) as usize;
                            return Some(Ok(StreamDelta::ToolInputDelta {
                                index,
                                delta: partial.to_string(),
                            }));
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        "content_block_start" => {
            if let Some(block) = json.get("content_block") {
                if block["type"].as_str() == Some("tool_use") {
                    let id = block["id"].as_str().unwrap_or("").to_string();
                    let name = block["name"].as_str().unwrap_or("").to_string();
                    let index = json["index"].as_u64().unwrap_or(0) as usize;
                    return Some(Ok(StreamDelta::ToolUseStart { index, id, name }));
                }
            }
            None
        }
        "message_start" => {
            // Extract session/message ID
            if let Some(msg) = json.get("message") {
                if let Some(id) = msg["id"].as_str() {
                    return Some(Ok(StreamDelta::MessageId(id.to_string())));
                }
            }
            None
        }
        "message_delta" => {
            // Usage in message_delta
            if let Some(usage) = json.get("usage") {
                let input = usage["input_tokens"].as_u64().unwrap_or(0);
                let output = usage["output_tokens"].as_u64().unwrap_or(0);
                if input > 0 || output > 0 {
                    return Some(Ok(StreamDelta::Usage {
                        input_tokens: input,
                        output_tokens: output,
                    }));
                }
            }
            if let Some(delta) = json.get("delta") {
                if let Some(stop_reason) = delta["stop_reason"].as_str() {
                    let reason = match stop_reason {
                        "end_turn" => StopReason::EndTurn,
                        "tool_use" => StopReason::ToolUse,
                        "max_tokens" => StopReason::MaxTokens,
                        _ => StopReason::EndTurn,
                    };
                    return Some(Ok(StreamDelta::Stop(reason)));
                }
            }
            None
        }
        _ => {
            // Unknown type — ignore
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_billing_type_api() {
        let config = ModelConfig {
            provider: "claude-code".into(),
            model_id: "claude-sonnet-4".into(),
            api_key: Some("sk-test".into()),
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
            cli_session_id: None,
        };
        assert_eq!(
            ClaudeCodeClient::detect_billing_type(&config),
            BillingType::Api
        );
    }

    #[test]
    fn detect_billing_type_subscription() {
        let config = ModelConfig {
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
            cli_session_id: None,
        };
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
        let delta = parse_stream_json(&json).unwrap().unwrap();
        assert!(matches!(delta, StreamDelta::MessageId(id) if id == "abc-123"));
    }

    #[test]
    fn parse_text_delta() {
        let json = serde_json::json!({
            "type": "assistant",
            "content": [{"type": "text", "text": "Hello world"}]
        });
        let delta = parse_stream_json(&json).unwrap().unwrap();
        assert!(matches!(delta, StreamDelta::TextDelta(t) if t == "Hello world"));
    }

    #[test]
    fn parse_result_stop() {
        let json = serde_json::json!({
            "type": "result",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        });
        let delta = parse_stream_json(&json).unwrap().unwrap();
        assert!(matches!(delta, StreamDelta::Stop(StopReason::EndTurn)));
    }
}
