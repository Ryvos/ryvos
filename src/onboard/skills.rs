use std::path::Path;

use anyhow::Result;
use dialoguer::Confirm;

pub fn configure(workspace: &Path) -> Result<()> {
    let create = Confirm::new()
        .with_prompt("Create a sample skill?")
        .default(false)
        .interact()?;

    if !create {
        return Ok(());
    }

    let skills_dir = workspace.join("skills/hello");
    std::fs::create_dir_all(&skills_dir)?;

    let manifest = r#"name = "hello"
description = "Say hello — a sample skill to get you started"
command = "echo Hello from Ryvos!"
timeout_secs = 10
"#;
    std::fs::write(skills_dir.join("skill.toml"), manifest)?;
    println!("  Created sample skill: {}", skills_dir.display());
    Ok(())
}
