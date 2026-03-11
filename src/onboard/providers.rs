use anyhow::Result;
use dialoguer::{Confirm, Input, Password, Select};

pub struct ProviderChoice {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub claude_command: Option<String>,
    pub cli_allowed_tools: Vec<String>,
    pub cli_permission_mode: Option<String>,
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
        "Claude Code (CLI, no API key)",
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
        12 => configure_claude_code(),
        13 => configure_ollama(),
        14 => configure_custom(),
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
        claude_command: None,
        cli_allowed_tools: vec![],
        cli_permission_mode: None,
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
        claude_command: None,
        cli_allowed_tools: vec![],
        cli_permission_mode: None,
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
        claude_command: None,
        cli_allowed_tools: vec![],
        cli_permission_mode: None,
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
        claude_command: None,
        cli_allowed_tools: vec![],
        cli_permission_mode: None,
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
        claude_command: None,
        cli_allowed_tools: vec![],
        cli_permission_mode: None,
    })
}

fn configure_claude_code() -> Result<ProviderChoice> {
    let detected = detect_claude_binary();

    match &detected {
        Some(path) => {
            // Try to get version
            let version = std::process::Command::new(path)
                .arg("--version")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_default();
            let version = version.trim();
            if version.is_empty() {
                println!("  Found claude at: {}", path);
            } else {
                println!("  Found claude at: {} ({})", path, version);
            }
        }
        None => {
            println!("  \x1b[1;33mWarning:\x1b[0m claude CLI not found in PATH.");
            println!("  Install it from https://docs.anthropic.com/en/docs/claude-code");
        }
    }

    let default_path = detected.unwrap_or_else(|| "claude".to_string());
    let command: String = Input::new()
        .with_prompt("Claude CLI path")
        .default(default_path)
        .interact_text()?;

    let billing_options = &["Subscription (Max/Pro plan, no API key)", "API key billing"];
    let billing_choice = Select::new()
        .with_prompt("Billing type")
        .items(billing_options)
        .default(0)
        .interact()?;

    let api_key = if billing_choice == 1 {
        Some(prompt_api_key()?)
    } else {
        None
    };

    // Permission level for CLI tool access
    let perm_options = &[
        "Full access (default, guardian monitors for dangerous ops)",
        "Restricted (whitelist specific tools via --allowedTools)",
    ];
    let perm_choice = Select::new()
        .with_prompt("CLI permission level")
        .items(perm_options)
        .default(0)
        .interact()?;

    let (cli_allowed_tools, cli_permission_mode) = match perm_choice {
        1 => {
            println!("  Restricting CLI to read-only tools. Edit cli_allowed_tools in config to customize.");
            (
                vec![
                    "Read".into(), "Glob".into(), "Grep".into(),
                    "WebSearch".into(), "WebFetch".into(), "mcp__*".into(),
                ],
                Some("plan".to_string()),
            )
        }
        _ => (vec![], None),
    };

    Ok(ProviderChoice {
        provider: "claude-code".to_string(),
        api_key,
        base_url: None,
        claude_command: Some(command),
        cli_allowed_tools,
        cli_permission_mode,
    })
}

fn detect_claude_binary() -> Option<String> {
    std::process::Command::new("which")
        .arg("claude")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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
        "claude-code" => (
            vec![
                "claude-sonnet-4-20250514",
                "claude-opus-4-20250514",
                "default",
                "Enter manually",
            ],
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
