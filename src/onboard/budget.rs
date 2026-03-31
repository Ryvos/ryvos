use std::collections::HashMap;

use anyhow::Result;
use dialoguer::{Confirm, Input};
use ryvos_core::config::BudgetConfig;

pub fn configure() -> Result<Option<BudgetConfig>> {
    let enable = Confirm::new()
        .with_prompt("Set a monthly budget limit?")
        .default(false)
        .interact()?;

    if !enable {
        return Ok(None);
    }

    let amount: String = Input::new()
        .with_prompt("Monthly budget in dollars (e.g., 50)")
        .validate_with(|input: &String| -> std::result::Result<(), String> {
            input
                .parse::<f64>()
                .map(|v| if v > 0.0 {})
                .map_err(|_| "Must be a positive number".to_string())
        })
        .interact_text()?;

    let dollars: f64 = amount.parse().unwrap_or(50.0);
    let cents = (dollars * 100.0) as u64;

    let warn_pct: String = Input::new()
        .with_prompt("Warning threshold (%)")
        .default("80".to_string())
        .interact_text()?;

    let hard_stop_pct: String = Input::new()
        .with_prompt("Hard stop threshold (%)")
        .default("100".to_string())
        .interact_text()?;

    Ok(Some(BudgetConfig {
        monthly_budget_cents: cents,
        warn_pct: warn_pct.parse().unwrap_or(80),
        hard_stop_pct: hard_stop_pct.parse().unwrap_or(100),
        pricing: HashMap::new(),
    }))
}
