use anyhow::Result;
use dialoguer::{Confirm, Input};
use ryvos_core::config::{CronConfig, CronJobConfig};

pub fn configure() -> Result<Option<CronConfig>> {
    let enable = Confirm::new()
        .with_prompt("Add scheduled tasks (cron jobs)?")
        .default(false)
        .interact()?;

    if !enable {
        return Ok(None);
    }

    let mut jobs = Vec::new();

    loop {
        println!();
        let name: String = Input::new().with_prompt("Task name").interact_text()?;

        let schedule: String = Input::new()
            .with_prompt("Cron expression (e.g., '0 9 * * *' for daily at 9am)")
            .interact_text()?;

        let prompt: String = Input::new()
            .with_prompt("Prompt (what should the agent do?)")
            .interact_text()?;

        let channel: String = Input::new()
            .with_prompt("Target channel (blank = no channel output)")
            .allow_empty(true)
            .interact_text()?;

        jobs.push(CronJobConfig {
            name,
            schedule,
            prompt,
            channel: if channel.is_empty() {
                None
            } else {
                Some(channel)
            },
            goal: None,
        });

        let another = Confirm::new()
            .with_prompt("Add another scheduled task?")
            .default(false)
            .interact()?;

        if !another {
            break;
        }
    }

    println!();
    println!("  Tip: Your agent can also create cron jobs at runtime using the `cron_add` tool.");
    println!("  Just tell it to do something \"every day\" or \"every hour\".");
    println!("  Note: new cron jobs require a daemon restart to take effect.");

    Ok(Some(CronConfig { jobs }))
}
