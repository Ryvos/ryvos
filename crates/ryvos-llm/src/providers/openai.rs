use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::warn;

use ryvos_core::config::ModelConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::LlmClient;
use ryvos_core::types::*;

use crate::streaming::{SseEvent, SseStream};

const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";

/// OpenAI-compatible client. Works with OpenAI, Ollama, vLLM, Groq, OpenRouter, etc.
pub struct OpenAiClient {
    http: Client,
}

impl OpenAiClient {
    pub fn new() -> Self {
        Self {
            http: Client::new(),
        }
    }
}

impl Default for OpenAiClient {
    fn default() -> Self {
        Self::new()
    }
}

// Request types
#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<OaiMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OaiTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct OaiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OaiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct OaiToolCall {
    #[serde(default)]
    index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function: Option<OaiFunction>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct OaiFunction {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct OaiTool {
    r#type: String,
    function: OaiToolDef,
}

#[derive(Serialize)]
pub(crate) struct OaiToolDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

// Response types
#[derive(Deserialize, Debug)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<StreamUsage>,
}

#[derive(Deserialize, Debug)]
struct StreamChoice {
    delta: StreamDeltaContent,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct StreamDeltaContent {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OaiToolCall>>,
}

#[derive(Deserialize, Debug)]
struct StreamUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

pub(crate) fn convert_tools(tools: &[ToolDefinition]) -> Vec<OaiTool> {
    tools
        .iter()
        .map(|t| OaiTool {
            r#type: "function".to_string(),
            function: OaiToolDef {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.input_schema.clone(),
            },
        })
        .collect()
}

pub(crate) fn convert_messages(messages: Vec<ChatMessage>) -> Vec<OaiMessage> {
    let mut oai_msgs = Vec::new();

    for msg in messages {
        match msg.role {
            Role::System => {
                oai_msgs.push(OaiMessage {
                    role: "system".to_string(),
                    content: Some(serde_json::Value::String(msg.text())),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            Role::User => {
                // Check if this contains tool results
                let tool_results: Vec<_> = msg
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } => Some((tool_use_id.clone(), content.clone())),
                        _ => None,
                    })
                    .collect();

                if !tool_results.is_empty() {
                    for (id, content) in tool_results {
                        oai_msgs.push(OaiMessage {
                            role: "tool".to_string(),
                            content: Some(serde_json::Value::String(content)),
                            tool_calls: None,
                            tool_call_id: Some(id),
                        });
                    }
                } else {
                    oai_msgs.push(OaiMessage {
                        role: "user".to_string(),
                        content: Some(serde_json::Value::String(msg.text())),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }
            }
            Role::Assistant => {
                let tool_uses = msg.tool_uses();
                if tool_uses.is_empty() {
                    oai_msgs.push(OaiMessage {
                        role: "assistant".to_string(),
                        content: Some(serde_json::Value::String(msg.text())),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                } else {
                    let text = msg.text();
                    let calls: Vec<OaiToolCall> = tool_uses
                        .iter()
                        .enumerate()
                        .map(|(i, (id, name, input))| OaiToolCall {
                            index: i,
                            id: Some(id.to_string()),
                            r#type: Some("function".to_string()),
                            function: Some(OaiFunction {
                                name: Some(name.to_string()),
                                arguments: Some(input.to_string()),
                            }),
                        })
                        .collect();

                    oai_msgs.push(OaiMessage {
                        role: "assistant".to_string(),
                        content: if text.is_empty() {
                            None
                        } else {
                            Some(serde_json::Value::String(text))
                        },
                        tool_calls: Some(calls),
                        tool_call_id: None,
                    });
                }
            }
            Role::Tool => {
                // Already handled via User role with ToolResult blocks
            }
        }
    }

    oai_msgs
}

pub(crate) fn parse_chunk(event: SseEvent) -> Vec<Result<StreamDelta>> {
    if event.data.trim() == "[DONE]" {
        return vec![];
    }

    let parsed: std::result::Result<StreamChunk, _> = serde_json::from_str(&event.data);
    match parsed {
        Ok(chunk) => {
            let mut deltas = Vec::new();

            if let Some(usage) = chunk.usage {
                deltas.push(Ok(StreamDelta::Usage {
                    input_tokens: usage.prompt_tokens,
                    output_tokens: usage.completion_tokens,
                }));
                return deltas;
            }

            let choice = match chunk.choices.into_iter().next() {
                Some(c) => c,
                None => return deltas,
            };

            // Check finish reason
            if let Some(reason) = choice.finish_reason {
                let stop = match reason.as_str() {
                    "stop" => StopReason::EndTurn,
                    "tool_calls" => StopReason::ToolUse,
                    "length" => StopReason::MaxTokens,
                    _ => StopReason::EndTurn,
                };
                deltas.push(Ok(StreamDelta::Stop(stop)));
                return deltas;
            }

            // Check for text delta
            if let Some(text) = choice.delta.content {
                if !text.is_empty() {
                    deltas.push(Ok(StreamDelta::TextDelta(text)));
                }
            }

            // Check for tool calls — emit both ToolUseStart and ToolInputDelta
            // when a provider sends name + arguments in the same SSE chunk
            // (common with Groq, Together, and other non-OpenAI providers).
            if let Some(tool_calls) = choice.delta.tool_calls {
                for tc in tool_calls {
                    if let Some(func) = tc.function {
                        if let Some(name) = func.name {
                            deltas.push(Ok(StreamDelta::ToolUseStart {
                                index: tc.index,
                                id: tc.id.unwrap_or_default(),
                                name,
                            }));
                        }
                        if let Some(args) = func.arguments {
                            deltas.push(Ok(StreamDelta::ToolInputDelta {
                                index: tc.index,
                                delta: args,
                            }));
                        }
                    }
                }
            }

            deltas
        }
        Err(e) => {
            warn!(data = %event.data, error = %e, "Failed to parse OpenAI SSE chunk");
            vec![]
        }
    }
}

impl LlmClient for OpenAiClient {
    fn chat_stream(
        &self,
        config: &ModelConfig,
        messages: Vec<ChatMessage>,
        tools: &[ToolDefinition],
    ) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>> {
        let config = config.clone();
        let tools = tools.to_vec();

        Box::pin(async move {
            let base_url = config.base_url.as_deref().unwrap_or(OPENAI_API_URL);

            let oai_messages = convert_messages(messages);
            let oai_tools: Vec<OaiTool> = tools
                .iter()
                .map(|t| OaiTool {
                    r#type: "function".to_string(),
                    function: OaiToolDef {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.input_schema.clone(),
                    },
                })
                .collect();

            // For o-series models, send reasoning_effort instead of temperature
            let is_o_series = config.model_id.starts_with("o1")
                || config.model_id.starts_with("o3")
                || config.model_id.starts_with("o4");

            let reasoning_effort =
                if is_o_series && config.thinking != ryvos_core::types::ThinkingLevel::Off {
                    Some(config.thinking.reasoning_effort().to_string())
                } else {
                    None
                };

            let body = ChatRequest {
                model: config.model_id.clone(),
                messages: oai_messages,
                max_tokens: config.max_tokens,
                temperature: if is_o_series {
                    None // o-series doesn't support temperature
                } else if config.temperature > 0.0 {
                    Some(config.temperature)
                } else {
                    None
                },
                stream: true,
                tools: oai_tools,
                reasoning_effort,
            };

            let mut req = self.http.post(base_url).json(&body);

            if let Some(api_key) = &config.api_key {
                req = req.header("Authorization", format!("Bearer {}", api_key));
            }

            // Apply extra headers from config (set by presets or user)
            for (k, v) in &config.extra_headers {
                req = req.header(k.as_str(), v.as_str());
            }

            let response = req
                .send()
                .await
                .map_err(|e| RyvosError::LlmRequest(e.to_string()))?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown".to_string());
                return Err(RyvosError::LlmRequest(format!("HTTP {}: {}", status, body)));
            }

            let byte_stream = response.bytes_stream();
            let sse_stream = SseStream::new(byte_stream);

            let delta_stream = sse_stream
                .map(|event| futures::stream::iter(parse_chunk(event)))
                .flatten();

            Ok(Box::pin(delta_stream) as BoxStream<'_, Result<StreamDelta>>)
        })
    }
}
