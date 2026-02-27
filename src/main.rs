mod doctor;
mod onboard;

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use clap::{CommandFactory, Parser, Subcommand};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use ryvos_core::config::{AppConfig, HooksConfig, McpJsonConfig, RetryConfig};
use ryvos_core::event::EventBus;
use ryvos_core::security::ApprovalDecision;
use ryvos_core::types::{AgentEvent, SessionId, ThinkingLevel};

use ryvos_agent::{AgentRuntime, ApprovalBroker, Guardian, SecurityGate};
use ryvos_memory::SqliteStore;
use ryvos_tools::ToolRegistry;

#[derive(Parser)]
#[command(name = "ryvos", version, about = "Blazingly fast AI agent runtime")]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "ryvos.toml")]
    config: PathBuf,

    /// Session ID (auto-generated if not provided)
    #[arg(short, long)]
    session: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start interactive REPL mode
    Repl,
    /// Run a single prompt and exit
    Run {
        /// The prompt to send to the agent
        #[arg(trailing_var_arg = true)]
        prompt: Vec<String>,
    },
    /// Show current configuration
    Config,
    /// Launch the terminal UI
    Tui,
    /// Start the WebSocket gateway server
    Serve,
    /// Run as a daemon with channel adapters (Telegram, Discord)
    Daemon {
        /// Also start the HTTP/WebSocket gateway
        #[arg(long)]
        gateway: bool,
    },
    /// Interactive setup wizard
    Init {
        /// Accept all defaults without prompting
        #[arg(long, short = 'y')]
        yes: bool,
        /// Provider name (anthropic, openai, ollama, groq, together, etc.)
        #[arg(long)]
        provider: Option<String>,
        /// Model ID
        #[arg(long)]
        model_id: Option<String>,
        /// API key (raw value or ${ENV_VAR} reference)
        #[arg(long)]
        api_key: Option<String>,
        /// Base URL for the LLM API
        #[arg(long)]
        base_url: Option<String>,
        /// Security level: strict (T0), standard (T1), permissive (T2)
        #[arg(long)]
        security_level: Option<String>,
        /// Comma-separated channels to enable (telegram,discord)
        #[arg(long)]
        channels: Option<String>,
        /// Read all config from environment variables (RYVOS_PROVIDER, etc.)
        #[arg(long)]
        from_env: bool,
        /// Telegram bot token
        #[arg(long)]
        telegram_token: Option<String>,
        /// Discord bot token
        #[arg(long)]
        discord_token: Option<String>,
        /// Enable gateway with defaults
        #[arg(long)]
        gateway: bool,
        /// Skip channel configuration
        #[arg(long)]
        no_channels: bool,
    },
    /// Run system health checks
    Doctor,
    /// Show tool health statistics
    Health {
        /// Number of days to look back (default: 7)
        #[arg(long, default_value = "7")]
        days: u64,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Manage MCP servers
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Manage skill registry
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// Personalize your agent with a soul interview
    Soul,
}

#[derive(Subcommand)]
enum McpAction {
    /// List configured MCP servers
    List,
    /// Add an MCP server to config
    Add {
        /// Server name
        name: String,
        /// Command for stdio transport
        #[arg(long)]
        command: Option<String>,
        /// Arguments for the command
        #[arg(long)]
        args: Vec<String>,
        /// URL for SSE transport
        #[arg(long)]
        url: Option<String>,
        /// Environment variables (KEY=VALUE)
        #[arg(long)]
        env: Vec<String>,
    },
    /// Remove an MCP server from config
    Remove {
        /// Server name
        name: String,
    },
}

#[derive(Subcommand)]
enum SkillAction {
    /// List installed skills
    List {
        /// Also show available skills from the remote registry
        #[arg(long)]
        remote: bool,
    },
    /// Search for skills in the registry
    Search {
        /// Search query
        query: String,
    },
    /// Install a skill from the registry
    Install {
        /// Skill name
        name: String,
    },
    /// Remove an installed skill
    Remove {
        /// Skill name
        name: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("ryvos=info,warn")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // Handle completions before config loading
    if let Some(Commands::Completions { shell }) = &cli.command {
        let mut cmd = Cli::command();
        clap_complete::generate(*shell, &mut cmd, "ryvos", &mut std::io::stdout());
        return Ok(());
    }

    // Handle init before config loading
    if let Some(Commands::Init {
        yes,
        provider,
        model_id,
        api_key,
        base_url,
        security_level,
        channels,
        from_env,
        telegram_token,
        discord_token,
        gateway,
        no_channels,
    }) = cli.command
    {
        let dest = if cli.config == *"ryvos.toml" {
            dirs_home()
                .map(|h| h.join(".ryvos").join("config.toml"))
                .unwrap_or_else(|| cli.config.clone())
        } else {
            cli.config.clone()
        };
        let options = onboard::InitOptions {
            non_interactive: yes || from_env,
            provider,
            model_id,
            api_key,
            base_url,
            security_level,
            channels,
            from_env,
            telegram_token,
            discord_token,
            enable_gateway: gateway,
            no_channels,
        };
        return onboard::run_onboarding(&dest, options).await;
    }

    // Handle soul interview before config loading
    if let Some(Commands::Soul) = &cli.command {
        let workspace = dirs_home()
            .map(|h| h.join(".ryvos"))
            .unwrap_or_else(|| PathBuf::from(".ryvos"));
        return onboard::run_soul_interview(&workspace);
    }

    // Handle MCP CLI subcommands before config loading
    if let Some(Commands::Mcp { action }) = &cli.command {
        return handle_mcp_cli(action, &cli.config);
    }

    // Handle Skill CLI subcommands before config loading
    if let Some(Commands::Skill { action }) = &cli.command {
        return handle_skill_cli(action).await;
    }

    // Load config
    let config = if cli.config.exists() {
        AppConfig::load(&cli.config)?
    } else {
        // Check for config in common locations
        let home_config = dirs_home().map(|h| h.join(".ryvos").join("config.toml"));

        if let Some(ref path) = home_config {
            if path.exists() {
                info!(path = %path.display(), "Loading config from home directory");
                AppConfig::load(path)?
            } else {
                eprintln!(
                    "Warning: No config file found. Set ANTHROPIC_API_KEY or create ryvos.toml"
                );
                eprintln!("See ryvos.toml.example for reference.");

                // Create minimal config from env
                create_env_config()?
            }
        } else {
            create_env_config()?
        }
    };

    // Set up components
    let workspace = config.workspace_dir();
    std::fs::create_dir_all(&workspace).ok();

    let db_path = workspace.join("sessions.db");
    let store = Arc::new(SqliteStore::open(&db_path)?);
    let mut tools = ToolRegistry::with_builtins();
    let event_bus = Arc::new(EventBus::default());

    // Build LLM client with retry and fallback chain
    let primary_llm = ryvos_llm::create_client(&config.model);
    let llm: Arc<dyn ryvos_core::traits::LlmClient> =
        if !config.fallback_models.is_empty() || config.model.retry.is_some() {
            let retry_config = config
                .model
                .retry
                .clone()
                .unwrap_or_else(RetryConfig::default);
            let fallbacks: Vec<_> = config
                .fallback_models
                .iter()
                .map(|mc| {
                    let client = ryvos_llm::create_client(mc);
                    (mc.clone(), client)
                })
                .collect();
            Arc::new(ryvos_llm::RetryingClient::new(
                primary_llm,
                fallbacks,
                retry_config,
            ))
        } else {
            Arc::from(primary_llm)
        };

    // Merge .mcp.json project config if present
    let mut mcp_config = config.mcp.clone().unwrap_or_default();
    if let Some(project_mcp) = load_mcp_json() {
        for (name, entry) in project_mcp.mcp_servers {
            if let Some(server_config) = entry.to_server_config() {
                if let std::collections::hash_map::Entry::Vacant(e) =
                    mcp_config.servers.entry(name.clone())
                {
                    info!(server = %name, "Loaded MCP server from .mcp.json");
                    e.insert(server_config);
                }
            }
        }
    }

    // Connect MCP servers and register bridged tools
    let mcp_manager = if !mcp_config.servers.is_empty() {
        let manager = Arc::new(ryvos_mcp::McpClientManager::new());
        for (name, server_config) in &mcp_config.servers {
            if server_config.auto_connect {
                match ryvos_mcp::connect_and_register(&manager, name, server_config, &mut tools)
                    .await
                {
                    Ok(count) => info!(server = %name, tools = count, "MCP server connected"),
                    Err(e) => error!(server = %name, error = %e, "Failed to connect MCP server"),
                }
            }
        }

        // Register the mcp_read_resource tool if any servers are connected
        if !manager.connected_servers().await.is_empty() {
            tools.register(ryvos_mcp::McpReadResourceTool::new(manager.clone()));
        }

        Some(manager)
    } else {
        None
    };

    // Register web search tool if configured
    if let Some(ref ws_config) = config.web_search {
        tools.register(ryvos_tools::builtin::web_search::WebSearchTool::new(
            &ws_config.api_key,
        ));
        info!(
            "Web search tool registered (provider: {})",
            ws_config.provider
        );
    }

    // Load drop-in skills
    let skills_dir = workspace.join("skills");
    let skill_count = ryvos_skills::load_and_register_skills(&skills_dir, &mut tools);
    if skill_count > 0 {
        info!(count = skill_count, "Loaded skills");
    }

    let tools = Arc::new(tokio::sync::RwLock::new(tools));

    // Build security gate
    let policy = config.security.to_policy();
    let broker = Arc::new(ApprovalBroker::new(event_bus.clone()));
    let gate = Arc::new(SecurityGate::new(
        policy,
        tools.clone(),
        broker.clone(),
        event_bus.clone(),
    ));

    info!(
        auto_approve = %config.security.auto_approve_up_to,
        "Security gate initialized"
    );

    // Spawn MCP event listener for dynamic tool refresh
    if let Some(ref manager) = mcp_manager {
        let mut event_rx = manager.subscribe_events();
        let tools_for_events = tools.clone();
        let manager_for_events = manager.clone();
        tokio::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                match event {
                    ryvos_mcp::McpEvent::ToolsChanged { server } => {
                        info!(server = %server, "MCP tools changed, refreshing");
                        let mut registry = tools_for_events.write().await;
                        match ryvos_mcp::refresh_tools(&manager_for_events, &server, &mut registry)
                            .await
                        {
                            Ok(count) => {
                                info!(server = %server, tools = count, "MCP tools refreshed");
                            }
                            Err(e) => {
                                error!(server = %server, error = %e, "Failed to refresh MCP tools");
                            }
                        }
                    }
                    ryvos_mcp::McpEvent::ResourcesChanged { server } => {
                        info!(server = %server, "MCP resources changed");
                    }
                    ryvos_mcp::McpEvent::PromptsChanged { server } => {
                        info!(server = %server, "MCP prompts changed");
                    }
                    ryvos_mcp::McpEvent::ResourceUpdated { server, uri } => {
                        info!(server = %server, uri = %uri, "MCP resource updated");
                    }
                    ryvos_mcp::McpEvent::LogMessage {
                        server,
                        level,
                        message,
                    } => {
                        info!(server = %server, level = %level, "MCP: {}", message);
                    }
                }
            }
        });
    }

    // Initialize failure journal for self-healing
    let journal_path = workspace.join("healing.db");
    let journal = match ryvos_agent::FailureJournal::open(&journal_path) {
        Ok(j) => {
            info!("Failure journal initialized");
            Some(Arc::new(j))
        }
        Err(e) => {
            error!(error = %e, "Failed to initialize failure journal");
            None
        }
    };

    let session_mgr = Arc::new(ryvos_agent::SessionManager::new());
    let mut runtime_inner =
        AgentRuntime::new_with_gate(config.clone(), llm, gate, store.clone(), event_bus.clone());
    if let Some(ref j) = journal {
        runtime_inner.set_journal(j.clone());
    }

    let session_id = cli
        .session
        .map(|s| SessionId::from_string(&s))
        .unwrap_or_else(SessionId::new);

    // Spawn Guardian watchdog if enabled
    if config.agent.guardian.enabled {
        let (guardian, hint_rx) = Guardian::new(
            config.agent.guardian.clone(),
            event_bus.clone(),
            runtime_inner.cancel_token(),
        );
        runtime_inner.set_guardian_hints(hint_rx);
        tokio::spawn(guardian.run(session_id.clone()));
    }

    // Spawn RunLogger if logging is enabled
    if let Some(ref log_config) = config.agent.log {
        if log_config.enabled {
            let log_dir = log_config
                .log_dir
                .as_ref()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| workspace.join("logs"));
            let logger = ryvos_agent::RunLogger::new(log_dir, log_config.level);
            let log_bus = event_bus.clone();
            let log_session = session_id.clone();
            let log_cancel = runtime_inner.cancel_token();
            tokio::spawn(async move {
                logger.run(log_bus, log_session, log_cancel).await;
            });
            info!("RunLogger started (level {})", log_config.level);
        }
    }

    let runtime = Arc::new(runtime_inner);

    match cli.command {
        Some(Commands::Doctor) => {
            println!("Ryvos Doctor");
            println!("============");
            doctor::run_doctor(&config);
            return Ok(());
        }
        Some(Commands::Health { days }) => {
            let journal_path = workspace.join("healing.db");
            match ryvos_agent::FailureJournal::open(&journal_path) {
                Ok(journal) => {
                    let since = chrono::Utc::now() - chrono::Duration::days(days as i64);
                    match journal.tool_health(since) {
                        Ok(health) => {
                            println!("Tool Health (last {} days):", days);
                            if health.is_empty() {
                                println!("  No tool usage recorded yet.");
                            } else {
                                let mut entries: Vec<_> = health.into_iter().collect();
                                entries.sort_by_key(|(name, _)| name.clone());
                                for (tool, (successes, failures)) in &entries {
                                    let total = successes + failures;
                                    let pct = if total > 0 {
                                        (*successes as f64 / total as f64 * 100.0) as u32
                                    } else {
                                        100
                                    };
                                    let status = if pct < 90 { " [degraded]" } else { "" };
                                    println!(
                                        "  {:<18} {}% success ({}/{}){}",
                                        format!("{}:", tool),
                                        pct,
                                        successes,
                                        total,
                                        status
                                    );
                                }
                            }
                        }
                        Err(e) => eprintln!("Failed to query tool health: {}", e),
                    }
                }
                Err(e) => eprintln!("Failed to open healing journal: {}", e),
            }
            return Ok(());
        }
        Some(Commands::Config) => {
            println!("{}", toml::to_string_pretty(&config)?);
        }
        Some(Commands::Run { prompt }) => {
            let text = prompt.join(" ");
            if text.is_empty() {
                // Read from stdin
                let stdin = io::stdin();
                let input: String = stdin
                    .lock()
                    .lines()
                    .map_while(|l| l.ok())
                    .collect::<Vec<_>>()
                    .join("\n");
                run_once(
                    &runtime,
                    &event_bus,
                    &session_id,
                    &input,
                    &config.hooks,
                    &broker,
                )
                .await?;
            } else {
                run_once(
                    &runtime,
                    &event_bus,
                    &session_id,
                    &text,
                    &config.hooks,
                    &broker,
                )
                .await?;
            }
        }
        Some(Commands::Tui) => {
            ryvos_tui::run_tui(
                runtime.clone(),
                event_bus.clone(),
                session_id,
                Some(broker.clone()),
            )
            .await?;
        }
        Some(Commands::Serve) => {
            let gateway_config = config.gateway.clone().unwrap_or_default();
            info!(bind = %gateway_config.bind, "Starting WebSocket gateway");
            let server = ryvos_gateway::GatewayServer::new(
                gateway_config,
                runtime,
                event_bus,
                store,
                session_mgr,
                broker,
            );
            let cancel = tokio_util::sync::CancellationToken::new();
            let cancel_clone = cancel.clone();

            // Graceful shutdown on Ctrl-C
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                info!("Shutting down gateway...");
                cancel_clone.cancel();
            });

            server.run(cancel).await?;
        }
        Some(Commands::Daemon { gateway }) => {
            info!("Starting daemon with channel adapters");
            let cancel = tokio_util::sync::CancellationToken::new();
            let cancel_clone = cancel.clone();

            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.ok();
                info!("Shutting down daemon...");
                cancel_clone.cancel();
            });

            // Start cron scheduler if configured
            if let Some(ref cron_config) = config.cron {
                if !cron_config.jobs.is_empty() {
                    let scheduler = ryvos_agent::CronScheduler::new(
                        cron_config,
                        runtime.clone(),
                        event_bus.clone(),
                        cancel.clone(),
                    );
                    tokio::spawn(async move {
                        scheduler.run().await;
                    });
                    info!("Cron scheduler started");
                }
            }

            // Start heartbeat if configured
            if let Some(ref hb_config) = config.heartbeat {
                if hb_config.enabled {
                    let heartbeat = ryvos_agent::Heartbeat::new(
                        hb_config.clone(),
                        runtime.clone(),
                        event_bus.clone(),
                        cancel.clone(),
                        workspace.clone(),
                    );
                    tokio::spawn(async move {
                        heartbeat.run().await;
                    });
                    info!("Heartbeat started");
                }
            }

            // Optionally start the gateway server alongside the channel dispatcher
            if gateway {
                let gateway_config = config.gateway.clone().unwrap_or_default();
                info!(bind = %gateway_config.bind, "Starting gateway in daemon mode");
                let server = ryvos_gateway::GatewayServer::new(
                    gateway_config,
                    runtime.clone(),
                    event_bus.clone(),
                    store.clone(),
                    session_mgr.clone(),
                    broker.clone(),
                );
                let gateway_cancel = cancel.clone();
                tokio::spawn(async move {
                    if let Err(e) = server.run(gateway_cancel).await {
                        error!(error = %e, "Gateway server error");
                    }
                });
            }

            let mut dispatcher = ryvos_channels::ChannelDispatcher::new(runtime, event_bus, cancel);

            dispatcher.set_broker(broker.clone());

            if let Some(ref hooks_config) = config.hooks {
                dispatcher.set_hooks(hooks_config.clone());
            }

            if let Some(ref tg_config) = config.channels.telegram {
                let mut adapter =
                    ryvos_channels::TelegramAdapter::new(tg_config.clone(), session_mgr.clone());
                adapter.set_broker(broker.clone());
                dispatcher.add_adapter(std::sync::Arc::new(adapter));
            }

            if let Some(ref dc_config) = config.channels.discord {
                let mut adapter =
                    ryvos_channels::DiscordAdapter::new(dc_config.clone(), session_mgr.clone());
                adapter.set_broker(broker.clone());
                dispatcher.add_adapter(std::sync::Arc::new(adapter));
            }

            if let Some(ref slack_config) = config.channels.slack {
                let mut adapter =
                    ryvos_channels::SlackAdapter::new(slack_config.clone(), session_mgr.clone());
                adapter.set_broker(broker.clone());
                dispatcher.add_adapter(std::sync::Arc::new(adapter));
            }

            dispatcher.run().await?;
        }
        Some(Commands::Init { .. }) => unreachable!("handled before config load"),
        Some(Commands::Completions { .. }) => unreachable!("handled before config load"),
        Some(Commands::Mcp { .. }) => unreachable!("handled before config load"),
        Some(Commands::Skill { .. }) => unreachable!("handled before config load"),
        Some(Commands::Soul) => unreachable!("handled before config load"),
        Some(Commands::Repl) | None => {
            run_repl(
                &runtime,
                &event_bus,
                &session_id,
                &config,
                &tools,
                &broker,
                &mcp_manager,
            )
            .await?;
        }
    }

    // Disconnect MCP servers on shutdown
    if let Some(manager) = mcp_manager {
        manager.disconnect_all().await;
    }

    Ok(())
}

async fn run_once(
    runtime: &AgentRuntime,
    event_bus: &EventBus,
    session_id: &SessionId,
    input: &str,
    hooks: &Option<HooksConfig>,
    broker: &Arc<ApprovalBroker>,
) -> anyhow::Result<()> {
    // Fire on_message hook
    if let Some(hooks) = hooks {
        ryvos_core::hooks::run_hooks(
            &hooks.on_message,
            &[("RYVOS_SESSION", &session_id.0), ("RYVOS_TEXT", input)],
        )
        .await;
    }

    // Subscribe to events for output
    let mut rx = event_bus.subscribe();

    // Clone on_tool_call commands for the event printer task
    let on_tool_call_cmds = hooks
        .as_ref()
        .map(|h| h.on_tool_call.clone())
        .unwrap_or_default();
    let session_id_str = session_id.0.clone();
    let broker_clone = broker.clone();

    // Spawn event printer
    let print_handle = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                AgentEvent::TextDelta(text) => {
                    print!("{}", text);
                    io::stdout().flush().ok();
                }
                AgentEvent::ToolStart { name, .. } => {
                    eprintln!("\n[tool: {}]", name);
                    if !on_tool_call_cmds.is_empty() {
                        let cmds = on_tool_call_cmds.clone();
                        let sid = session_id_str.clone();
                        let tool = name.clone();
                        tokio::spawn(async move {
                            ryvos_core::hooks::run_hooks(
                                &cmds,
                                &[("RYVOS_SESSION", &sid), ("RYVOS_TOOL", &tool)],
                            )
                            .await;
                        });
                    }
                }
                AgentEvent::ToolEnd { name, result } => {
                    if result.is_error {
                        eprintln!("[{}: ERROR] {}", name, truncate(&result.content, 200));
                    } else {
                        eprintln!("[{}: ok] {}", name, truncate(&result.content, 200));
                    }
                }
                AgentEvent::ApprovalRequested { request } => {
                    eprintln!(
                        "\n[APPROVAL] {} ({}): \"{}\"",
                        request.tool_name, request.tier, request.input_summary
                    );
                    let broker = broker_clone.clone();
                    let req_id = request.id.clone();
                    tokio::task::spawn_blocking(move || {
                        let approved = dialoguer::Confirm::new()
                            .with_prompt("Allow?")
                            .default(true)
                            .interact()
                            .unwrap_or(false);
                        (approved, broker, req_id)
                    })
                    .await
                    .map(|(approved, broker, req_id)| {
                        let decision = if approved {
                            ApprovalDecision::Approved
                        } else {
                            ApprovalDecision::Denied {
                                reason: "denied by user".into(),
                            }
                        };
                        tokio::spawn(async move {
                            broker.respond(&req_id, decision).await;
                        });
                    })
                    .ok();
                }
                AgentEvent::ToolBlocked { name, tier, reason } => {
                    eprintln!("\n[BLOCKED] {} ({}): {}", name, tier, reason);
                }
                AgentEvent::RunComplete {
                    total_turns,
                    input_tokens,
                    output_tokens,
                    ..
                } => {
                    eprintln!(
                        "\n[done: {} turns, {}in/{}out tokens]",
                        total_turns, input_tokens, output_tokens
                    );
                    break;
                }
                AgentEvent::RunError { error } => {
                    eprintln!("\n[error: {}]", error);
                    break;
                }
                AgentEvent::GuardianStall {
                    elapsed_secs, turn, ..
                } => {
                    eprintln!(
                        "\n[GUARDIAN] Stall detected: {}s at turn {}",
                        elapsed_secs, turn
                    );
                }
                AgentEvent::GuardianDoomLoop {
                    tool_name,
                    consecutive_calls,
                    ..
                } => {
                    eprintln!(
                        "\n[GUARDIAN] Doom loop: {} x{}",
                        tool_name, consecutive_calls
                    );
                }
                AgentEvent::GuardianBudgetAlert {
                    used_tokens,
                    budget_tokens,
                    is_hard_stop,
                    ..
                } => {
                    let kind = if is_hard_stop { "HARD STOP" } else { "warning" };
                    eprintln!(
                        "\n[GUARDIAN] Budget {}: {}/{} tokens",
                        kind, used_tokens, budget_tokens
                    );
                }
                AgentEvent::GoalEvaluated { evaluation, .. } => {
                    let status = if evaluation.passed {
                        "PASSED"
                    } else {
                        "FAILED"
                    };
                    eprintln!(
                        "\n[GOAL {}] score: {:.0}%",
                        status,
                        evaluation.overall_score * 100.0
                    );
                }
                AgentEvent::JudgeVerdict { verdict, .. } => {
                    let text = match &verdict {
                        ryvos_core::types::Verdict::Accept { confidence } => {
                            format!("[JUDGE] Accepted (confidence: {:.0}%)", confidence * 100.0)
                        }
                        ryvos_core::types::Verdict::Retry { reason, .. } => {
                            format!("[JUDGE] Retry: {}", reason)
                        }
                        ryvos_core::types::Verdict::Escalate { reason } => {
                            format!("[JUDGE] Escalated: {}", reason)
                        }
                        ryvos_core::types::Verdict::Continue => "[JUDGE] Continue".to_string(),
                    };
                    eprintln!("\n{}", text);
                }
                AgentEvent::GuardianHint { .. }
                | AgentEvent::UsageUpdate { .. }
                | AgentEvent::DecisionMade { .. } => {}
                _ => {}
            }
        }
    });

    match runtime.run(session_id, input).await {
        Ok(_) => {}
        Err(e) => {
            error!(error = %e, "Agent run failed");
            event_bus.publish(AgentEvent::RunError {
                error: e.to_string(),
            });
        }
    }

    // Fire on_response hook
    if let Some(hooks) = hooks {
        ryvos_core::hooks::run_hooks(&hooks.on_response, &[("RYVOS_SESSION", &session_id.0)]).await;
    }

    println!();
    print_handle.abort();
    Ok(())
}

pub(crate) async fn run_repl(
    runtime: &AgentRuntime,
    event_bus: &EventBus,
    session_id: &SessionId,
    config: &AppConfig,
    tools: &Arc<tokio::sync::RwLock<ToolRegistry>>,
    broker: &Arc<ApprovalBroker>,
    mcp_manager: &Option<Arc<ryvos_mcp::McpClientManager>>,
) -> anyhow::Result<()> {
    println!("Ryvos v{}", env!("CARGO_PKG_VERSION"));
    println!("Session: {}", session_id);
    println!(
        "Security: auto-approve up to {}",
        config.security.auto_approve_up_to
    );
    println!("Type /help for commands, /quit to exit.\n");

    // Fire on_start hook
    if let Some(ref hooks) = config.hooks {
        ryvos_core::hooks::run_hooks(&hooks.on_start, &[("RYVOS_SESSION", &session_id.0)]).await;
    }

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut total_input: u64 = 0;
    let mut total_output: u64 = 0;
    let mut session_thinking = config.model.thinking.clone();
    let mut _force_compact = false;

    loop {
        print!("> ");
        stdout.flush()?;

        let mut input = String::new();
        if stdin.lock().read_line(&mut input)? == 0 {
            break; // EOF
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        let parts: Vec<&str> = input.split_whitespace().collect();

        match parts[0] {
            "/quit" | "/exit" | "/q" => {
                println!("Goodbye!");
                break;
            }
            "/clear" => {
                println!("Session cleared. (Note: history persists in DB)");
                continue;
            }
            "/session" => {
                println!("Session ID: {}", session_id);
                continue;
            }
            "/status" => {
                let tool_list = tools
                    .read()
                    .await
                    .list()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
                println!("Session: {}", session_id);
                println!(
                    "Model: {} ({})",
                    config.model.model_id, config.model.provider
                );
                println!("Thinking: {:?}", session_thinking);
                println!("Tools: {}", tool_list.join(", "));
                if let Some(ref mgr) = mcp_manager {
                    let servers = mgr.connected_servers().await;
                    if servers.is_empty() {
                        println!("MCP: no servers connected");
                    } else {
                        println!(
                            "MCP: {} server(s) connected ({})",
                            servers.len(),
                            servers.join(", ")
                        );
                    }
                } else {
                    println!("MCP: none configured");
                }
                continue;
            }
            "/usage" => {
                println!(
                    "Session tokens -- Input: {}, Output: {}",
                    total_input, total_output
                );
                continue;
            }
            "/tools" => {
                let tool_list = tools
                    .read()
                    .await
                    .list()
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
                for name in &tool_list {
                    println!("  - {}", name);
                }
                continue;
            }
            "/think" => {
                session_thinking = match parts.get(1).copied() {
                    None | Some("off") => ThinkingLevel::Off,
                    Some("low") => ThinkingLevel::Low,
                    Some("medium") | Some("hard") => ThinkingLevel::Medium,
                    Some("high") => ThinkingLevel::High,
                    Some(other) => {
                        println!(
                            "Unknown thinking level: {}. Use off/low/medium/high.",
                            other
                        );
                        continue;
                    }
                };
                println!("Thinking level: {:?}", session_thinking);
                continue;
            }
            "/compact" => {
                println!("Context will be compacted on next message.");
                _force_compact = true;
                continue;
            }
            "/security" => {
                println!("Security Policy:");
                println!(
                    "  Auto-approve up to: {}",
                    config.security.auto_approve_up_to
                );
                if let Some(deny) = config.security.deny_above {
                    println!("  Deny above: {}", deny);
                } else {
                    println!("  Deny above: (none)");
                }
                println!(
                    "  Approval timeout: {}s",
                    config.security.approval_timeout_secs
                );
                if !config.security.tool_overrides.is_empty() {
                    println!("  Tool overrides:");
                    for (tool, tier) in &config.security.tool_overrides {
                        println!("    {} -> {}", tool, tier);
                    }
                }
                let pending = broker.pending_requests().await;
                if pending.is_empty() {
                    println!("  Pending approvals: none");
                } else {
                    println!("  Pending approvals:");
                    for req in &pending {
                        println!(
                            "    [{}] {} ({}): \"{}\"",
                            &req.id[..8],
                            req.tool_name,
                            req.tier,
                            req.input_summary
                        );
                    }
                }
                continue;
            }
            "/approve" => {
                if let Some(prefix) = parts.get(1) {
                    if let Some(full_id) = broker.find_by_prefix(prefix).await {
                        if broker.respond(&full_id, ApprovalDecision::Approved).await {
                            println!("Approved: {}", &full_id[..8]);
                        } else {
                            println!("Request not found (may have timed out).");
                        }
                    } else {
                        println!("No pending request matching '{}'.", prefix);
                    }
                } else {
                    println!("Usage: /approve <id-prefix>");
                }
                continue;
            }
            "/deny" => {
                if let Some(prefix) = parts.get(1) {
                    let reason = if parts.len() > 2 {
                        parts[2..].join(" ")
                    } else {
                        "denied by user".to_string()
                    };
                    if let Some(full_id) = broker.find_by_prefix(prefix).await {
                        if broker
                            .respond(&full_id, ApprovalDecision::Denied { reason })
                            .await
                        {
                            println!("Denied: {}", &full_id[..8]);
                        } else {
                            println!("Request not found (may have timed out).");
                        }
                    } else {
                        println!("No pending request matching '{}'.", prefix);
                    }
                } else {
                    println!("Usage: /deny <id-prefix> [reason]");
                }
                continue;
            }
            "/mcp" => {
                handle_mcp_repl(parts.get(1..).unwrap_or(&[]), mcp_manager, tools).await;
                continue;
            }
            "/prompts" => {
                if let Some(ref mgr) = mcp_manager {
                    for server in mgr.connected_servers().await {
                        match mgr.list_prompts(&server).await {
                            Ok(prompts) => {
                                if prompts.is_empty() {
                                    println!("  {}: (no prompts)", server);
                                } else {
                                    for p in &prompts {
                                        let desc = p.description.as_deref().unwrap_or("");
                                        println!("  /mcp__{}__{}  {}", server, p.name, desc);
                                    }
                                }
                            }
                            Err(e) => println!("  {}: error: {}", server, e),
                        }
                    }
                } else {
                    println!("No MCP servers configured.");
                }
                continue;
            }
            "/soul" => {
                let workspace = config.workspace_dir();
                if let Err(e) = onboard::run_soul_interview(&workspace) {
                    eprintln!("Soul interview failed: {e}");
                }
                continue;
            }
            "/help" => {
                println!("Commands:");
                println!("  /quit       Exit");
                println!("  /clear      Reset session context");
                println!("  /session    Show session ID");
                println!("  /status     Show agent status");
                println!("  /usage      Show token usage");
                println!("  /tools      List available tools");
                println!("  /think [level]  Set thinking level (off/low/medium/high)");
                println!("  /compact    Force context compaction");
                println!("  /security   Show security policy and pending approvals");
                println!("  /approve <id>   Approve a pending tool call");
                println!("  /deny <id> [reason]  Deny a pending tool call");
                println!("  /mcp            Show MCP status");
                println!("  /mcp list       List configured servers");
                println!("  /mcp connect <name>   Connect to a server");
                println!("  /mcp disconnect <name>  Disconnect from a server");
                println!("  /mcp resources [server]  List MCP resources");
                println!("  /mcp prompts [server]  List MCP prompts");
                println!("  /mcp tools [server]  List MCP tools");
                println!("  /prompts    List all MCP prompts");
                println!("  /soul       Personalize your agent");
                continue;
            }
            _ if input.starts_with("/mcp__") => {
                // MCP prompt invocation: /mcp__server__prompt_name
                if let Some(ref mgr) = mcp_manager {
                    let stripped = &input[6..]; // remove "/mcp__"
                    if let Some(sep) = stripped.find("__") {
                        let server = &stripped[..sep];
                        let prompt_name = &stripped[sep + 2..];
                        match mgr.get_prompt(server, prompt_name, None).await {
                            Ok(messages) => {
                                for msg in &messages {
                                    let role = match msg.role {
                                        ryvos_mcp::PromptMessageRole::User => "user",
                                        ryvos_mcp::PromptMessageRole::Assistant => "assistant",
                                    };
                                    let text = match &msg.content {
                                        ryvos_mcp::PromptMessageContent::Text { text } => {
                                            text.clone()
                                        }
                                        _ => "[non-text content]".to_string(),
                                    };
                                    println!("[{}] {}", role, text);
                                }
                                // Inject the first user message as a prompt
                                let user_text: Vec<String> = messages
                                    .iter()
                                    .filter(|m| {
                                        matches!(m.role, ryvos_mcp::PromptMessageRole::User)
                                    })
                                    .filter_map(|m| match &m.content {
                                        ryvos_mcp::PromptMessageContent::Text { text } => {
                                            Some(text.clone())
                                        }
                                        _ => None,
                                    })
                                    .collect();
                                if !user_text.is_empty() {
                                    let combined = user_text.join("\n");
                                    println!("\nSending prompt to agent...\n");
                                    run_once(
                                        runtime,
                                        event_bus,
                                        session_id,
                                        &combined,
                                        &config.hooks,
                                        broker,
                                    )
                                    .await?;
                                }
                            }
                            Err(e) => println!("Failed to get prompt: {}", e),
                        }
                    } else {
                        println!("Invalid prompt format. Use /mcp__<server>__<prompt>");
                    }
                } else {
                    println!("No MCP servers configured.");
                }
                continue;
            }
            _ if input.starts_with('/') => {
                println!(
                    "Unknown command: {}. Type /help for available commands.",
                    parts[0]
                );
                continue;
            }
            _ => {}
        }

        // Subscribe to events to track token usage
        let mut rx = event_bus.subscribe();
        let usage_handle = tokio::spawn(async move {
            let mut inp = 0u64;
            let mut out = 0u64;
            while let Ok(event) = rx.recv().await {
                match event {
                    AgentEvent::RunComplete {
                        input_tokens,
                        output_tokens,
                        ..
                    } => {
                        inp = input_tokens;
                        out = output_tokens;
                        break;
                    }
                    AgentEvent::RunError { .. } => break,
                    _ => {}
                }
            }
            (inp, out)
        });

        run_once(runtime, event_bus, session_id, input, &config.hooks, broker).await?;

        if let Ok((inp, out)) = usage_handle.await {
            total_input += inp;
            total_output += out;
        }
    }

    Ok(())
}

/// Handle /mcp REPL commands.
async fn handle_mcp_repl(
    args: &[&str],
    mcp_manager: &Option<Arc<ryvos_mcp::McpClientManager>>,
    tools: &Arc<tokio::sync::RwLock<ToolRegistry>>,
) {
    let Some(ref mgr) = mcp_manager else {
        println!("No MCP servers configured.");
        return;
    };

    let subcommand = args.first().copied().unwrap_or("");

    match subcommand {
        "" | "status" => {
            let connected = mgr.connected_servers().await;
            if connected.is_empty() {
                println!("MCP: no servers connected");
            } else {
                println!("MCP Status:");
                for name in &connected {
                    let is_alive = mgr.is_connected(name).await;
                    let status = if is_alive { "connected" } else { "stale" };
                    let tool_count = tools
                        .read()
                        .await
                        .list()
                        .iter()
                        .filter(|t| t.starts_with(&format!("mcp__{}__", name)))
                        .count();
                    println!("  {} [{}] ({} tools)", name, status, tool_count);
                }
            }
        }
        "list" => {
            let connected = mgr.connected_servers().await;
            let configured = mgr.configured_servers().await;
            if configured.is_empty() && connected.is_empty() {
                println!("No MCP servers configured.");
            } else {
                println!("MCP Servers:");
                for name in &configured {
                    let status = if connected.contains(name) {
                        "connected"
                    } else {
                        "disconnected"
                    };
                    println!("  {} [{}]", name, status);
                }
            }
        }
        "connect" => {
            if let Some(name) = args.get(1) {
                println!("Reconnecting to {}...", name);
                match mgr.reconnect(name).await {
                    Ok(()) => {
                        // Refresh tools
                        let mut registry = tools.write().await;
                        match ryvos_mcp::refresh_tools(mgr, name, &mut registry).await {
                            Ok(count) => println!("Connected to {} ({} tools)", name, count),
                            Err(e) => println!("Connected but failed to register tools: {}", e),
                        }
                    }
                    Err(e) => println!("Failed to connect: {}", e),
                }
            } else {
                println!("Usage: /mcp connect <server-name>");
            }
        }
        "disconnect" => {
            if let Some(name) = args.get(1) {
                mgr.disconnect(name).await;
                // Unregister tools
                let prefix = format!("mcp__{}__", name);
                let mut registry = tools.write().await;
                let to_remove: Vec<String> = registry
                    .list()
                    .into_iter()
                    .filter(|t| t.starts_with(&prefix))
                    .map(|s| s.to_string())
                    .collect();
                for t in &to_remove {
                    registry.unregister(t);
                }
                println!(
                    "Disconnected from {} ({} tools removed)",
                    name,
                    to_remove.len()
                );
            } else {
                println!("Usage: /mcp disconnect <server-name>");
            }
        }
        "resources" => {
            let server_filter = args.get(1).copied();
            let servers = mgr.connected_servers().await;
            for server in &servers {
                if let Some(filter) = server_filter {
                    if server != filter {
                        continue;
                    }
                }
                match mgr.list_resources(server).await {
                    Ok(resources) => {
                        if resources.is_empty() {
                            println!("  {}: (no resources)", server);
                        } else {
                            println!("  {}:", server);
                            for r in &resources {
                                let desc = r.description.as_deref().unwrap_or("");
                                println!("    {} - {} {}", r.uri, r.name, desc);
                            }
                        }
                    }
                    Err(e) => println!("  {}: error: {}", server, e),
                }
            }
        }
        "prompts" => {
            let server_filter = args.get(1).copied();
            let servers = mgr.connected_servers().await;
            for server in &servers {
                if let Some(filter) = server_filter {
                    if server != filter {
                        continue;
                    }
                }
                match mgr.list_prompts(server).await {
                    Ok(prompts) => {
                        if prompts.is_empty() {
                            println!("  {}: (no prompts)", server);
                        } else {
                            println!("  {}:", server);
                            for p in &prompts {
                                let desc = p.description.as_deref().unwrap_or("");
                                println!("    /mcp__{}__{}  {}", server, p.name, desc);
                            }
                        }
                    }
                    Err(e) => println!("  {}: error: {}", server, e),
                }
            }
        }
        "tools" => {
            let server_filter = args.get(1).copied();
            let registry = tools.read().await;
            let all_tools = registry.list();
            let mcp_tools: Vec<&str> = all_tools
                .into_iter()
                .filter(|t| t.starts_with("mcp__"))
                .filter(|t| {
                    if let Some(filter) = server_filter {
                        t.starts_with(&format!("mcp__{}__", filter))
                    } else {
                        true
                    }
                })
                .collect();
            if mcp_tools.is_empty() {
                println!("No MCP tools registered.");
            } else {
                for t in mcp_tools {
                    println!("  {}", t);
                }
            }
        }
        _ => {
            println!("Unknown MCP command: {}", subcommand);
            println!("Usage: /mcp [status|list|connect|disconnect|resources|prompts|tools]");
        }
    }
}

/// Handle `ryvos mcp` CLI subcommands.
fn handle_mcp_cli(action: &McpAction, config_path: &PathBuf) -> anyhow::Result<()> {
    let config_path = if config_path == &PathBuf::from("ryvos.toml") && !config_path.exists() {
        dirs_home()
            .map(|h| h.join(".ryvos").join("config.toml"))
            .unwrap_or_else(|| config_path.clone())
    } else {
        config_path.clone()
    };

    match action {
        McpAction::List => {
            if config_path.exists() {
                let config = AppConfig::load(&config_path)?;
                if let Some(mcp) = &config.mcp {
                    if mcp.servers.is_empty() {
                        println!("No MCP servers configured.");
                    } else {
                        println!("MCP Servers:");
                        for (name, server) in &mcp.servers {
                            let transport = match &server.transport {
                                ryvos_core::config::McpTransport::Stdio {
                                    command, args, ..
                                } => {
                                    format!("stdio: {} {}", command, args.join(" "))
                                }
                                ryvos_core::config::McpTransport::Sse { url } => {
                                    format!("sse: {}", url)
                                }
                            };
                            let auto = if server.auto_connect {
                                "auto"
                            } else {
                                "manual"
                            };
                            println!("  {} [{}] ({})", name, auto, transport);
                        }
                    }
                } else {
                    println!("No MCP configuration found.");
                }
            } else {
                println!("Config file not found: {}", config_path.display());
            }
        }
        McpAction::Add {
            name,
            command,
            args,
            url,
            env,
        } => {
            let transport_section = if let Some(cmd) = command {
                let args_str = if args.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\nargs = [{}]",
                        args.iter()
                            .map(|a| format!("\"{}\"", a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                let env_str = if env.is_empty() {
                    String::new()
                } else {
                    let pairs: Vec<String> = env
                        .iter()
                        .filter_map(|e| {
                            let (k, v) = e.split_once('=')?;
                            Some(format!("{} = \"{}\"", k, v))
                        })
                        .collect();
                    if pairs.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "\n\n[mcp.servers.{}.transport.env]\n{}",
                            name,
                            pairs.join("\n")
                        )
                    }
                };
                format!(
                    "\n[mcp.servers.{name}]\nauto_connect = true\n\n[mcp.servers.{name}.transport]\ntype = \"stdio\"\ncommand = \"{cmd}\"{args_str}{env_str}\n",
                )
            } else if let Some(url) = url {
                format!(
                    "\n[mcp.servers.{name}]\nauto_connect = true\n\n[mcp.servers.{name}.transport]\ntype = \"sse\"\nurl = \"{url}\"\n",
                )
            } else {
                eprintln!("Error: either --command or --url must be specified");
                return Ok(());
            };

            // Append to config file
            if config_path.exists() {
                let mut content = std::fs::read_to_string(&config_path)?;
                content.push_str(&transport_section);
                std::fs::write(&config_path, content)?;
            } else {
                // Create minimal config
                std::fs::create_dir_all(config_path.parent().unwrap_or(&PathBuf::from(".")))?;
                std::fs::write(&config_path, &transport_section)?;
            }
            println!("Added MCP server '{}' to {}", name, config_path.display());
        }
        McpAction::Remove { name } => {
            if config_path.exists() {
                let content = std::fs::read_to_string(&config_path)?;
                // Simple removal: filter out lines matching this server section
                // For a production implementation, use a TOML library for precise editing
                let section_header = format!("[mcp.servers.{}]", name);
                let transport_header = format!("[mcp.servers.{}.transport]", name);
                let env_header = format!("[mcp.servers.{}.transport.env]", name);

                let mut result = Vec::new();
                let mut skip = false;
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed == section_header
                        || trimmed == transport_header
                        || trimmed == env_header
                    {
                        skip = true;
                        continue;
                    }
                    if skip && trimmed.starts_with('[') {
                        skip = false;
                    }
                    if !skip {
                        result.push(line);
                    }
                }
                std::fs::write(&config_path, result.join("\n"))?;
                println!(
                    "Removed MCP server '{}' from {}",
                    name,
                    config_path.display()
                );
            } else {
                println!("Config file not found: {}", config_path.display());
            }
        }
    }
    Ok(())
}

/// Handle `ryvos skill` CLI subcommands.
async fn handle_skill_cli(action: &SkillAction) -> anyhow::Result<()> {
    let home = dirs_home().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let skills_dir = home.join(".ryvos").join("skills");

    match action {
        SkillAction::List { remote } => {
            // List local skills
            let installed = ryvos_skills::registry::list_installed(&skills_dir);
            if installed.is_empty() {
                println!("No skills installed.");
            } else {
                println!("Installed skills:");
                for name in &installed {
                    println!("  - {}", name);
                }
            }

            if *remote {
                let registry_url = ryvos_core::config::RegistryConfig::default().url;
                match ryvos_skills::registry::fetch_index(&registry_url).await {
                    Ok(index) => {
                        println!("\nAvailable in registry ({} skills):", index.skills.len());
                        for entry in &index.skills {
                            let installed_marker = if installed.contains(&entry.name) {
                                " [installed]"
                            } else {
                                ""
                            };
                            println!(
                                "  - {} v{}: {}{}",
                                entry.name, entry.version, entry.description, installed_marker
                            );
                        }
                    }
                    Err(e) => eprintln!("Failed to fetch registry: {}", e),
                }
            }
        }
        SkillAction::Search { query } => {
            let registry_url = ryvos_core::config::RegistryConfig::default().url;
            match ryvos_skills::registry::fetch_index(&registry_url).await {
                Ok(index) => {
                    let results = ryvos_skills::registry::search_skills(&index, query);
                    if results.is_empty() {
                        println!("No skills found matching '{}'", query);
                    } else {
                        println!("Skills matching '{}':", query);
                        for entry in results {
                            println!(
                                "  - {} v{}: {}",
                                entry.name, entry.version, entry.description
                            );
                        }
                    }
                }
                Err(e) => eprintln!("Failed to fetch registry: {}", e),
            }
        }
        SkillAction::Install { name } => {
            let registry_url = ryvos_core::config::RegistryConfig::default().url;
            match ryvos_skills::registry::fetch_index(&registry_url).await {
                Ok(index) => {
                    if let Some(entry) = index.skills.iter().find(|e| e.name == *name) {
                        match ryvos_skills::registry::install_skill(entry, &skills_dir).await {
                            Ok(_path) => println!("Installed skill '{}'", name),
                            Err(e) => eprintln!("Failed to install skill '{}': {}", name, e),
                        }
                    } else {
                        eprintln!("Skill '{}' not found in registry", name);
                    }
                }
                Err(e) => eprintln!("Failed to fetch registry: {}", e),
            }
        }
        SkillAction::Remove { name } => {
            match ryvos_skills::registry::remove_skill(name, &skills_dir) {
                Ok(()) => println!("Removed skill '{}'", name),
                Err(e) => eprintln!("Failed to remove skill '{}': {}", name, e),
            }
        }
    }
    Ok(())
}

/// Load .mcp.json from the current working directory.
fn load_mcp_json() -> Option<McpJsonConfig> {
    let path = std::env::current_dir().ok()?.join(".mcp.json");
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&content) {
        Ok(config) => {
            info!(path = %path.display(), "Loaded .mcp.json project config");
            Some(config)
        }
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Failed to parse .mcp.json");
            None
        }
    }
}

fn create_env_config() -> anyhow::Result<AppConfig> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").ok();
    let openai_key = std::env::var("OPENAI_API_KEY").ok();

    let (provider, model_id, key) = if let Some(key) = api_key {
        (
            "anthropic".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            Some(key),
        )
    } else if let Some(key) = openai_key {
        ("openai".to_string(), "gpt-4o".to_string(), Some(key))
    } else {
        // Default to Ollama (local)
        ("ollama".to_string(), "llama3.2".to_string(), None)
    };

    let mut model = ryvos_core::config::ModelConfig {
        provider,
        model_id,
        api_key: key,
        base_url: if std::env::var("ANTHROPIC_API_KEY").is_err()
            && std::env::var("OPENAI_API_KEY").is_err()
        {
            Some("http://localhost:11434/v1/chat/completions".to_string())
        } else {
            None
        },
        max_tokens: 8192,
        temperature: 0.0,
        thinking: ThinkingLevel::Off,
        retry: None,
        azure_resource: None,
        azure_deployment: None,
        azure_api_version: None,
        aws_region: None,
        extra_headers: Default::default(),
    };
    ryvos_llm::apply_preset_defaults(&mut model);

    Ok(AppConfig {
        agent: Default::default(),
        model,
        fallback_models: vec![],
        gateway: None,
        channels: Default::default(),
        mcp: None,
        hooks: None,
        wizard: None,
        cron: None,
        heartbeat: None,
        web_search: None,
        security: Default::default(),
        embedding: None,
        daily_logs: None,
        registry: None,
    })
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
