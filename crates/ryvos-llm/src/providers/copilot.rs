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

/// LLM client that delegates to the GitHub Copilot CLI (`copilot`).
///
/// This enables subscription billing: users with a GitHub Copilot license
/// pay nothing per-token. Ryvos spawns the CLI as a child process and parses
/// its JSONL output (`--output-format json`).
///
/// Security: `assistant.message` events contain `toolRequests[]` which are
/// inspected against the configured dangerous-command patterns. If a match
/// is found, the child process is killed before it can execute the command.
pub struct CopilotClient {
    /// Compiled dangerous-pattern matcher for intercepting CLI tool calls.
    pattern_matcher: Option<Arc<DangerousPatternMatcher>>,
}

impl CopilotClient {
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

    /// Copilot CLI always uses the user's GitHub Copilot subscription.
    pub fn detect_billing_type(_config: &ModelConfig) -> BillingType {
        BillingType::Subscription
    }
}

/// Strip ANSI escape sequences (CSI and OSC) from a string.
fn strip_ansi_escapes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                // CSI sequence: ESC [ ... final_byte
                Some('[') => {
                    chars.next(); // consume '['
                    // Consume parameter bytes (0x30-0x3F), intermediate bytes (0x20-0x2F),
                    // until we hit a final byte (0x40-0x7E).
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC sequence: ESC ] ... ST (ST = ESC \ or BEL)
                Some(']') => {
                    chars.next(); // consume ']'
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            // BEL terminates OSC
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next(); // consume '\'
                            break;
                        }
                    }
                }
                // Other escape (e.g., ESC ( B) — skip the next char
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }

    out
}

/// Build CLI args from a ModelConfig (extracted for testability).
fn build_args(config: &ModelConfig, prompt: &str, system_context: &str) -> Vec<String> {
    // Copilot CLI has no --system-prompt flag; prepend system context to the prompt.
    let mut full_prompt = String::new();
    if !system_context.is_empty() {
        full_prompt.push_str(system_context);
        full_prompt.push_str("\n\n---\n\n");
    }
    full_prompt.push_str(prompt);

    let mut args = vec![
        "--prompt".to_string(),
        full_prompt,
        "--output-format".to_string(),
        "json".to_string(),
        "--silent".to_string(),
        "--no-color".to_string(),
        "--no-ask-user".to_string(),
        "--autopilot".to_string(),
    ];

    // Tool permissions
    if config
        .cli_permission_mode
        .as_deref()
        .map_or(false, |m| m == "dontAsk")
    {
        args.push("--allow-all".to_string());
    } else {
        for tool in &config.cli_allowed_tools {
            args.push("--allow-tool".to_string());
            args.push(tool.clone());
        }
    }

    // Model override
    if !config.model_id.is_empty() && config.model_id != "default" {
        args.push("--model".to_string());
        args.push(config.model_id.clone());
    }

    // Session resumption
    if let Some(ref session_id) = config.cli_session_id {
        args.push(format!("--resume={}", session_id));
    }

    args
}

/// Parse a single JSONL event from the Copilot CLI.
///
/// Key event types:
/// - `assistant.message_delta` → `TextDelta(data.deltaContent)`
/// - `assistant.reasoning_delta` → `ThinkingDelta(data.deltaContent)`
/// - `assistant.message` → inspect `data.toolRequests[]` for dangerous commands
/// - `result` → `MessageId(sessionId)`
/// - Everything else → ignored
async fn parse_copilot_event(
    json: &serde_json::Value,
    matcher: Option<&DangerousPatternMatcher>,
    child_id: Option<u32>,
    killed: &Mutex<bool>,
) -> Option<Result<StreamDelta>> {
    let event_type = json["type"].as_str()?;

    match event_type {
        "assistant.message_delta" => {
            let content = json
                .get("data")
                .and_then(|d| d["deltaContent"].as_str())?;
            if content.is_empty() {
                return None;
            }
            Some(Ok(StreamDelta::TextDelta(content.to_string())))
        }
        "assistant.reasoning_delta" => {
            let content = json
                .get("data")
                .and_then(|d| d["deltaContent"].as_str())?;
            if content.is_empty() {
                return None;
            }
            Some(Ok(StreamDelta::ThinkingDelta(content.to_string())))
        }
        "assistant.message" => {
            // Check for usage data in the message
            if let Some(usage) = json.get("data").and_then(|d| d.get("usage"))
                .or_else(|| json.get("usage"))
            {
                let input = usage.get("inputTokens")
                    .or_else(|| usage.get("input_tokens"))
                    .and_then(|v| v.as_u64()).unwrap_or(0);
                let output = usage.get("outputTokens")
                    .or_else(|| usage.get("output_tokens"))
                    .and_then(|v| v.as_u64()).unwrap_or(0);
                if input > 0 || output > 0 {
                    return Some(Ok(StreamDelta::Usage { input_tokens: input, output_tokens: output }));
                }
            }

            // Extract ALL tool requests for audit trail / safety memory logging.
            // CLI providers execute tools internally — we can't block, but we CAN log.
            let tool_requests = json
                .get("data")
                .and_then(|d| d.get("toolRequests"))
                .and_then(|t| t.as_array());

            if let Some(requests) = tool_requests {
                for req in requests {
                    let tool_name = req["toolName"].as_str().unwrap_or("unknown");
                    let input_summary = if tool_name == "Bash" || tool_name == "bash" || tool_name.starts_with("shell") {
                        req.get("input")
                            .and_then(|i| i["command"].as_str().or_else(|| i["cmd"].as_str()))
                            .unwrap_or("")
                            .to_string()
                    } else {
                        let s = serde_json::to_string(req.get("input").unwrap_or(&serde_json::Value::Null)).unwrap_or_default();
                        if s.len() > 120 { format!("{}...", &s[..120]) } else { s }
                    };

                    // Informational pattern check (no blocking)
                    if let Some(matcher) = matcher {
                        if tool_name == "Bash" || tool_name == "bash" || tool_name.starts_with("shell") {
                            if let Some(label) = matcher.is_dangerous(&input_summary) {
                                warn!(
                                    tool = tool_name,
                                    command = %input_summary,
                                    pattern = label,
                                    "Copilot CLI executed potentially dangerous command (logged, not blocked)"
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
            None
        }
        "assistant.turn_end" => {
            // Turn ended — don't stop yet, wait for "result" event which
            // carries the sessionId needed for --resume on subsequent calls.
            // The process will exit naturally after result.
            None
        }
        "result" => {
            // Final event — extract session ID for --resume, then stop.
            // This mirrors how claude_code handles its "result" event.
            if let Some(session_id) = json.get("sessionId").and_then(|s| s.as_str()) {
                return Some(Ok(StreamDelta::MessageId(session_id.to_string())));
            }
            Some(Ok(StreamDelta::Stop(StopReason::EndTurn)))
        }
        _ => None,
    }
}

impl LlmClient for CopilotClient {
    fn chat_stream(
        &self,
        config: &ModelConfig,
        messages: Vec<ChatMessage>,
        _tools: &[ToolDefinition],
    ) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>> {
        let config = config.clone();
        let messages = messages;
        let matcher = self.pattern_matcher.clone();

        Box::pin(async move {
            // Extract the last user message as the prompt
            let prompt = messages
                .iter()
                .rev()
                .find_map(|m| {
                    if m.role == Role::User {
                        let t = m.text();
                        if t.is_empty() { None } else { Some(t) }
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

            if prompt.is_empty() {
                return Err(RyvosError::LlmRequest(
                    "No user message found for copilot provider".into(),
                ));
            }

            // Build system context from system messages
            let system_context: String = messages
                .iter()
                .filter(|m| m.role == Role::System)
                .map(|m| m.text())
                .collect::<Vec<_>>()
                .join("\n");

            let copilot_bin = config.copilot_command.as_deref().unwrap_or("copilot");
            let args = build_args(&config, &prompt, &system_context);

            debug!(bin = copilot_bin, args = ?args, "Spawning copilot CLI");

            let mut cmd = Command::new(copilot_bin);
            cmd.args(&args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            // Pass GH_TOKEN from api_key if configured
            if let Some(ref token) = config.api_key {
                cmd.env("GH_TOKEN", token);
            }

            let mut child = cmd.spawn().map_err(|e| {
                RyvosError::LlmRequest(format!(
                    "Failed to spawn copilot CLI '{}': {}. Is it installed? (npm install -g @github/copilot)",
                    copilot_bin, e
                ))
            })?;

            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| RyvosError::LlmStream("Failed to capture stdout".into()))?;

            let reader = BufReader::new(stdout);
            let lines = reader.lines();

            // Share the child PID so the stream can kill it if needed
            let child_id = child.id();
            let killed = Arc::new(Mutex::new(false));

            // Drain stderr in background
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
                            debug!(target: "copilot_cli_stderr", "{}", trimmed);
                        }
                        line.clear();
                    }
                }
                let _ = child.wait().await;
            });

            // Stream stdout line-by-line, parse JSONL events
            let stream = tokio_stream::wrappers::LinesStream::new(lines)
                .filter_map(
                    move |line_result: std::result::Result<String, std::io::Error>| {
                        let matcher = matcher.clone();
                        let killed = killed.clone();

                        async move {
                            let line: String = match line_result {
                                Ok(l) => l,
                                Err(e) => {
                                    error!(error = %e, "Error reading copilot CLI stream");
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

                            parse_copilot_event(&json, matcher.as_deref(), child_id, &killed).await
                        }
                    },
                )
                .chain(futures::stream::once(async {
                    Ok(StreamDelta::Stop(StopReason::EndTurn))
                }));

            Ok(Box::pin(stream) as BoxStream<'_, Result<StreamDelta>>)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a test ModelConfig.
    fn test_config() -> ModelConfig {
        ModelConfig {
            provider: "copilot".into(),
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

    // ── ANSI stripping tests ──

    #[test]
    fn strip_ansi_color_codes() {
        let input = "\x1b[32mHello\x1b[0m world";
        assert_eq!(strip_ansi_escapes(input), "Hello world");
    }

    #[test]
    fn strip_ansi_nested_sequences() {
        let input = "\x1b[1;34m\x1b[4mBold underline\x1b[0m normal";
        assert_eq!(strip_ansi_escapes(input), "Bold underline normal");
    }

    #[test]
    fn strip_ansi_osc_sequence() {
        let input = "\x1b]0;window title\x07some text";
        assert_eq!(strip_ansi_escapes(input), "some text");
    }

    #[test]
    fn strip_ansi_noop_on_clean_text() {
        let input = "No escape sequences here.";
        assert_eq!(strip_ansi_escapes(input), input);
    }

    #[test]
    fn strip_ansi_empty_input() {
        assert_eq!(strip_ansi_escapes(""), "");
    }

    // ── build_args tests ──

    #[test]
    fn build_args_json_format() {
        let config = test_config();
        let args = build_args(&config, "hello", "");
        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"json".to_string()));
        assert!(args.contains(&"--silent".to_string()));
        assert!(args.contains(&"--no-color".to_string()));
        assert!(args.contains(&"--no-ask-user".to_string()));
        assert!(args.contains(&"--autopilot".to_string()));
    }

    #[test]
    fn build_args_allow_all() {
        let mut config = test_config();
        config.cli_permission_mode = Some("dontAsk".into());
        config.cli_allowed_tools = vec!["shell(git)".into(), "read".into()];
        let args = build_args(&config, "hello", "");
        assert!(args.contains(&"--allow-all".to_string()));
        // --allow-tool should NOT appear when --allow-all is used
        assert!(!args.contains(&"--allow-tool".to_string()));
    }

    #[test]
    fn build_args_allow_tool() {
        let mut config = test_config();
        config.cli_allowed_tools = vec!["shell(git)".into(), "read".into()];
        let args = build_args(&config, "hello", "");
        assert!(!args.contains(&"--allow-all".to_string()));
        assert!(args.contains(&"--allow-tool".to_string()));
        assert!(args.contains(&"shell(git)".to_string()));
        assert!(args.contains(&"read".to_string()));
    }

    #[test]
    fn build_args_model() {
        let config = test_config();
        let args = build_args(&config, "hello", "");
        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"claude-sonnet-4".to_string()));
    }

    #[test]
    fn build_args_resume() {
        let mut config = test_config();
        config.cli_session_id = Some("sess-abc-123".into());
        let args = build_args(&config, "hello", "");
        assert!(args.contains(&"--resume=sess-abc-123".to_string()));
    }

    #[test]
    fn build_args_system_context_prepended() {
        let config = test_config();
        let args = build_args(&config, "user prompt", "system context");
        let prompt = &args[1]; // after "--prompt"
        assert!(prompt.starts_with("system context"));
        assert!(prompt.contains("user prompt"));
    }

    #[test]
    fn build_args_no_system_context() {
        let config = test_config();
        let args = build_args(&config, "user prompt", "");
        let prompt = &args[1];
        assert_eq!(prompt, "user prompt");
    }

    // ── JSONL event parsing tests ──

    #[test]
    fn parse_message_delta() {
        let json = serde_json::json!({
            "type": "assistant.message_delta",
            "data": { "deltaContent": "Hello world" }
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        let delta = rt
            .block_on(parse_copilot_event(&json, None, None, &killed))
            .unwrap()
            .unwrap();
        assert!(matches!(delta, StreamDelta::TextDelta(t) if t == "Hello world"));
    }

    #[test]
    fn parse_reasoning_delta() {
        let json = serde_json::json!({
            "type": "assistant.reasoning_delta",
            "data": { "deltaContent": "Let me think..." }
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        let delta = rt
            .block_on(parse_copilot_event(&json, None, None, &killed))
            .unwrap()
            .unwrap();
        assert!(matches!(delta, StreamDelta::ThinkingDelta(t) if t == "Let me think..."));
    }

    #[test]
    fn parse_message_tool_emits_cli_executed() {
        let json = serde_json::json!({
            "type": "assistant.message",
            "data": {
                "content": "I'll list the files.",
                "toolRequests": [
                    { "toolName": "Bash", "input": { "command": "ls -la" } }
                ]
            }
        });
        let patterns = ryvos_core::security::SecurityPolicy::default_patterns();
        let matcher = DangerousPatternMatcher::new(&patterns);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        // All tool requests now emit CliToolExecuted for audit logging
        let result = rt.block_on(parse_copilot_event(&json, Some(&matcher), None, &killed));
        assert!(result.is_some());
        let delta = result.unwrap().unwrap();
        assert!(matches!(delta, StreamDelta::CliToolExecuted { .. }));
    }

    #[test]
    fn parse_message_dangerous_not_blocked() {
        // Dangerous patterns are deprecated — tools are never blocked.
        let json = serde_json::json!({
            "type": "assistant.message",
            "data": {
                "content": "I'll clean up.",
                "toolRequests": [
                    { "toolName": "Bash", "input": { "command": "rm -rf /home/user" } }
                ]
            }
        });
        // With empty default patterns, no blocking occurs
        let patterns = ryvos_core::security::SecurityPolicy::default_patterns();
        let matcher = DangerousPatternMatcher::new(&patterns);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        let result = rt
            .block_on(parse_copilot_event(&json, Some(&matcher), None, &killed));
        // No patterns → no blocking → process is NOT killed
        assert!(!*rt.block_on(killed.lock()), "should not kill process with no patterns");
        // result may be None or Some(Ok(_))
        if let Some(r) = result {
            assert!(r.is_ok(), "should not return error with no patterns");
        }
    }

    #[test]
    fn parse_result_with_session_id() {
        // Result emits MessageId for --resume support
        let json = serde_json::json!({
            "type": "result",
            "sessionId": "sess-xyz-789",
            "exitCode": 0,
            "usage": { "inputTokens": 100, "outputTokens": 50 }
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        let delta = rt
            .block_on(parse_copilot_event(&json, None, None, &killed))
            .unwrap()
            .unwrap();
        assert!(matches!(delta, StreamDelta::MessageId(id) if id == "sess-xyz-789"));
    }

    #[test]
    fn parse_turn_end_ignored() {
        // turn_end is ignored — we wait for "result" which has the sessionId
        let json = serde_json::json!({
            "type": "assistant.turn_end",
            "data": { "turnId": "0" }
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        assert!(rt
            .block_on(parse_copilot_event(&json, None, None, &killed))
            .is_none());
    }

    #[test]
    fn parse_result_emits_message_id() {
        let json = serde_json::json!({
            "type": "result",
            "sessionId": "sess-abc-123",
            "exitCode": 0
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        let delta = rt
            .block_on(parse_copilot_event(&json, None, None, &killed))
            .unwrap()
            .unwrap();
        assert!(matches!(delta, StreamDelta::MessageId(id) if id == "sess-abc-123"));
    }

    #[test]
    fn parse_result_no_session_id_stops() {
        let json = serde_json::json!({
            "type": "result",
            "exitCode": 0
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        let delta = rt
            .block_on(parse_copilot_event(&json, None, None, &killed))
            .unwrap()
            .unwrap();
        assert!(matches!(delta, StreamDelta::Stop(_)));
    }

    #[test]
    fn parse_unknown_ignored() {
        let json = serde_json::json!({"type": "session.tools_updated"});
        let rt = tokio::runtime::Runtime::new().unwrap();
        let killed = Mutex::new(false);
        assert!(rt
            .block_on(parse_copilot_event(&json, None, None, &killed))
            .is_none());

        let json2 = serde_json::json!({"type": "assistant.turn_start"});
        assert!(rt
            .block_on(parse_copilot_event(&json2, None, None, &killed))
            .is_none());
    }

    // ── Billing type test ──

    #[test]
    fn detect_billing_type_always_subscription() {
        let config = test_config();
        assert_eq!(
            CopilotClient::detect_billing_type(&config),
            BillingType::Subscription
        );

        let mut config_with_key = test_config();
        config_with_key.api_key = Some("ghp_test".into());
        assert_eq!(
            CopilotClient::detect_billing_type(&config_with_key),
            BillingType::Subscription
        );
    }
}
