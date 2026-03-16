use std::io::{self, Write};

/// Run the Jira setup wizard.
pub fn run_jira_setup() -> anyhow::Result<Option<ryvos_core::config::JiraConfig>> {
    println!();
    println!("┌─ Jira Setup ──────────────────────────────────────┐");
    println!("│                                                    │");
    println!("│  Connect Jira issues and sprints to Ryvos.         │");
    println!("│                                                    │");
    println!("│  Step 1: Go to id.atlassian.com/manage-profile     │");
    println!("│  Step 2: Create an API token                       │");
    println!("│                                                    │");
    println!("└────────────────────────────────────────────────────┘");
    println!();

    print!("Jira instance URL (e.g., https://myorg.atlassian.net) or 'skip': ");
    io::stdout().flush()?;
    let mut url = String::new();
    io::stdin().read_line(&mut url)?;
    let url = url.trim();

    if url.is_empty() || url.eq_ignore_ascii_case("skip") {
        println!("  Skipping Jira setup.");
        return Ok(None);
    }

    print!("Email: ");
    io::stdout().flush()?;
    let mut email = String::new();
    io::stdin().read_line(&mut email)?;

    print!("API token: ");
    io::stdout().flush()?;
    let mut token = String::new();
    io::stdin().read_line(&mut token)?;

    Ok(Some(ryvos_core::config::JiraConfig {
        base_url: url.to_string(),
        email: email.trim().to_string(),
        api_token: token.trim().to_string(),
    }))
}

/// Run the Linear setup wizard.
pub fn run_linear_setup() -> anyhow::Result<Option<ryvos_core::config::LinearConfig>> {
    println!();
    println!("┌─ Linear Setup ────────────────────────────────────┐");
    println!("│                                                    │");
    println!("│  Connect Linear issues and projects to Ryvos.      │");
    println!("│                                                    │");
    println!("│  Step 1: Go to linear.app/settings/api             │");
    println!("│  Step 2: Create a personal API key                 │");
    println!("│                                                    │");
    println!("└────────────────────────────────────────────────────┘");
    println!();

    print!("Linear API key (or 'skip'): ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() || input.eq_ignore_ascii_case("skip") {
        println!("  Skipping Linear setup.");
        return Ok(None);
    }

    Ok(Some(ryvos_core::config::LinearConfig {
        api_key: input.to_string(),
    }))
}
