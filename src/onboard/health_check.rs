use ryvos_core::config::AppConfig;

pub fn run(config: &AppConfig) {
    println!();
    println!("  \x1b[1;36mHealth Check\x1b[0m");
    println!();

    let mut warnings = 0;

    // Check API key
    if let Some(ref key) = config.model.api_key {
        if key.is_empty() || key == "YOUR_API_KEY" || key == "sk-placeholder" {
            warn("API key appears to be a placeholder");
            warnings += 1;
        } else {
            ok("API key configured");
        }
    } else if config.model.provider != "ollama" && config.model.provider != "bedrock" {
        warn("No API key set — agent may not be able to call the LLM");
        warnings += 1;
    } else {
        ok("API key not required for this provider");
    }

    // Check channel tokens
    if let Some(ref tg) = config.channels.telegram {
        if tg.bot_token.len() < 20 && !tg.bot_token.starts_with("${") {
            warn("Telegram token looks too short");
            warnings += 1;
        } else {
            ok("Telegram configured");
        }
    }

    if let Some(ref dc) = config.channels.discord {
        if dc.bot_token.len() < 20 && !dc.bot_token.starts_with("${") {
            warn("Discord token looks too short");
            warnings += 1;
        } else {
            ok("Discord configured");
        }
    }

    if let Some(ref sl) = config.channels.slack {
        if !sl.bot_token.starts_with("xoxb-") && !sl.bot_token.starts_with("${") {
            warn("Slack bot token should start with 'xoxb-'");
            warnings += 1;
        } else {
            ok("Slack configured");
        }
    }

    if config.channels.whatsapp.is_some() {
        ok("WhatsApp configured");
    }

    // Check npx availability for MCP servers
    if config.mcp.is_some() {
        match std::process::Command::new("which").arg("npx").output() {
            Ok(output) if output.status.success() => {
                ok("npx found on PATH (needed for MCP servers)");
            }
            _ => {
                warn("npx not found — MCP servers using npx will fail. Install Node.js.");
                warnings += 1;
            }
        }
    }

    // Check gateway port
    if let Some(ref gw) = config.gateway {
        if let Some(port_str) = gw.bind.split(':').last() {
            if let Ok(port) = port_str.parse::<u16>() {
                match std::net::TcpListener::bind(("127.0.0.1", port)) {
                    Ok(_) => ok(&format!("Gateway port {} available", port)),
                    Err(_) => {
                        warn(&format!("Gateway port {} may already be in use", port));
                        warnings += 1;
                    }
                }
            }
        }
    }

    // Summary
    println!();
    if warnings == 0 {
        println!("  \x1b[1;32mAll checks passed.\x1b[0m");
    } else {
        println!(
            "  \x1b[1;33m{} warning{} — review above before launching.\x1b[0m",
            warnings,
            if warnings == 1 { "" } else { "s" }
        );
    }
}

fn ok(msg: &str) {
    println!("  \x1b[32m✓\x1b[0m {msg}");
}

fn warn(msg: &str) {
    println!("  \x1b[33m⚠\x1b[0m {msg}");
}
