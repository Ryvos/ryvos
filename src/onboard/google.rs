use std::io::{self, Write};
use std::path::Path;

/// Run the Google Workspace OAuth setup wizard.
pub fn run_google_setup() -> anyhow::Result<Option<ryvos_core::config::GoogleConfig>> {
    println!();
    println!("┌─ Google Workspace Setup ──────────────────────────┐");
    println!("│                                                    │");
    println!("│  Connect Gmail, Calendar, and Drive to Ryvos.      │");
    println!("│                                                    │");
    println!("│  Step 1: Go to console.cloud.google.com            │");
    println!("│  Step 2: Create a project (or select existing)     │");
    println!("│  Step 3: Enable Gmail, Calendar, and Drive APIs    │");
    println!("│  Step 4: Create OAuth Desktop credentials          │");
    println!("│  Step 5: Download the client_secret.json file      │");
    println!("│                                                    │");
    println!("└────────────────────────────────────────────────────┘");
    println!();

    print!("Path to client_secret.json (or 'skip'): ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() || input.eq_ignore_ascii_case("skip") {
        println!("  Skipping Google Workspace setup.");
        return Ok(None);
    }

    let path = Path::new(input);
    if !path.exists() {
        println!("  Warning: File not found at {}. Saving path anyway.", input);
    }

    Ok(Some(ryvos_core::config::GoogleConfig {
        client_secret_path: input.to_string(),
        tokens_path: "~/.ryvos/credentials/google/tokens.json".to_string(),
        gmail: true,
        calendar: true,
        drive: true,
        contacts: false,
    }))
}
