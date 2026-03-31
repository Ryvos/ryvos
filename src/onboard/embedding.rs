use anyhow::Result;
use dialoguer::{Confirm, Input, Select};
use ryvos_core::config::EmbeddingConfig;

pub fn configure() -> Result<Option<EmbeddingConfig>> {
    let enable = Confirm::new()
        .with_prompt("Enable semantic memory (embeddings)?")
        .default(false)
        .interact()?;

    if !enable {
        return Ok(None);
    }

    let providers = &["OpenAI", "Ollama (local)", "Custom (OpenAI-compatible)"];
    let choice = Select::new()
        .with_prompt("Embedding provider")
        .items(providers)
        .default(0)
        .interact()?;

    let (provider, default_model, default_base_url, needs_key) = match choice {
        0 => ("openai", "text-embedding-3-small", None, true),
        1 => (
            "ollama",
            "nomic-embed-text",
            Some("http://localhost:11434/v1"),
            false,
        ),
        _ => ("custom", "text-embedding-3-small", None, true),
    };

    let model: String = Input::new()
        .with_prompt("Model name")
        .default(default_model.to_string())
        .interact_text()?;

    let base_url = if let Some(default) = default_base_url {
        let url: String = Input::new()
            .with_prompt("Base URL")
            .default(default.to_string())
            .interact_text()?;
        Some(url)
    } else if choice == 2 {
        let url: String = Input::new().with_prompt("Base URL").interact_text()?;
        Some(url)
    } else {
        None
    };

    let api_key = if needs_key {
        let env_var = match provider {
            "openai" => "OPENAI_API_KEY",
            _ => "EMBEDDING_API_KEY",
        };
        if std::env::var(env_var).is_ok() {
            Some(format!("${{{env_var}}}"))
        } else {
            let key: String = Input::new()
                .with_prompt(format!("API key (or set {env_var})"))
                .interact_text()?;
            Some(key)
        }
    } else {
        None
    };

    let dimensions: String = Input::new()
        .with_prompt("Embedding dimensions")
        .default("1536".to_string())
        .interact_text()?;

    Ok(Some(EmbeddingConfig {
        provider: provider.to_string(),
        model,
        base_url,
        api_key,
        dimensions: dimensions.parse().unwrap_or(1536),
    }))
}
