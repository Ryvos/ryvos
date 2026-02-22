use std::time::Duration;

use futures::future::BoxFuture;
use futures::stream::BoxStream;
use tracing::{info, warn};

use ryvos_core::config::{ModelConfig, RetryConfig};
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::LlmClient;
use ryvos_core::types::*;

/// An LLM client that retries failed requests and falls back to alternative providers.
pub struct RetryingClient {
    primary: Box<dyn LlmClient>,
    fallbacks: Vec<(ModelConfig, Box<dyn LlmClient>)>,
    retry_config: RetryConfig,
}

impl RetryingClient {
    pub fn new(
        primary: Box<dyn LlmClient>,
        fallbacks: Vec<(ModelConfig, Box<dyn LlmClient>)>,
        retry_config: RetryConfig,
    ) -> Self {
        Self {
            primary,
            fallbacks,
            retry_config,
        }
    }
}

fn is_retryable(e: &RyvosError) -> bool {
    match e {
        RyvosError::LlmRequest(msg) => {
            msg.contains("429")
                || msg.contains("500")
                || msg.contains("502")
                || msg.contains("503")
                || msg.contains("timeout")
                || msg.contains("connection")
        }
        RyvosError::LlmStream(_) => true,
        _ => false,
    }
}

fn calculate_backoff(attempt: u32, config: &RetryConfig) -> Duration {
    let ms = (config.initial_backoff_ms * 2u64.pow(attempt)).min(config.max_backoff_ms);
    // Add jitter: 0.8x to 1.2x
    let jitter = 0.8 + rand::random::<f64>() * 0.4;
    Duration::from_millis((ms as f64 * jitter) as u64)
}

impl LlmClient for RetryingClient {
    fn chat_stream(
        &self,
        config: &ModelConfig,
        messages: Vec<ChatMessage>,
        tools: &[ToolDefinition],
    ) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>> {
        let config = config.clone();
        let messages = messages.clone();
        let tools = tools.to_vec();

        Box::pin(async move {
            let max_retries = self.retry_config.max_retries;

            // Try primary with retries
            let mut last_err = None;
            for attempt in 0..=max_retries {
                match self
                    .primary
                    .chat_stream(&config, messages.clone(), &tools)
                    .await
                {
                    Ok(stream) => return Ok(stream),
                    Err(e) => {
                        if is_retryable(&e) && attempt < max_retries {
                            let backoff = calculate_backoff(attempt, &self.retry_config);
                            warn!(
                                attempt = attempt + 1,
                                max_retries,
                                backoff_ms = backoff.as_millis() as u64,
                                error = %e,
                                "Retrying LLM request"
                            );
                            tokio::time::sleep(backoff).await;
                            last_err = Some(e);
                            continue;
                        }
                        last_err = Some(e);
                        break;
                    }
                }
            }

            // Primary exhausted — try fallbacks
            if !self.fallbacks.is_empty() {
                info!("Primary LLM exhausted, trying fallback models");
            }
            for (fb_config, fb_client) in &self.fallbacks {
                match fb_client
                    .chat_stream(fb_config, messages.clone(), &tools)
                    .await
                {
                    Ok(stream) => {
                        info!(
                            model = %fb_config.model_id,
                            provider = %fb_config.provider,
                            "Fell back to alternative model"
                        );
                        return Ok(stream);
                    }
                    Err(e) => {
                        warn!(
                            model = %fb_config.model_id,
                            error = %e,
                            "Fallback model also failed"
                        );
                        continue;
                    }
                }
            }

            Err(last_err.unwrap_or_else(|| RyvosError::LlmRequest("All providers failed".into())))
        })
    }
}
