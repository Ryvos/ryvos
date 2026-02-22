use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use ryvos_core::config::ModelConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::LlmClient;
use ryvos_core::types::*;

use crate::streaming::SseEvent;

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicClient {
    http: Client,
}

impl AnthropicClient {
    pub fn new() -> Self {
        Self {
            http: Client::new(),
        }
    }
}

// Anthropic API request types
#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    stream: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ApiTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
}

#[derive(Serialize)]
struct ThinkingConfig {
    r#type: String,
    budget_tokens: u32,
}

#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: serde_json::Value,
}

#[derive(Serialize)]
struct ApiTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

// Anthropic API response types
#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
#[allow(dead_code)]
enum SseData {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageInfo },
    #[serde(rename = "content_block_start")]
    ContentBlockStart { index: usize, content_block: ContentBlockInfo },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: usize, delta: DeltaInfo },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize, },
    #[serde(rename = "message_delta")]
    MessageDelta { delta: MessageDeltaInfo, usage: Option<UsageInfo> },
    #[serde(rename = "message_stop")]
    MessageStop {},
    #[serde(rename = "ping")]
    Ping {},
    #[serde(rename = "error")]
    Error { error: ApiError },
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct MessageInfo {
    id: String,
    usage: Option<UsageInfo>,
}

#[derive(Deserialize, Debug)]
struct UsageInfo {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
#[allow(dead_code)]
enum ContentBlockInfo {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum DeltaInfo {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
}

#[derive(Deserialize, Debug)]
struct MessageDeltaInfo {
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ApiError {
    message: String,
}

fn convert_messages(messages: Vec<ChatMessage>) -> (Option<String>, Vec<ApiMessage>) {
    let mut system = None;
    let mut api_msgs = Vec::new();

    for msg in messages {
        match msg.role {
            Role::System => {
                system = Some(msg.text());
            }
            Role::User => {
                let content = convert_content_blocks(&msg.content);
                api_msgs.push(ApiMessage {
                    role: "user".to_string(),
                    content,
                });
            }
            Role::Assistant => {
                let content = convert_content_blocks(&msg.content);
                api_msgs.push(ApiMessage {
                    role: "assistant".to_string(),
                    content,
                });
            }
            Role::Tool => {
                // Tool results are sent as user messages in Anthropic API
                let content = convert_content_blocks(&msg.content);
                api_msgs.push(ApiMessage {
                    role: "user".to_string(),
                    content,
                });
            }
        }
    }

    (system, api_msgs)
}

fn convert_content_blocks(blocks: &[ContentBlock]) -> serde_json::Value {
    if blocks.len() == 1 {
        if let ContentBlock::Text { text } = &blocks[0] {
            return serde_json::Value::String(text.clone());
        }
    }

    let api_blocks: Vec<serde_json::Value> = blocks
        .iter()
        .map(|b| match b {
            ContentBlock::Text { text } => serde_json::json!({
                "type": "text",
                "text": text,
            }),
            ContentBlock::ToolUse { id, name, input } => serde_json::json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input,
            }),
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content,
                "is_error": is_error,
            }),
            ContentBlock::Thinking { thinking } => serde_json::json!({
                "type": "thinking",
                "thinking": thinking,
            }),
        })
        .collect();

    serde_json::Value::Array(api_blocks)
}

fn parse_sse_to_delta(event: SseEvent) -> Option<Result<StreamDelta>> {
    if event.data.trim() == "[DONE]" {
        return None;
    }

    let parsed: std::result::Result<SseData, _> = serde_json::from_str(&event.data);
    match parsed {
        Ok(data) => match data {
            SseData::MessageStart { message } => {
                let mut deltas = vec![StreamDelta::MessageId(message.id)];
                if let Some(usage) = message.usage {
                    deltas.push(StreamDelta::Usage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                    });
                }
                // Return just the message ID; usage tracked separately
                Some(Ok(deltas.remove(0)))
            }
            SseData::ContentBlockStart {
                index,
                content_block,
            } => match content_block {
                ContentBlockInfo::Text { .. } => None,
                ContentBlockInfo::ToolUse { id, name } => {
                    Some(Ok(StreamDelta::ToolUseStart { index, id, name }))
                }
                ContentBlockInfo::Thinking { .. } => None,
            },
            SseData::ContentBlockDelta { index, delta } => match delta {
                DeltaInfo::TextDelta { text } => Some(Ok(StreamDelta::TextDelta(text))),
                DeltaInfo::InputJsonDelta { partial_json } => {
                    Some(Ok(StreamDelta::ToolInputDelta {
                        index,
                        delta: partial_json,
                    }))
                }
                DeltaInfo::ThinkingDelta { thinking } => {
                    Some(Ok(StreamDelta::ThinkingDelta(thinking)))
                }
            },
            SseData::ContentBlockStop { .. } => None,
            SseData::MessageDelta { delta, usage } => {
                let stop = match delta.stop_reason.as_deref() {
                    Some("end_turn") => Some(StopReason::EndTurn),
                    Some("tool_use") => Some(StopReason::ToolUse),
                    Some("max_tokens") => Some(StopReason::MaxTokens),
                    Some("stop_sequence") => Some(StopReason::StopSequence),
                    _ => None,
                };
                if let Some(usage) = usage {
                    // We have usage info — emit stop with it
                    debug!(
                        input_tokens = usage.input_tokens,
                        output_tokens = usage.output_tokens,
                        "Token usage"
                    );
                }
                stop.map(|s| Ok(StreamDelta::Stop(s)))
            }
            SseData::MessageStop {} => None,
            SseData::Ping {} => None,
            SseData::Error { error } => {
                Some(Err(RyvosError::LlmStream(error.message)))
            }
        },
        Err(e) => {
            warn!(data = %event.data, error = %e, "Failed to parse SSE data");
            None
        }
    }
}

impl LlmClient for AnthropicClient {
    fn chat_stream(
        &self,
        config: &ModelConfig,
        messages: Vec<ChatMessage>,
        tools: &[ToolDefinition],
    ) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>> {
        let config = config.clone();
        let tools = tools.to_vec();

        Box::pin(async move {
            let api_key = config
                .api_key
                .as_deref()
                .ok_or_else(|| RyvosError::Config("Anthropic API key not set".into()))?;

            let base_url = config
                .base_url
                .as_deref()
                .unwrap_or(ANTHROPIC_API_URL);

            let (system, api_messages) = convert_messages(messages);

            let api_tools: Vec<ApiTool> = tools
                .iter()
                .map(|t| ApiTool {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: t.input_schema.clone(),
                })
                .collect();

            // Build thinking config if enabled
            let thinking = if config.thinking != ryvos_core::types::ThinkingLevel::Off {
                Some(ThinkingConfig {
                    r#type: "enabled".to_string(),
                    budget_tokens: config.thinking.budget_tokens(),
                })
            } else {
                None
            };

            let body = AnthropicRequest {
                model: config.model_id.clone(),
                max_tokens: config.max_tokens,
                // Must NOT send temperature when thinking is enabled (Anthropic constraint)
                temperature: if thinking.is_some() {
                    None
                } else if config.temperature > 0.0 {
                    Some(config.temperature)
                } else {
                    None
                },
                messages: api_messages,
                system,
                stream: true,
                tools: api_tools,
                thinking,
            };

            let response = self
                .http
                .post(base_url)
                .header("x-api-key", api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| RyvosError::LlmRequest(e.to_string()))?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown error".to_string());
                return Err(RyvosError::LlmRequest(format!(
                    "HTTP {}: {}",
                    status, body
                )));
            }

            let byte_stream = response.bytes_stream();
            let sse_stream = crate::streaming::SseStream::new(byte_stream);

            let delta_stream = sse_stream.filter_map(|event| async move {
                parse_sse_to_delta(event)
            });

            Ok(Box::pin(delta_stream) as BoxStream<'_, Result<StreamDelta>>)
        })
    }
}

impl Default for AnthropicClient {
    fn default() -> Self {
        Self::new()
    }
}
