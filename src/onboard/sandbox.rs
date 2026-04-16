use anyhow::Result;
use dialoguer::{Confirm, Input};
use ryvos_core::config::SandboxConfig;

pub fn configure() -> Result<Option<SandboxConfig>> {
    println!("  \x1b[1;36mDocker Sandbox\x1b[0m");
    println!("  Run bash commands in isolated Docker containers.");

    let enabled = Confirm::new()
        .with_prompt("Enable sandboxed bash execution?")
        .default(false)
        .interact()?;
    if !enabled {
        return Ok(None);
    }

    // Check if Docker is available
    if std::process::Command::new("docker")
        .arg("--version")
        .output()
        .is_err()
    {
        println!("  \x1b[33mWarning: Docker not found on PATH. Sandbox requires Docker.\x1b[0m");
    }

    let image: String = Input::new()
        .with_prompt("Docker image")
        .default("ubuntu:24.04".to_string())
        .interact_text()?;

    let memory_mb: String = Input::new()
        .with_prompt("Memory limit (MB)")
        .default("512".to_string())
        .interact_text()?;

    let timeout_secs: String = Input::new()
        .with_prompt("Execution timeout (seconds)")
        .default("120".to_string())
        .interact_text()?;

    Ok(Some(SandboxConfig {
        enabled: true,
        mode: "docker".to_string(),
        image,
        memory_mb: memory_mb.parse().unwrap_or(512),
        timeout_secs: timeout_secs.parse().unwrap_or(120),
        mount_workspace: true,
    }))
}
