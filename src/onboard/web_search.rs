use anyhow::Result;
use dialoguer::{Password, Select};
use ryvos_core::config::{McpServerConfig, McpTransport};

pub fn configure() -> Result<Option<(String, McpServerConfig)>> {
    let options = &["Brave Search (API key)", "Tavily (API key)", "Skip"];
    let choice = Select::new()
        .with_prompt("Enable web search?")
        .items(options)
        .default(2)
        .interact()?;

    match choice {
        0 => configure_brave(),
        1 => configure_tavily(),
        _ => Ok(None),
    }
}

fn configure_brave() -> Result<Option<(String, McpServerConfig)>> {
    let api_key = prompt_search_key("BRAVE_API_KEY")?;
    let mut env = std::collections::HashMap::new();
    env.insert("BRAVE_API_KEY".to_string(), api_key);

    Ok(Some((
        "web_search".to_string(),
        McpServerConfig {
            transport: McpTransport::Stdio {
                command: "npx".to_string(),
                args: vec![
                    "-y".to_string(),
                    "@anthropic/mcp-server-brave-search".to_string(),
                ],
                env,
            },
            auto_connect: true,
            allow_sampling: false,
            timeout_secs: 120,
            tier_override: None,
            headers: std::collections::HashMap::new(),
        },
    )))
}

fn configure_tavily() -> Result<Option<(String, McpServerConfig)>> {
    let api_key = prompt_search_key("TAVILY_API_KEY")?;
    let mut env = std::collections::HashMap::new();
    env.insert("TAVILY_API_KEY".to_string(), api_key);

    Ok(Some((
        "web_search".to_string(),
        McpServerConfig {
            transport: McpTransport::Stdio {
                command: "npx".to_string(),
                args: vec!["-y".to_string(), "tavily-mcp".to_string()],
                env,
            },
            auto_connect: true,
            allow_sampling: false,
            timeout_secs: 120,
            tier_override: None,
            headers: std::collections::HashMap::new(),
        },
    )))
}

fn prompt_search_key(env_var: &str) -> Result<String> {
    if std::env::var(env_var).is_ok() {
        // env var exists — use reference
        Ok(format!("${{{env_var}}}"))
    } else {
        let key = Password::new()
            .with_prompt(format!("{env_var}"))
            .interact()?;
        Ok(key)
    }
}
