use std::collections::HashMap;

/// A named provider preset for OpenAI-compatible APIs.
pub struct ProviderPreset {
    pub default_base_url: &'static str,
    pub needs_api_key: bool,
    pub extra_headers: &'static [(&'static str, &'static str)],
}

/// Look up a provider preset by name.
pub fn get_preset(provider: &str) -> Option<ProviderPreset> {
    match provider {
        "ollama" => Some(ProviderPreset {
            default_base_url: "http://localhost:11434/v1/chat/completions",
            needs_api_key: false,
            extra_headers: &[],
        }),
        "groq" => Some(ProviderPreset {
            default_base_url: "https://api.groq.com/openai/v1/chat/completions",
            needs_api_key: true,
            extra_headers: &[],
        }),
        "openrouter" => Some(ProviderPreset {
            default_base_url: "https://openrouter.ai/api/v1/chat/completions",
            needs_api_key: true,
            extra_headers: &[("X-Title", "Ryvos")],
        }),
        "together" => Some(ProviderPreset {
            default_base_url: "https://api.together.xyz/v1/chat/completions",
            needs_api_key: true,
            extra_headers: &[],
        }),
        "fireworks" => Some(ProviderPreset {
            default_base_url: "https://api.fireworks.ai/inference/v1/chat/completions",
            needs_api_key: true,
            extra_headers: &[],
        }),
        "cerebras" => Some(ProviderPreset {
            default_base_url: "https://api.cerebras.ai/v1/chat/completions",
            needs_api_key: true,
            extra_headers: &[],
        }),
        "xai" => Some(ProviderPreset {
            default_base_url: "https://api.x.ai/v1/chat/completions",
            needs_api_key: true,
            extra_headers: &[],
        }),
        "mistral" => Some(ProviderPreset {
            default_base_url: "https://api.mistral.ai/v1/chat/completions",
            needs_api_key: true,
            extra_headers: &[],
        }),
        "perplexity" => Some(ProviderPreset {
            default_base_url: "https://api.perplexity.ai/chat/completions",
            needs_api_key: true,
            extra_headers: &[],
        }),
        "deepseek" => Some(ProviderPreset {
            default_base_url: "https://api.deepseek.com/v1/chat/completions",
            needs_api_key: true,
            extra_headers: &[],
        }),
        _ => None,
    }
}

/// Build extra headers from a preset + user config overrides.
pub fn build_extra_headers(
    preset: &ProviderPreset,
    user_headers: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = preset
        .extra_headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    for (k, v) in user_headers {
        // User overrides take precedence
        if let Some(pos) = headers.iter().position(|(hk, _)| hk == k) {
            headers[pos].1 = v.clone();
        } else {
            headers.push((k.clone(), v.clone()));
        }
    }

    headers
}

/// List all known preset provider names.
pub fn all_preset_names() -> &'static [&'static str] {
    &[
        "ollama",
        "groq",
        "openrouter",
        "together",
        "fireworks",
        "cerebras",
        "xai",
        "mistral",
        "perplexity",
        "deepseek",
    ]
}
