use futures::future::BoxFuture;
use futures::stream::{BoxStream, StreamExt};
use reqwest::Client;
use ryvos_core::config::ModelConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::LlmClient;
use ryvos_core::types::*;

use crate::streaming::SseStream;

/// Azure OpenAI client. Uses the same wire format as OpenAI but different
/// endpoint structure and `api-key` header instead of Bearer token.
pub struct AzureClient {
    http: Client,
}

impl AzureClient {
    pub fn new() -> Self {
        Self {
            http: Client::new(),
        }
    }
}

impl Default for AzureClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient for AzureClient {
    fn chat_stream(
        &self,
        config: &ModelConfig,
        messages: Vec<ChatMessage>,
        tools: &[ToolDefinition],
    ) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>> {
        let config = config.clone();
        let tools = tools.to_vec();

        Box::pin(async move {
            let resource = config
                .azure_resource
                .as_deref()
                .ok_or_else(|| RyvosError::Config("Azure: azure_resource is required".into()))?;
            let deployment = config
                .azure_deployment
                .as_deref()
                .ok_or_else(|| RyvosError::Config("Azure: azure_deployment is required".into()))?;
            let api_version = config.azure_api_version.as_deref().unwrap_or("2024-06-01");
            let api_key = config
                .api_key
                .as_deref()
                .ok_or_else(|| RyvosError::Config("Azure: api_key is required".into()))?;

            let url = format!(
                "https://{resource}.openai.azure.com/openai/deployments/{deployment}/chat/completions?api-version={api_version}"
            );

            // Reuse OpenAI message conversion
            let oai_messages = super::openai::convert_messages(messages);
            let oai_tools = super::openai::convert_tools(&tools);

            let body = serde_json::json!({
                "messages": oai_messages,
                "max_tokens": config.max_tokens,
                "stream": true,
                "tools": oai_tools,
            });

            let response = self
                .http
                .post(&url)
                .header("api-key", api_key)
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

            let delta_stream = sse_stream
                .map(|event| futures::stream::iter(super::openai::parse_chunk(event)))
                .flatten();

            Ok(Box::pin(delta_stream) as BoxStream<'_, Result<StreamDelta>>)
        })
    }
}
