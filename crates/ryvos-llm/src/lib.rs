pub mod providers;
pub mod retry;
pub mod streaming;

use ryvos_core::config::ModelConfig;
use ryvos_core::traits::LlmClient;

pub use providers::anthropic::AnthropicClient;
pub use providers::azure::AzureClient;
pub use providers::bedrock::BedrockClient;
pub use providers::cohere::CohereClient;
pub use providers::gemini::GeminiClient;
pub use providers::openai::OpenAiClient;
pub use retry::RetryingClient;

/// Create an LLM client based on the provider name.
///
/// Supports 16 providers:
/// - `anthropic` / `claude` — Anthropic Messages API
/// - `gemini` — Google Gemini native API
/// - `azure` — Azure OpenAI (api-key header, deployment URL)
/// - `bedrock` — AWS Bedrock (stub, v0.3.0)
/// - `cohere` — Cohere v2 Chat API
/// - `openai` — OpenAI (default fallback)
/// - 10 preset providers (OpenAI-compatible): ollama, groq, openrouter,
///   together, fireworks, cerebras, xai, mistral, perplexity, deepseek
pub fn create_client(config: &ModelConfig) -> Box<dyn LlmClient> {
    match config.provider.as_str() {
        "anthropic" | "claude" => Box::new(AnthropicClient::new()),
        "gemini" | "google" => Box::new(GeminiClient::new()),
        "azure" | "azure-openai" => Box::new(AzureClient::new()),
        "bedrock" | "aws-bedrock" | "aws" => Box::new(BedrockClient::new()),
        "cohere" => Box::new(CohereClient::new()),
        // Everything else uses the OpenAI-compatible client.
        // For known presets, apply default base_url and extra headers
        // via the config's extra_headers and base_url fields (set during
        // config loading or init).
        _ => Box::new(OpenAiClient::new()),
    }
}

/// Resolve preset defaults into a ModelConfig, filling in base_url and
/// extra_headers if not already set by the user.
pub fn apply_preset_defaults(config: &mut ModelConfig) {
    if let Some(preset) = providers::presets::get_preset(&config.provider) {
        // Fill base_url if not set
        if config.base_url.is_none() {
            config.base_url = Some(preset.default_base_url.to_string());
        }

        // Merge preset headers (user headers take precedence)
        let merged = providers::presets::build_extra_headers(&preset, &config.extra_headers);
        config.extra_headers = merged.into_iter().collect();
    }
}
