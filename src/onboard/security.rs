use anyhow::Result;
use dialoguer::{Confirm, Input, Select};
use ryvos_core::config::SecurityConfig;
use ryvos_core::security::{DangerousPattern, SecurityTier};

pub fn configure() -> Result<SecurityConfig> {
    let levels = &[
        "Strict  — auto-approve T0 only (read-only tools)",
        "Standard — auto-approve up to T1 (default, most tools)",
        "Permissive — auto-approve up to T2 (shell, network, etc.)",
    ];
    let choice = Select::new()
        .with_prompt("Security level")
        .items(levels)
        .default(1)
        .interact()?;

    let auto_approve = match choice {
        0 => SecurityTier::T0,
        2 => SecurityTier::T2,
        _ => SecurityTier::T1,
    };

    let deny_tiers = &[
        "None (allow all tiers with approval)",
        "T3 — block destructive actions",
        "T4 — block only system-level actions",
    ];
    let deny_choice = Select::new()
        .with_prompt("Deny tools above tier")
        .items(deny_tiers)
        .default(0)
        .interact()?;

    let deny_above = match deny_choice {
        1 => Some(SecurityTier::T3),
        2 => Some(SecurityTier::T4),
        _ => None,
    };

    let mut dangerous_patterns = Vec::new();

    if Confirm::new()
        .with_prompt("Add custom dangerous command patterns?")
        .default(false)
        .interact()?
    {
        loop {
            let pattern: String = Input::new()
                .with_prompt("Regex pattern (e.g., 'rm\\s+-rf')")
                .interact_text()?;
            let label: String = Input::new()
                .with_prompt("Label for this pattern")
                .interact_text()?;

            dangerous_patterns.push(DangerousPattern { pattern, label });

            if !Confirm::new()
                .with_prompt("Add another pattern?")
                .default(false)
                .interact()?
            {
                break;
            }
        }
    }

    Ok(SecurityConfig {
        auto_approve_up_to: auto_approve,
        deny_above,
        approval_timeout_secs: 120,
        tool_overrides: Default::default(),
        dangerous_patterns,
        sub_agent_policy: None,
        pause_before: vec![],
    })
}
