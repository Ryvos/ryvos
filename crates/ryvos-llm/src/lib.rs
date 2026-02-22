pub mod providers;
pub mod retry;
pub mod streaming;

use ryvos_core::config::ModelConfig;
use ryvos_core::traits::LlmClient;

pub use providers::anthropic::AnthropicClient;
pub use providers::openai::OpenAiClient;
pub use retry::RetryingClient;

/// Create an LLM client based on the provider name.
pub fn create_client(config: &ModelConfig) -> Box<dyn LlmClient> {
    match config.provider.as_str() {
        "anthropic" | "claude" => Box::new(AnthropicClient::new()),
        // Everything else uses the OpenAI-compatible client
        _ => Box::new(OpenAiClient::new()),
    }
}
