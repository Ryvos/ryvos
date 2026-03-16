use std::io::{self, Write};

/// Run the Notion API key setup wizard.
pub fn run_notion_setup() -> anyhow::Result<Option<ryvos_core::config::NotionConfig>> {
    println!();
    println!("┌─ Notion Setup ────────────────────────────────────┐");
    println!("│                                                    │");
    println!("│  Connect Notion pages and databases to Ryvos.      │");
    println!("│                                                    │");
    println!("│  Step 1: Go to notion.so/my-integrations           │");
    println!("│  Step 2: Create a new integration                  │");
    println!("│  Step 3: Copy the API key (starts with ntn_)       │");
    println!("│                                                    │");
    println!("└────────────────────────────────────────────────────┘");
    println!();

    print!("Notion API key (or 'skip'): ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() || input.eq_ignore_ascii_case("skip") {
        println!("  Skipping Notion setup.");
        return Ok(None);
    }

    Ok(Some(ryvos_core::config::NotionConfig {
        api_key: input.to_string(),
    }))
}
