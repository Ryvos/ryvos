use anyhow::Result;
use dialoguer::{Confirm, Input};
use ryvos_core::config::DirectorConfig;

pub fn configure() -> Result<Option<DirectorConfig>> {
    println!("  \x1b[1;36mDirector Orchestration\x1b[0m");
    println!("  OODA-loop graph execution for complex multi-step tasks.");

    let enabled = Confirm::new()
        .with_prompt("Enable Director orchestration?")
        .default(true)
        .interact()?;
    if !enabled {
        return Ok(Some(DirectorConfig {
            enabled: false,
            ..Default::default()
        }));
    }

    let max_evolution: String = Input::new()
        .with_prompt("Max evolution cycles (replanning attempts)")
        .default("3".to_string())
        .interact_text()?;

    let failure_threshold: String = Input::new()
        .with_prompt("Failure threshold before evolution")
        .default("3".to_string())
        .interact_text()?;

    Ok(Some(DirectorConfig {
        enabled: true,
        max_evolution_cycles: max_evolution.parse().unwrap_or(3),
        failure_threshold: failure_threshold.parse().unwrap_or(3),
        model: None,
    }))
}
