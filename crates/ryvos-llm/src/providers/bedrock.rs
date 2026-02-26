use futures::future::BoxFuture;
use futures::stream::BoxStream;

use ryvos_core::config::ModelConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::LlmClient;
use ryvos_core::types::*;

/// AWS Bedrock client stub.
///
/// Bedrock requires AWS SigV4 request signing which needs the `aws-sigv4` and
/// `aws-credential-types` crates. This is a placeholder that returns a clear
/// error message. Full implementation planned for v0.3.0.
pub struct BedrockClient;

impl BedrockClient {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BedrockClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient for BedrockClient {
    fn chat_stream(
        &self,
        config: &ModelConfig,
        _messages: Vec<ChatMessage>,
        _tools: &[ToolDefinition],
    ) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>> {
        let region = config
            .aws_region
            .as_deref()
            .unwrap_or("us-east-1")
            .to_string();

        Box::pin(async move {
            Err(RyvosError::LlmRequest(format!(
                "AWS Bedrock provider (region: {}) requires SigV4 signing which is not yet \
                 implemented. Use the OpenAI-compatible gateway or set provider to 'anthropic' \
                 with a direct API key instead. Full Bedrock support is planned for v0.3.0.",
                region
            )))
        })
    }
}
