mod banner;
mod channels;
mod gateway;
mod hooks;
mod providers;
mod service;
mod skills;
mod web_search;

use std::path::Path;

use anyhow::Result;
use dialoguer::{Confirm, Input, Select};
use ryvos_core::config::{
    AgentConfig, AppConfig, ChannelsConfig, DiscordConfig, DmPolicy, GatewayConfig,
    McpConfig, ModelConfig, TelegramConfig, WizardMetadata,
};

pub enum OnboardingMode {
    QuickStart,
    Manual,
}

#[derive(Default)]
pub struct InitOptions {
    pub non_interactive: bool,
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub api_key: Option<String>,
    pub telegram_token: Option<String>,
    pub discord_token: Option<String>,
    pub enable_gateway: bool,
    pub no_channels: bool,
}

pub async fn run_onboarding(config_path: &Path, options: InitOptions) -> Result<()> {
    if options.non_interactive {
        return run_non_interactive(config_path, options).await;
    }

    run_interactive(config_path).await
}

// ───────────────────────── Non-interactive path ─────────────────────────

async fn run_non_interactive(config_path: &Path, options: InitOptions) -> Result<()> {
    // Resolve provider
    let provider_name = options.provider.unwrap_or_else(|| {
        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            "anthropic".to_string()
        } else if std::env::var("OPENAI_API_KEY").is_ok() {
            "openai".to_string()
        } else {
            "anthropic".to_string()
        }
    });

    // Resolve model
    let model_id = options.model_id.unwrap_or_else(|| {
        match provider_name.as_str() {
            "anthropic" => "claude-sonnet-4-20250514".to_string(),
            "openai" => "gpt-4o".to_string(),
            "ollama" => "llama3.2".to_string(),
            "openrouter" => "anthropic/claude-sonnet-4-20250514".to_string(),
            "gemini" => "gemini-2.0-flash".to_string(),
            _ => "claude-sonnet-4-20250514".to_string(),
        }
    });

    // Resolve API key
    let (api_key, base_url) = resolve_non_interactive_provider(&provider_name, options.api_key)?;

    // Gateway
    let gateway_config = if options.enable_gateway {
        let token = format!("{:x}{:x}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        Some(GatewayConfig {
            bind: "127.0.0.1:18789".to_string(),
            token: Some(token),
            password: None,
            api_keys: vec![],
        })
    } else {
        None
    };

    // Channels
    let channels = if options.no_channels {
        ChannelsConfig::default()
    } else {
        let telegram = options.telegram_token.map(|token| TelegramConfig {
            bot_token: token,
            allowed_users: vec![],
            dm_policy: DmPolicy::Allowlist,
        });
        let discord = options.discord_token.map(|token| DiscordConfig {
            bot_token: token,
            dm_policy: DmPolicy::Allowlist,
            allowed_users: vec![],
        });
        ChannelsConfig { telegram, discord, slack: None }
    };

    let wizard_meta = WizardMetadata {
        last_run_at: Some(chrono::Utc::now().to_rfc3339()),
        last_run_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    let config = AppConfig {
        agent: AgentConfig::default(),
        model: ModelConfig {
            provider: provider_name,
            model_id,
            api_key,
            base_url,
            max_tokens: 8192,
            temperature: 0.0,
            thinking: Default::default(),
            retry: None,
        },
        fallback_models: vec![],
        gateway: gateway_config,
        channels,
        mcp: None,
        hooks: None,
        wizard: Some(wizard_meta),
        cron: None,
        web_search: None,
        security: Default::default(),
        embedding: None,
    };

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let toml_str = toml::to_string_pretty(&config)?;
    std::fs::write(config_path, &toml_str)?;
    println!("Config written to: {}", config_path.display());

    // Auto-install service
    let mode = OnboardingMode::QuickStart;
    service::install(config_path, &mode, true).await?;

    Ok(())
}

fn resolve_non_interactive_provider(
    provider: &str,
    api_key_flag: Option<String>,
) -> Result<(Option<String>, Option<String>)> {
    match provider {
        "anthropic" => {
            let key = api_key_flag.or_else(|| {
                if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                    Some("${ANTHROPIC_API_KEY}".to_string())
                } else {
                    None
                }
            });
            Ok((key, None))
        }
        "openai" => {
            let key = api_key_flag.or_else(|| {
                if std::env::var("OPENAI_API_KEY").is_ok() {
                    Some("${OPENAI_API_KEY}".to_string())
                } else {
                    None
                }
            });
            Ok((key, None))
        }
        "ollama" => {
            Ok((None, Some("http://localhost:11434/v1/chat/completions".to_string())))
        }
        "openrouter" => {
            let key = api_key_flag.or_else(|| {
                if std::env::var("OPENROUTER_API_KEY").is_ok() {
                    Some("${OPENROUTER_API_KEY}".to_string())
                } else {
                    None
                }
            });
            Ok((key, None))
        }
        "gemini" => {
            let key = api_key_flag.or_else(|| {
                if std::env::var("GEMINI_API_KEY").is_ok() {
                    Some("${GEMINI_API_KEY}".to_string())
                } else {
                    None
                }
            });
            Ok((
                key,
                Some("https://generativelanguage.googleapis.com/v1beta/openai".to_string()),
            ))
        }
        _ => {
            // Generic: use provided key or try common env var patterns
            let env_var = format!("{}_API_KEY", provider.to_uppercase().replace('-', "_"));
            let key = api_key_flag.or_else(|| {
                if std::env::var(&env_var).is_ok() {
                    Some(format!("${{{env_var}}}"))
                } else {
                    None
                }
            });
            Ok((key, None))
        }
    }
}

// ───────────────────────── Interactive path ─────────────────────────

async fn run_interactive(config_path: &Path) -> Result<()> {
    // 1. Banner
    banner::print_banner();

    // 2. Risk acknowledgement
    println!();
    println!("  \x1b[1;33mSecurity Notice\x1b[0m");
    println!();
    println!("  This agent can read files and run shell commands if tools are enabled.");
    println!("  A bad prompt can trick it into doing unsafe things (prompt injection).");
    println!();
    println!("  Recommended baseline:");
    println!("  - Use tool allowlists to limit what the agent can do.");
    println!("  - Run in a sandbox or container when possible.");
    println!("  - Keep secrets out of the agent's reachable filesystem.");
    println!("  - Use the strongest model for bots with tools or untrusted inboxes.");
    println!();

    let accepted = Confirm::new()
        .with_prompt("I understand the risks and want to continue")
        .default(false)
        .interact()?;

    if !accepted {
        println!("Setup cancelled.");
        return Ok(());
    }

    // 3. Existing config check
    if config_path.exists() {
        println!();
        println!("  Found existing config: {}", config_path.display());

        if let Ok(existing) = AppConfig::load(config_path) {
            println!(
                "  Provider: {}, Model: {}",
                existing.model.provider, existing.model.model_id
            );
            if existing.channels.telegram.is_some() {
                println!("  Telegram: configured");
            }
            if existing.channels.discord.is_some() {
                println!("  Discord: configured");
            }
            if existing.gateway.is_some() {
                println!("  Gateway: configured");
            }
        }

        let options = &["Use existing config", "Update config", "Reset config"];
        let choice = Select::new()
            .with_prompt("What would you like to do?")
            .items(options)
            .default(0)
            .interact()?;

        match choice {
            0 => {
                println!("Keeping existing config.");
                return Ok(());
            }
            2 => {
                let reset_options = &[
                    "Config only",
                    "Config + sessions",
                    "Full reset (config + sessions + workspace)",
                ];
                let reset_choice = Select::new()
                    .with_prompt("Reset scope")
                    .items(reset_options)
                    .default(0)
                    .interact()?;

                match reset_choice {
                    0 => {
                        std::fs::remove_file(config_path).ok();
                        println!("  Config removed.");
                    }
                    1 => {
                        std::fs::remove_file(config_path).ok();
                        let workspace = dirs_home()
                            .map(|h| h.join(".ryvos"))
                            .unwrap_or_default();
                        let db = workspace.join("sessions.db");
                        std::fs::remove_file(&db).ok();
                        println!("  Config and sessions removed.");
                    }
                    2 => {
                        let workspace = dirs_home()
                            .map(|h| h.join(".ryvos"))
                            .unwrap_or_default();
                        if workspace.exists() {
                            std::fs::remove_dir_all(&workspace).ok();
                            println!("  Full workspace removed.");
                        } else {
                            std::fs::remove_file(config_path).ok();
                            println!("  Config removed.");
                        }
                    }
                    _ => unreachable!(),
                }
            }
            _ => {
                // Update — fall through to wizard
            }
        }
    }

    // 4. Onboarding mode
    let mode_options = &[
        "QuickStart (sensible defaults, minimal prompts)",
        "Manual (configure everything)",
    ];
    let mode_choice = Select::new()
        .with_prompt("Setup mode")
        .items(mode_options)
        .default(0)
        .interact()?;

    let mode = match mode_choice {
        0 => OnboardingMode::QuickStart,
        _ => OnboardingMode::Manual,
    };

    // 5. Provider selection
    println!();
    let provider = providers::select_provider()?;

    // 6. Model selection
    println!();
    let model = providers::select_model(&provider)?;

    // 7. Agent settings (Manual only)
    let agent_config = match mode {
        OnboardingMode::Manual => {
            println!();
            let workspace: String = Input::new()
                .with_prompt("Workspace directory")
                .default("~/.ryvos".to_string())
                .interact_text()?;

            let max_turns: String = Input::new()
                .with_prompt("Max turns per run")
                .default("25".to_string())
                .validate_with(|input: &String| -> std::result::Result<(), String> {
                    input
                        .parse::<usize>()
                        .map(|_| ())
                        .map_err(|_| "Must be a positive number".to_string())
                })
                .interact_text()?;

            let system_prompt: String = Input::new()
                .with_prompt("Custom system prompt (blank for default)")
                .allow_empty(true)
                .interact_text()?;

            AgentConfig {
                workspace,
                max_turns: max_turns.parse().unwrap_or(25),
                system_prompt: if system_prompt.is_empty() {
                    None
                } else {
                    Some(system_prompt)
                },
                ..Default::default()
            }
        }
        OnboardingMode::QuickStart => AgentConfig::default(),
    };

    // 8. Gateway config (Manual only)
    let gateway_config = match mode {
        OnboardingMode::Manual => {
            println!();
            gateway::configure()?
        }
        OnboardingMode::QuickStart => {
            println!();
            println!("  Gateway: disabled (enable later in config.toml)");
            None
        }
    };

    // 9. Channel setup
    println!();
    let channels_config = channels::configure(&mode)?;

    // 10. Skills (create sample skill)
    println!();
    let workspace_path = resolve_workspace(&agent_config.workspace);
    skills::configure(&workspace_path)?;

    // 11. Hooks (Manual only)
    let hooks_config = match mode {
        OnboardingMode::Manual => {
            println!();
            hooks::configure()?
        }
        OnboardingMode::QuickStart => None,
    };

    // 12. Web search MCP
    println!();
    let web_search_mcp = web_search::configure()?;

    // 13. Build and write config
    let wizard_meta = WizardMetadata {
        last_run_at: Some(chrono::Utc::now().to_rfc3339()),
        last_run_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    let mcp = if let Some((name, server_config)) = web_search_mcp {
        let mut servers = std::collections::HashMap::new();
        servers.insert(name, server_config);
        Some(McpConfig { servers })
    } else {
        None
    };

    let config = AppConfig {
        agent: agent_config,
        model: ModelConfig {
            provider: provider.provider,
            model_id: model.model_id,
            api_key: provider.api_key,
            base_url: provider.base_url,
            max_tokens: 8192,
            temperature: 0.0,
            thinking: Default::default(),
            retry: None,
        },
        fallback_models: vec![],
        gateway: gateway_config,
        channels: channels_config,
        mcp,
        hooks: hooks_config,
        wizard: Some(wizard_meta),
        cron: None,
        web_search: None,
        security: Default::default(),
        embedding: None,
    };

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let toml_str = toml::to_string_pretty(&config)?;
    std::fs::write(config_path, &toml_str)?;
    println!();
    println!("  Config written to: {}", config_path.display());

    // 14. Service install (systemd/launchd)
    println!();
    service::install(config_path, &mode, false).await?;

    // 15. Shell completions
    println!();
    let install_completions = Confirm::new()
        .with_prompt("Install shell completion?")
        .default(false)
        .interact()?;

    if install_completions {
        install_shell_completion();
    }

    // 16. Launch prompt
    println!();
    let launch_options = &[
        "Launch TUI (recommended)",
        "Start REPL",
        "Do this later",
    ];
    let launch_choice = Select::new()
        .with_prompt("What next?")
        .items(launch_options)
        .default(0)
        .interact()?;

    // 17. Closing notes
    println!();
    println!("  Setup complete!");
    println!();
    println!("  Security reminders:");
    println!("  - Keep API keys secret. Config uses ${{ENV_VAR}} references where possible.");
    println!("  - Review allowed_users for Telegram to restrict access.");
    println!("  - The agent can execute shell commands — use responsibly.");
    println!();
    println!("  Useful commands:");
    println!("    ryvos           Start REPL");
    println!("    ryvos tui       Launch terminal UI");
    println!("    ryvos config    Show current config");
    println!("    ryvos daemon    Run with channel adapters");
    println!();

    // Launch if requested
    match launch_choice {
        0 => {
            // Re-load and launch TUI
            let config = AppConfig::load(config_path)?;
            launch_app(config, LaunchMode::Tui).await?;
        }
        1 => {
            let config = AppConfig::load(config_path)?;
            launch_app(config, LaunchMode::Repl).await?;
        }
        _ => {
            println!("  Run `ryvos` when you're ready.");
        }
    }

    Ok(())
}

fn resolve_workspace(workspace: &str) -> std::path::PathBuf {
    if let Some(rest) = workspace.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return home.join(rest);
        }
    }
    std::path::PathBuf::from(workspace)
}

enum LaunchMode {
    Tui,
    Repl,
}

async fn launch_app(config: AppConfig, mode: LaunchMode) -> Result<()> {
    use std::sync::Arc;

    use ryvos_core::event::EventBus;
    use ryvos_core::types::SessionId;

    let workspace = config.workspace_dir();
    std::fs::create_dir_all(&workspace).ok();

    let db_path = workspace.join("sessions.db");
    let store = Arc::new(ryvos_memory::SqliteStore::open(&db_path)?);
    let mut tools = ryvos_tools::ToolRegistry::with_builtins();
    let event_bus = Arc::new(EventBus::default());
    let llm = ryvos_llm::create_client(&config.model);

    // Connect MCP servers
    if let Some(ref mcp_config) = config.mcp {
        let manager = Arc::new(ryvos_mcp::McpClientManager::new());
        for (name, server_config) in &mcp_config.servers {
            if server_config.auto_connect {
                ryvos_mcp::connect_and_register(&manager, name, server_config, &mut tools)
                    .await
                    .ok();
            }
        }
    }

    // Load skills
    let skills_dir = workspace.join("skills");
    ryvos_skills::load_and_register_skills(&skills_dir, &mut tools);

    let tools = Arc::new(tokio::sync::RwLock::new(tools));
    let broker = Arc::new(ryvos_agent::ApprovalBroker::new(event_bus.clone()));
    let runtime = Arc::new(ryvos_agent::AgentRuntime::new(
        config.clone(),
        llm,
        tools.clone(),
        store.clone(),
        event_bus.clone(),
    ));

    let session_id = SessionId::new();
    let no_mcp: Option<Arc<ryvos_mcp::McpClientManager>> = None;

    match mode {
        LaunchMode::Tui => {
            ryvos_tui::run_tui(runtime, event_bus, session_id).await?;
        }
        LaunchMode::Repl => {
            crate::run_repl(&runtime, &event_bus, &session_id, &config, &tools, &broker, &no_mcp).await?;
        }
    }

    Ok(())
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(std::path::PathBuf::from)
}

fn install_shell_completion() {
    let shell = std::env::var("SHELL").unwrap_or_default();
    let home = match dirs_home() {
        Some(h) => h,
        None => {
            println!("  Could not determine home directory.");
            return;
        }
    };

    if shell.ends_with("/fish") {
        // Fish: write completion file
        let comp_dir = home.join(".config/fish/completions");
        std::fs::create_dir_all(&comp_dir).ok();
        let comp_file = comp_dir.join("ryvos.fish");
        let content = "ryvos completions fish | source\n";

        if comp_file.exists() {
            if let Ok(existing) = std::fs::read_to_string(&comp_file) {
                if existing.contains("ryvos completions") {
                    println!("  Fish completion already installed.");
                    return;
                }
            }
        }

        match std::fs::write(&comp_file, content) {
            Ok(_) => println!("  Fish completion installed: {}", comp_file.display()),
            Err(e) => println!("  Failed to write fish completion: {e}"),
        }
    } else {
        // Bash/Zsh: append to profile
        let (profile, shell_name) = if shell.ends_with("/zsh") {
            (home.join(".zshrc"), "zsh")
        } else {
            (home.join(".bashrc"), "bash")
        };

        let line = format!("eval \"$(ryvos completions {shell_name})\"");

        // Check if already present
        if let Ok(existing) = std::fs::read_to_string(&profile) {
            if existing.contains("ryvos completions") {
                println!("  Shell completion already installed in {}", profile.display());
                return;
            }
        }

        // Append
        use std::io::Write;
        match std::fs::OpenOptions::new().append(true).create(true).open(&profile) {
            Ok(mut f) => {
                if writeln!(f, "\n# Ryvos shell completion\n{line}").is_ok() {
                    println!("  Completion added to {}", profile.display());
                    println!("  Run `source {}` or restart your shell.", profile.display());
                } else {
                    println!("  Failed to write to {}", profile.display());
                }
            }
            Err(e) => println!("  Failed to open {}: {e}", profile.display()),
        }
    }
}
