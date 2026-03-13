use std::path::Path;

use anyhow::Result;
use dialoguer::{Confirm, MultiSelect};
use ryvos_core::config::RegistryConfig;

struct BundledSkill {
    name: &'static str,
    label: &'static str,
    description: &'static str,
    command: &'static str,
    tier: &'static str,
}

const BUNDLED_SKILLS: &[BundledSkill] = &[
    BundledSkill {
        name: "weather",
        label: "weather — get current weather for a city",
        description: "Get current weather for a city",
        command: "curl -s \"wttr.in/$CITY?format=3\"",
        tier: "T1",
    },
    BundledSkill {
        name: "summarize-url",
        label: "summarize-url — fetch and summarize a webpage",
        description: "Fetch and summarize a webpage",
        command: "curl -sL \"$URL\" | head -c 8000",
        tier: "T1",
    },
    BundledSkill {
        name: "git-standup",
        label: "git-standup — show what you worked on today",
        description: "Show what you worked on today (git log since yesterday)",
        command: "git -C \"${REPO:-.}\" log --oneline --since=yesterday --author=\"$(git config user.email)\"",
        tier: "T0",
    },
    BundledSkill {
        name: "disk-report",
        label: "disk-report — detailed disk usage report",
        description: "Detailed disk usage report",
        command: "df -h && du -sh /home/*/ 2>/dev/null | sort -rh | head -10",
        tier: "T0",
    },
    BundledSkill {
        name: "port-scan",
        label: "port-scan — check if a port is open on a host",
        description: "Check if a port is open on a host",
        command: "nc -zv $HOST $PORT 2>&1",
        tier: "T1",
    },
    BundledSkill {
        name: "docker-status",
        label: "docker-status — list running containers and resource usage",
        description: "List running containers and resource usage",
        command: "docker ps --format \"table {{.Names}}\\t{{.Status}}\\t{{.Ports}}\" && docker stats --no-stream --format \"table {{.Name}}\\t{{.CPUPerc}}\\t{{.MemUsage}}\" 2>/dev/null",
        tier: "T1",
    },
];

pub fn configure(workspace: &Path) -> Result<Option<RegistryConfig>> {
    // Step 1: Bundled skills
    let labels: Vec<&str> = BUNDLED_SKILLS.iter().map(|s| s.label).collect();

    let selections = MultiSelect::new()
        .with_prompt("Install bundled skills (space to toggle, enter to confirm)")
        .items(&labels)
        .interact()?;

    let skills_dir = workspace.join("skills");
    for idx in selections {
        let skill = &BUNDLED_SKILLS[idx];
        install_skill(&skills_dir, skill)?;
    }

    // Step 2: Skills registry
    println!();
    let registry = if Confirm::new()
        .with_prompt("Enable skills registry? (browse and install community skills)")
        .default(false)
        .interact()?
    {
        println!("  Registry enabled. Use these commands:");
        println!("    ryvos skill search <query>");
        println!("    ryvos skill install <name>");
        Some(RegistryConfig {
            url: "https://raw.githubusercontent.com/Ryvos/registry/main/index.json".to_string(),
            cache_dir: None,
        })
    } else {
        None
    };

    Ok(registry)
}

fn install_skill(skills_dir: &Path, skill: &BundledSkill) -> Result<()> {
    let dir = skills_dir.join(skill.name);
    std::fs::create_dir_all(&dir)?;

    let manifest = format!(
        "name = \"{}\"\n\
         description = \"{}\"\n\
         command = \"{}\"\n\
         timeout_secs = 30\n\
         tier = \"{}\"\n",
        skill.name,
        skill.description,
        skill.command.replace('\\', "\\\\").replace('"', "\\\""),
        skill.tier,
    );

    std::fs::write(dir.join("skill.toml"), manifest)?;
    println!("  Installed skill: {}", skill.name);
    Ok(())
}
