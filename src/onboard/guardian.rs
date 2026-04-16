use anyhow::Result;
use dialoguer::{Confirm, Input};
use ryvos_core::config::GuardianConfig;

pub fn configure() -> Result<GuardianConfig> {
    println!("  \x1b[1;36mGuardian Watchdog\x1b[0m");
    println!("  Detects doom loops, stalls, and token budget overruns.");

    let enabled = Confirm::new()
        .with_prompt("Enable guardian watchdog?")
        .default(true)
        .interact()?;
    if !enabled {
        return Ok(GuardianConfig {
            enabled: false,
            ..Default::default()
        });
    }

    let token_budget: String = Input::new()
        .with_prompt("Token budget per session (0 = unlimited)")
        .default("60000".to_string())
        .interact_text()?;

    let doom_loop_threshold: String = Input::new()
        .with_prompt("Doom loop threshold (consecutive identical tool calls)")
        .default("3".to_string())
        .interact_text()?;

    let stall_timeout: String = Input::new()
        .with_prompt("Stall timeout (seconds without progress)")
        .default("120".to_string())
        .interact_text()?;

    Ok(GuardianConfig {
        enabled: true,
        token_budget: token_budget.parse().unwrap_or(60000),
        doom_loop_threshold: doom_loop_threshold.parse().unwrap_or(3),
        stall_timeout_secs: stall_timeout.parse().unwrap_or(120),
        token_warn_pct: 80,
    })
}
