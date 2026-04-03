use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};
use futures::StreamExt;

use ryvos_core::config::ModelConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::LlmClient;
use ryvos_core::types::*;

/// A mock LLM client for testing. Returns pre-configured sequences of
/// `StreamDelta` values, and records every call for assertion.
#[derive(Clone)]
pub struct MockLlmClient {
    responses: Arc<Mutex<Vec<Vec<StreamDelta>>>>,
    calls: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
}

impl MockLlmClient {
    /// Create a new mock with no pre-configured responses.
    /// Calls to `chat_stream` will return an error.
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(Vec::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add a response sequence. Each call to `chat_stream` pops the first
    /// response from the queue. If the queue is empty, returns an error.
    pub fn with_response(self, deltas: Vec<StreamDelta>) -> Self {
        self.responses.lock().unwrap().push(deltas);
        self
    }

    /// Convenience: add a simple text response that ends with EndTurn.
    pub fn with_text_response(self, text: &str) -> Self {
        self.with_response(vec![
            StreamDelta::TextDelta(text.to_string()),
            StreamDelta::Usage {
                input_tokens: 100,
                output_tokens: 50,
            },
            StreamDelta::Stop(StopReason::EndTurn),
        ])
    }

    /// Convenience: add a tool call response.
    pub fn with_tool_call(self, name: &str, input_json: &str) -> Self {
        self.with_response(vec![
            StreamDelta::ToolUseStart {
                index: 0,
                id: format!("tool_{}", name),
                name: name.to_string(),
            },
            StreamDelta::ToolInputDelta {
                index: 0,
                delta: input_json.to_string(),
            },
            StreamDelta::Usage {
                input_tokens: 100,
                output_tokens: 50,
            },
            StreamDelta::Stop(StopReason::ToolUse),
        ])
    }

    /// How many times `chat_stream` was called.
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    /// Get the messages from call N (0-indexed).
    pub fn call_messages(&self, n: usize) -> Vec<ChatMessage> {
        self.calls.lock().unwrap()[n].clone()
    }
}

impl Default for MockLlmClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmClient for MockLlmClient {
    fn chat_stream(
        &self,
        _config: &ModelConfig,
        messages: Vec<ChatMessage>,
        _tools: &[ToolDefinition],
    ) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>> {
        self.calls.lock().unwrap().push(messages);

        let deltas = {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                return Box::pin(async {
                    Err(RyvosError::LlmRequest("No more mock responses".into()))
                });
            }
            responses.remove(0)
        };

        Box::pin(async move {
            let stream = stream::iter(deltas.into_iter().map(Ok));
            Ok(stream.boxed())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_llm_text_response() {
        let client = MockLlmClient::new().with_text_response("hello world");
        let config = crate::test_config();
        let config = &config.model;
        let msgs = vec![ChatMessage::user("hi")];
        let mut stream = client.chat_stream(config, msgs, &[]).await.unwrap();

        let mut texts = Vec::new();
        while let Some(Ok(delta)) = stream.next().await {
            if let StreamDelta::TextDelta(t) = delta {
                texts.push(t);
            }
        }
        assert_eq!(texts, vec!["hello world"]);
        assert_eq!(client.call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_llm_tool_call() {
        let client = MockLlmClient::new().with_tool_call("bash", r#"{"command":"ls"}"#);
        let config = crate::test_config();
        let config = &config.model;
        let mut stream = client
            .chat_stream(config, vec![ChatMessage::user("list files")], &[])
            .await
            .unwrap();

        let mut tool_name = None;
        while let Some(Ok(delta)) = stream.next().await {
            if let StreamDelta::ToolUseStart { name, .. } = delta {
                tool_name = Some(name);
            }
        }
        assert_eq!(tool_name.unwrap(), "bash");
    }

    #[tokio::test]
    async fn test_mock_llm_no_responses_errors() {
        let client = MockLlmClient::new();
        let config = crate::test_config();
        let config = &config.model;
        let result = client
            .chat_stream(config, vec![ChatMessage::user("hi")], &[])
            .await;
        assert!(result.is_err());
    }
}
