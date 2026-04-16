use anyhow::Result;
use dialoguer::{Confirm, Input};
use ryvos_core::config::OpenVikingConfig;

pub fn configure() -> Result<Option<OpenVikingConfig>> {
    println!("  \x1b[1;36mOpenViking Hierarchical Memory\x1b[0m");
    println!("  Persistent memory that survives across sessions and restarts.");

    let enabled = Confirm::new()
        .with_prompt("Enable Viking hierarchical memory?")
        .default(false)
        .interact()?;
    if !enabled {
        return Ok(None);
    }

    let base_url: String = Input::new()
        .with_prompt("Viking server URL")
        .default("http://localhost:1933".to_string())
        .interact_text()?;

    let user_id: String = Input::new()
        .with_prompt("User ID")
        .default("ryvos-default".to_string())
        .interact_text()?;

    let auto_iterate = Confirm::new()
        .with_prompt("Auto-extract memories after sessions?")
        .default(true)
        .interact()?;

    Ok(Some(OpenVikingConfig {
        enabled: true,
        base_url,
        user_id,
        auto_iterate,
    }))
}
