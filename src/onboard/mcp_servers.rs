use std::collections::HashMap;

use anyhow::Result;
use dialoguer::{Input, MultiSelect, Password};
use ryvos_core::config::{McpServerConfig, McpTransport};

struct McpTemplate {
    label: &'static str,
    name: &'static str,
    package: &'static str,
    env_vars: &'static [&'static str],
}

const TEMPLATES: &[McpTemplate] = &[
    McpTemplate {
        label: "Filesystem — read/write local files",
        name: "filesystem",
        package: "@anthropic/mcp-server-filesystem",
        env_vars: &[],
    },
    McpTemplate {
        label: "GitHub — repos, issues, PRs",
        name: "github",
        package: "@anthropic/mcp-server-github",
        env_vars: &["GITHUB_TOKEN"],
    },
    McpTemplate {
        label: "Puppeteer — browser automation",
        name: "puppeteer",
        package: "@anthropic/mcp-server-puppeteer",
        env_vars: &[],
    },
    McpTemplate {
        label: "Memory — persistent key-value store",
        name: "memory",
        package: "@anthropic/mcp-server-memory",
        env_vars: &[],
    },
    McpTemplate {
        label: "Fetch — HTTP requests",
        name: "fetch",
        package: "@anthropic/mcp-server-fetch",
        env_vars: &[],
    },
    McpTemplate {
        label: "Postgres — query a database",
        name: "postgres",
        package: "@anthropic/mcp-server-postgres",
        env_vars: &["DATABASE_URL"],
    },
    McpTemplate {
        label: "Slack — workspace integration",
        name: "slack",
        package: "@anthropic/mcp-server-slack",
        env_vars: &["SLACK_BOT_TOKEN"],
    },
    McpTemplate {
        label: "Gmail / Google — custom command",
        name: "google",
        package: "",
        env_vars: &[],
    },
];

pub fn configure() -> Result<HashMap<String, McpServerConfig>> {
    let labels: Vec<&str> = TEMPLATES.iter().map(|t| t.label).collect();

    let selections = MultiSelect::new()
        .with_prompt("Select MCP server integrations (space to toggle, enter to confirm)")
        .items(&labels)
        .interact()?;

    let mut servers = HashMap::new();

    for idx in selections {
        let template = &TEMPLATES[idx];
        let config = configure_server(template)?;
        servers.insert(template.name.to_string(), config);
    }

    Ok(servers)
}

fn configure_server(template: &McpTemplate) -> Result<McpServerConfig> {
    println!();
    println!("  Configuring {}...", template.label.split('—').next().unwrap().trim());

    let mut env = HashMap::new();

    // Collect required env vars
    for var in template.env_vars {
        let value = prompt_env_var(var)?;
        env.insert(var.to_string(), value);
    }

    let transport = if template.package.is_empty() {
        // Custom command (e.g., Google/Gmail)
        let command: String = Input::new()
            .with_prompt("Command to run")
            .interact_text()?;
        let args_str: String = Input::new()
            .with_prompt("Arguments (space-separated, blank for none)")
            .allow_empty(true)
            .interact_text()?;
        let args = if args_str.trim().is_empty() {
            vec![]
        } else {
            args_str.split_whitespace().map(String::from).collect()
        };
        McpTransport::Stdio { command, args, env }
    } else {
        McpTransport::Stdio {
            command: "npx".to_string(),
            args: vec!["-y".to_string(), template.package.to_string()],
            env,
        }
    };

    Ok(McpServerConfig {
        transport,
        auto_connect: true,
        allow_sampling: false,
        timeout_secs: 120,
        tier_override: None,
        headers: HashMap::new(),
    })
}

fn prompt_env_var(var: &str) -> Result<String> {
    if std::env::var(var).is_ok() {
        Ok(format!("${{{var}}}"))
    } else {
        let value = Password::new()
            .with_prompt(var.to_string())
            .interact()?;
        Ok(value)
    }
}
