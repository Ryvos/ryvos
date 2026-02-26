use anyhow::Result;
use dialoguer::{Confirm, Input, Password, Select};

pub struct ProviderChoice {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

pub struct ModelChoice {
    pub model_id: String,
}

pub fn select_provider() -> Result<ProviderChoice> {
    let providers = &[
        "Anthropic (API key)",
        "OpenAI (API key)",
        "OpenRouter (API key)",
        "Google Gemini (API key)",
        "Moonshot AI (API key)",
        "Z.AI (API key)",
        "Venice AI (API key)",
        "Synthetic (API key)",
        "Xiaomi (API key)",
        "MiniMax (API key)",
        "Vercel AI Gateway (URL + key)",
        "OpenCode Zen (URL + key)",
        "Ollama (local, no key needed)",
        "Custom (OpenAI-compatible)",
    ];

    let selection = Select::new()
        .with_prompt("Model provider")
        .items(providers)
        .default(0)
        .interact()?;

    match selection {
        0 => configure_keyed_provider("anthropic", "ANTHROPIC_API_KEY"),
        1 => configure_keyed_provider("openai", "OPENAI_API_KEY"),
        2 => configure_keyed_provider("openrouter", "OPENROUTER_API_KEY"),
        3 => configure_keyed_provider_with_url(
            "gemini",
            "GEMINI_API_KEY",
            "https://generativelanguage.googleapis.com/v1beta/openai",
        ),
        4 => configure_keyed_provider_with_url(
            "moonshot",
            "MOONSHOT_API_KEY",
            "https://api.moonshot.cn/v1",
        ),
        5 => configure_keyed_provider_with_url(
            "zai",
            "ZAI_API_KEY",
            "https://open.bigmodel.cn/api/paas/v4",
        ),
        6 => configure_keyed_provider_with_url(
            "venice",
            "VENICE_API_KEY",
            "https://api.venice.ai/api/v1",
        ),
        7 => configure_keyed_provider_with_url(
            "synthetic",
            "SYNTHETIC_API_KEY",
            "https://api.synthetic.computer/v1",
        ),
        8 => configure_keyed_provider_with_url(
            "xiaomi",
            "XIAOMI_API_KEY",
            "https://api.xiaomi.com/v1",
        ),
        9 => configure_keyed_provider_with_url(
            "minimax",
            "MINIMAX_API_KEY",
            "https://api.minimax.chat/v1",
        ),
        10 => configure_keyed_provider_with_custom_url("ai-gateway", "AI_GATEWAY_API_KEY"),
        11 => configure_keyed_provider_with_custom_url("opencode-zen", "OPENCODE_ZEN_API_KEY"),
        12 => configure_ollama(),
        13 => configure_custom(),
        _ => unreachable!(),
    }
}

fn configure_keyed_provider(provider: &str, env_var: &str) -> Result<ProviderChoice> {
    let api_key = if let Ok(existing) = std::env::var(env_var) {
        let masked = mask_key(&existing);
        let use_existing = Confirm::new()
            .with_prompt(format!("Found {env_var} ({masked}). Use it?"))
            .default(true)
            .interact()?;

        if use_existing {
            format!("${{{env_var}}}")
        } else {
            prompt_api_key()?
        }
    } else {
        prompt_api_key()?
    };

    Ok(ProviderChoice {
        provider: provider.to_string(),
        api_key: Some(api_key),
        base_url: None,
    })
}

fn configure_keyed_provider_with_url(
    provider: &str,
    env_var: &str,
    base_url: &str,
) -> Result<ProviderChoice> {
    let api_key = if let Ok(existing) = std::env::var(env_var) {
        let masked = mask_key(&existing);
        let use_existing = Confirm::new()
            .with_prompt(format!("Found {env_var} ({masked}). Use it?"))
            .default(true)
            .interact()?;

        if use_existing {
            format!("${{{env_var}}}")
        } else {
            prompt_api_key()?
        }
    } else {
        prompt_api_key()?
    };

    Ok(ProviderChoice {
        provider: provider.to_string(),
        api_key: Some(api_key),
        base_url: Some(base_url.to_string()),
    })
}

fn configure_keyed_provider_with_custom_url(
    provider: &str,
    env_var: &str,
) -> Result<ProviderChoice> {
    let base_url: String = Input::new()
        .with_prompt("Base URL (include /v1)")
        .interact_text()?;

    let api_key = if let Ok(existing) = std::env::var(env_var) {
        let masked = mask_key(&existing);
        let use_existing = Confirm::new()
            .with_prompt(format!("Found {env_var} ({masked}). Use it?"))
            .default(true)
            .interact()?;

        if use_existing {
            format!("${{{env_var}}}")
        } else {
            prompt_api_key()?
        }
    } else {
        prompt_api_key()?
    };

    Ok(ProviderChoice {
        provider: provider.to_string(),
        api_key: Some(api_key),
        base_url: Some(base_url),
    })
}

fn configure_ollama() -> Result<ProviderChoice> {
    let base_url: String = Input::new()
        .with_prompt("Ollama base URL")
        .default("http://localhost:11434/v1/chat/completions".to_string())
        .interact_text()?;

    Ok(ProviderChoice {
        provider: "ollama".to_string(),
        api_key: None,
        base_url: Some(base_url),
    })
}

fn configure_custom() -> Result<ProviderChoice> {
    let base_url: String = Input::new()
        .with_prompt("Base URL (include /v1)")
        .interact_text()?;

    let api_key: String = Password::new()
        .with_prompt("API key (blank if none)")
        .allow_empty_password(true)
        .interact()?;

    let api_key = if api_key.is_empty() {
        None
    } else {
        Some(api_key)
    };

    Ok(ProviderChoice {
        provider: "custom".to_string(),
        api_key,
        base_url: Some(base_url),
    })
}

pub fn select_model(provider: &ProviderChoice) -> Result<ModelChoice> {
    let (models, default_idx) = match provider.provider.as_str() {
        "anthropic" => (
            vec![
                "claude-sonnet-4-20250514",
                "claude-opus-4-20250514",
                "claude-haiku-4-5-20251001",
                "Enter manually",
            ],
            0,
        ),
        "openai" => (
            vec!["gpt-4o", "gpt-4o-mini", "o3-mini", "Enter manually"],
            0,
        ),
        "gemini" => (
            vec!["gemini-2.0-flash", "gemini-1.5-pro", "Enter manually"],
            0,
        ),
        "moonshot" => (vec!["kimi-k2-0905-preview", "Enter manually"], 0),
        "zai" => (vec!["glm-4.7", "Enter manually"], 0),
        "xiaomi" => (vec!["mimo-v2-flash", "Enter manually"], 0),
        "minimax" => (
            vec!["MiniMax-M2.1", "MiniMax-M2.1-Lightning", "Enter manually"],
            0,
        ),
        "ollama" => (
            vec![
                "llama3.2",
                "llama3.1",
                "codellama",
                "mistral",
                "deepseek-coder-v2",
                "Enter manually",
            ],
            0,
        ),
        _ => (vec!["Enter manually"], 0),
    };

    if models.len() == 1 {
        // Only "Enter manually"
        let model_id: String = Input::new().with_prompt("Model ID").interact_text()?;
        return Ok(ModelChoice { model_id });
    }

    let selection = Select::new()
        .with_prompt("Default model")
        .items(&models)
        .default(default_idx)
        .interact()?;

    let model_id = if models[selection] == "Enter manually" {
        Input::new().with_prompt("Model ID").interact_text()?
    } else {
        models[selection].to_string()
    };

    Ok(ModelChoice { model_id })
}

fn prompt_api_key() -> Result<String> {
    let key = Password::new().with_prompt("API key").interact()?;
    Ok(key)
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        "****".to_string()
    } else {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    }
}
