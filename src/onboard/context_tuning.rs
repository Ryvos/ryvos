use anyhow::Result;
use dialoguer::{Input, Select};
use ryvos_core::config::ContextConfig;

pub fn configure() -> Result<ContextConfig> {
    println!("  \x1b[1;36mContext Management\x1b[0m");
    println!("  Controls what gets loaded into the agent's system prompt.");

    let mode_options = &[
        "relevant (skip logs when not needed -- saves tokens)",
        "always (load every time)",
        "never (disable daily logs)",
    ];
    let mode_choice = Select::new()
        .with_prompt("Daily log loading mode")
        .items(mode_options)
        .default(0)
        .interact()?;
    let daily_log_mode = match mode_choice {
        0 => "relevant",
        1 => "always",
        _ => "never",
    }
    .to_string();

    let viking_max_l0: String = Input::new()
        .with_prompt("Max Viking L0 entries per directory")
        .default("10".to_string())
        .interact_text()?;

    let protected_ttl: String = Input::new()
        .with_prompt("Protected message TTL in turns (0 = never expire)")
        .default("20".to_string())
        .interact_text()?;

    Ok(ContextConfig {
        daily_log_mode,
        daily_log_days: 3,
        viking_max_l0: viking_max_l0.parse().unwrap_or(10),
        viking_min_relevance: 0.3,
        max_safety_lessons: 3,
        safety_filter_by_tools: true,
        protected_ttl: protected_ttl.parse().unwrap_or(20),
    })
}
