use std::str::FromStr;

use ryvos_core::config::AppConfig;

struct CheckResult {
    label: String,
    ok: bool,
    detail: String,
}

pub fn run_doctor(config: &AppConfig) {
    let mut checks = Vec::new();

    // 1. API key configured for primary model
    checks.push(check_api_key(config));

    // 2. Workspace dir exists and writable
    checks.push(check_workspace(config));

    // 3. SQLite DB accessible
    checks.push(check_database(config));

    // 4. Channel tokens non-empty
    checks.push(check_channels(config));

    // 5. Cron expressions parseable
    checks.push(check_cron(config));

    // 6. MCP server configs valid
    checks.push(check_mcp(config));

    // 7. Security policy consistency
    checks.push(check_security(config));

    // Print results
    let mut ok_count = 0;
    let mut fail_count = 0;

    for check in &checks {
        let icon = if check.ok { "[OK]" } else { "[!!]" };
        println!("  {} {}: {}", icon, check.label, check.detail);
        if check.ok {
            ok_count += 1;
        } else {
            fail_count += 1;
        }
    }

    println!();
    println!("  {} passed, {} issues found", ok_count, fail_count);
}

fn check_api_key(config: &AppConfig) -> CheckResult {
    let has_key = config.model.api_key.as_ref().map_or(false, |k| {
        !k.is_empty() && !k.starts_with("${")
    });
    let provider = &config.model.provider;
    let needs_key = provider != "ollama";

    if !needs_key || has_key {
        CheckResult {
            label: "API Key".into(),
            ok: true,
            detail: format!("Configured for {} ({})", config.model.model_id, provider),
        }
    } else {
        CheckResult {
            label: "API Key".into(),
            ok: false,
            detail: format!("No API key set for provider '{}'", provider),
        }
    }
}

fn check_workspace(config: &AppConfig) -> CheckResult {
    let ws = config.workspace_dir();
    if ws.exists() && ws.is_dir() {
        // Check writable by attempting to create a temp file
        let test_file = ws.join(".doctor_test");
        match std::fs::write(&test_file, "test") {
            Ok(_) => {
                std::fs::remove_file(&test_file).ok();
                CheckResult {
                    label: "Workspace".into(),
                    ok: true,
                    detail: format!("{}", ws.display()),
                }
            }
            Err(e) => CheckResult {
                label: "Workspace".into(),
                ok: false,
                detail: format!("{} (not writable: {})", ws.display(), e),
            },
        }
    } else {
        CheckResult {
            label: "Workspace".into(),
            ok: false,
            detail: format!("{} (does not exist)", ws.display()),
        }
    }
}

fn check_database(config: &AppConfig) -> CheckResult {
    let ws = config.workspace_dir();
    let db_path = ws.join("sessions.db");
    match ryvos_memory::SqliteStore::open(&db_path) {
        Ok(_) => CheckResult {
            label: "Database".into(),
            ok: true,
            detail: format!("{}", db_path.display()),
        },
        Err(e) => CheckResult {
            label: "Database".into(),
            ok: false,
            detail: format!("{}: {}", db_path.display(), e),
        },
    }
}

fn check_channels(config: &AppConfig) -> CheckResult {
    let mut issues = Vec::new();
    let mut configured = Vec::new();

    if let Some(ref tg) = config.channels.telegram {
        if tg.bot_token.is_empty() {
            issues.push("telegram: empty bot_token");
        } else {
            configured.push("telegram");
        }
    }
    if let Some(ref dc) = config.channels.discord {
        if dc.bot_token.is_empty() {
            issues.push("discord: empty bot_token");
        } else {
            configured.push("discord");
        }
    }
    if let Some(ref slack) = config.channels.slack {
        if slack.bot_token.is_empty() {
            issues.push("slack: empty bot_token");
        } else if slack.app_token.is_empty() {
            issues.push("slack: empty app_token");
        } else {
            configured.push("slack");
        }
    }

    if !issues.is_empty() {
        CheckResult {
            label: "Channels".into(),
            ok: false,
            detail: issues.join(", "),
        }
    } else if configured.is_empty() {
        CheckResult {
            label: "Channels".into(),
            ok: true,
            detail: "None configured (REPL only)".into(),
        }
    } else {
        CheckResult {
            label: "Channels".into(),
            ok: true,
            detail: configured.join(", "),
        }
    }
}

fn check_cron(config: &AppConfig) -> CheckResult {
    if let Some(ref cron_config) = config.cron {
        let mut bad = Vec::new();
        for job in &cron_config.jobs {
            if cron::Schedule::from_str(&job.schedule).is_err() {
                bad.push(format!("'{}' ({})", job.name, job.schedule));
            }
        }
        if bad.is_empty() {
            CheckResult {
                label: "Cron".into(),
                ok: true,
                detail: format!("{} jobs configured", cron_config.jobs.len()),
            }
        } else {
            CheckResult {
                label: "Cron".into(),
                ok: false,
                detail: format!("Invalid schedules: {}", bad.join(", ")),
            }
        }
    } else {
        CheckResult {
            label: "Cron".into(),
            ok: true,
            detail: "Not configured".into(),
        }
    }
}

fn check_mcp(config: &AppConfig) -> CheckResult {
    if let Some(ref mcp) = config.mcp {
        let count = mcp.servers.len();
        let auto = mcp.servers.values().filter(|s| s.auto_connect).count();
        CheckResult {
            label: "MCP".into(),
            ok: true,
            detail: format!("{} servers ({} auto-connect)", count, auto),
        }
    } else {
        CheckResult {
            label: "MCP".into(),
            ok: true,
            detail: "Not configured".into(),
        }
    }
}

fn check_security(config: &AppConfig) -> CheckResult {
    let auto = config.security.auto_approve_up_to;
    let deny = config.security.deny_above;

    if let Some(deny_tier) = deny {
        if auto >= deny_tier {
            return CheckResult {
                label: "Security".into(),
                ok: false,
                detail: format!(
                    "auto_approve_up_to ({}) >= deny_above ({}) — inconsistent",
                    auto, deny_tier
                ),
            };
        }
    }

    let deny_str = deny
        .map(|d| format!("deny above {}", d))
        .unwrap_or_else(|| "no deny limit".into());

    CheckResult {
        label: "Security".into(),
        ok: true,
        detail: format!("auto-approve up to {}, {}", auto, deny_str),
    }
}
