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

/// Cohere v2 Chat API client.
pub struct CohereClient {
    http: Client,
}

impl CohereClient {
    pub fn new() -> Self {
        Self {
            http: Client::new(),
        }
    }
}

impl Default for CohereClient {
    fn default() -> Self {
        Self::new()
    }
}

const COHERE_API_URL: &str = "https://api.cohere.com/v2/chat";

// ── Request types ────────────────────────────────────────────────

#[derive(Serialize)]
struct CohereRequest {
    model: String,
    messages: Vec<CohereMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<CohereTool>,
}

#[derive(Serialize)]
struct CohereMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<CohereToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_results: Option<Vec<CohereToolResult>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CohereToolCall {
    id: String,
    r#type: String,
    function: CohereFnCall,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CohereFnCall {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct CohereToolResult {
    call: CohereToolCall,
    outputs: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct CohereTool {
    r#type: String,
    function: CohereFnDef,
}

#[derive(Serialize)]
struct CohereFnDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

// ── Response types ───────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct CohereStreamEvent {
    #[serde(default, rename = "type")]
    event_type: Option<String>,
    #[serde(default)]
    delta: Option<CohereDelta>,
    #[serde(default)]
    response: Option<CohereResponse>,
}

#[derive(Deserialize, Debug)]
struct CohereDelta {
    #[serde(default)]
    message: Option<CohereDeltaMessage>,
}

#[derive(Deserialize, Debug)]
struct CohereDeltaMessage {
    #[serde(default)]
    content: Option<CohereDeltaContent>,
    #[serde(default)]
    tool_calls: Option<CohereDeltaToolCalls>,
}

#[derive(Deserialize, Debug)]
struct CohereDeltaContent {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct CohereDeltaToolCalls {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<CohereDeltaFn>,
}

#[derive(Deserialize, Debug)]
struct CohereDeltaFn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Deserialize, Debug)]
struct CohereResponse {
    #[serde(default)]
    finish_reason: Option<String>,
    #[serde(default)]
    meta: Option<CohereMeta>,
}

#[derive(Deserialize, Debug)]
struct CohereMeta {
    #[serde(default)]
    tokens: Option<CohereTokens>,
}

#[derive(Deserialize, Debug)]
struct CohereTokens {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

// ── Conversion ───────────────────────────────────────────────────

fn convert_messages(messages: Vec<ChatMessage>) -> Vec<CohereMessage> {
    let mut out = Vec::new();

    for msg in messages {
        match msg.role {
            Role::System => {
                out.push(CohereMessage {
                    role: "system".to_string(),
                    content: Some(msg.text()),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_results: None,
                });
            }
            Role::User => {
                // Check for tool results
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
                        out.push(CohereMessage {
                            role: "tool".to_string(),
                            content: Some(content),
                            tool_calls: None,
                            tool_call_id: Some(id),
                            tool_results: None,
                        });
                    }
                } else {
                    out.push(CohereMessage {
                        role: "user".to_string(),
                        content: Some(msg.text()),
                        tool_calls: None,
                        tool_call_id: None,
                        tool_results: None,
                    });
                }
            }
            Role::Assistant => {
                let tool_uses = msg.tool_uses();
                if tool_uses.is_empty() {
                    out.push(CohereMessage {
                        role: "assistant".to_string(),
                        content: Some(msg.text()),
                        tool_calls: None,
                        tool_call_id: None,
                        tool_results: None,
                    });
                } else {
                    let calls: Vec<CohereToolCall> = tool_uses
                        .iter()
                        .map(|(id, name, input)| CohereToolCall {
                            id: id.to_string(),
                            r#type: "function".to_string(),
                            function: CohereFnCall {
                                name: name.to_string(),
                                arguments: input.to_string(),
                            },
                        })
                        .collect();
                    let text = msg.text();
                    out.push(CohereMessage {
                        role: "assistant".to_string(),
                        content: if text.is_empty() { None } else { Some(text) },
                        tool_calls: Some(calls),
                        tool_call_id: None,
                        tool_results: None,
                    });
                }
            }
            Role::Tool => {
                // Handled via User role ToolResult blocks
            }
        }
    }

    out
}

fn parse_cohere_chunk(event: SseEvent) -> Option<Result<StreamDelta>> {
    if event.data.trim() == "[DONE]" {
        return None;
    }

    let parsed: std::result::Result<CohereStreamEvent, _> = serde_json::from_str(&event.data);
    match parsed {
        Ok(evt) => {
            let event_type = evt.event_type.as_deref().unwrap_or("");

            match event_type {
                "content-delta" => {
                    if let Some(delta) = evt.delta {
                        if let Some(msg) = delta.message {
                            if let Some(content) = msg.content {
                                if let Some(text) = content.text {
                                    if !text.is_empty() {
                                        return Some(Ok(StreamDelta::TextDelta(text)));
                                    }
                                }
                            }
                        }
                    }
                    None
                }
                "tool-call-start" => {
                    if let Some(delta) = evt.delta {
                        if let Some(msg) = delta.message {
                            if let Some(tc) = msg.tool_calls {
                                if let (Some(id), Some(func)) = (tc.id, tc.function) {
                                    if let Some(name) = func.name {
                                        return Some(Ok(StreamDelta::ToolUseStart {
                                            index: 0,
                                            id,
                                            name,
                                        }));
                                    }
                                }
                            }
                        }
                    }
                    None
                }
                "tool-call-delta" => {
                    if let Some(delta) = evt.delta {
                        if let Some(msg) = delta.message {
                            if let Some(tc) = msg.tool_calls {
                                if let Some(func) = tc.function {
                                    if let Some(args) = func.arguments {
                                        return Some(Ok(StreamDelta::ToolInputDelta {
                                            index: 0,
                                            delta: args,
                                        }));
                                    }
                                }
                            }
                        }
                    }
                    None
                }
                "message-end" => {
                    if let Some(resp) = evt.response {
                        if let Some(meta) = resp.meta {
                            if let Some(tokens) = meta.tokens {
                                // Emit usage before stop
                                return Some(Ok(StreamDelta::Usage {
                                    input_tokens: tokens.input_tokens,
                                    output_tokens: tokens.output_tokens,
                                }));
                            }
                        }
                        let stop = match resp.finish_reason.as_deref() {
                            Some("COMPLETE") => Some(StopReason::EndTurn),
                            Some("MAX_TOKENS") => Some(StopReason::MaxTokens),
                            Some("TOOL_CALL") => Some(StopReason::ToolUse),
                            _ => Some(StopReason::EndTurn),
                        };
                        return stop.map(|s| Ok(StreamDelta::Stop(s)));
                    }
                    None
                }
                _ => None,
            }
        }
        Err(e) => {
            warn!(data = %event.data, error = %e, "Failed to parse Cohere SSE chunk");
            None
        }
    }
}

impl LlmClient for CohereClient {
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
                .ok_or_else(|| RyvosError::Config("Cohere: api_key is required".into()))?;

            let base_url = config.base_url.as_deref().unwrap_or(COHERE_API_URL);

            let cohere_messages = convert_messages(messages);
            let cohere_tools: Vec<CohereTool> = tools
                .iter()
                .map(|t| CohereTool {
                    r#type: "function".to_string(),
                    function: CohereFnDef {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.input_schema.clone(),
                    },
                })
                .collect();

            let body = CohereRequest {
                model: config.model_id.clone(),
                messages: cohere_messages,
                max_tokens: Some(config.max_tokens),
                temperature: if config.temperature > 0.0 {
                    Some(config.temperature)
                } else {
                    None
                },
                stream: true,
                tools: cohere_tools,
            };

            let response = self
                .http
                .post(base_url)
                .header("Authorization", format!("Bearer {}", api_key))
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
                    .unwrap_or_else(|_| "unknown".to_string());
                return Err(RyvosError::LlmRequest(format!("HTTP {}: {}", status, body)));
            }

            let byte_stream = response.bytes_stream();
            let sse_stream = SseStream::new(byte_stream);

            let delta_stream =
                sse_stream.filter_map(|event| async move { parse_cohere_chunk(event) });

            Ok(Box::pin(delta_stream) as BoxStream<'_, Result<StreamDelta>>)
        })
    }
}
