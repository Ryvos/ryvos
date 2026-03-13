use anyhow::Result;
use dialoguer::{Confirm, Input};
use ryvos_core::config::{ActiveHoursConfig, HeartbeatConfig};

pub fn configure() -> Result<Option<HeartbeatConfig>> {
    let enable = Confirm::new()
        .with_prompt("Enable heartbeat monitoring?")
        .default(false)
        .interact()?;

    if !enable {
        return Ok(None);
    }

    let interval: String = Input::new()
        .with_prompt("Heartbeat interval in seconds")
        .default("1800".to_string())
        .validate_with(|input: &String| -> std::result::Result<(), String> {
            input
                .parse::<u64>()
                .map(|v| {
                    if v >= 60 {
                        ()
                    } else {
                        return;
                    }
                })
                .map_err(|_| "Must be a number >= 60".to_string())
        })
        .interact_text()?;

    let target_channel: String = Input::new()
        .with_prompt("Target channel for alerts (blank = broadcast to all)")
        .allow_empty(true)
        .interact_text()?;

    let active_hours = if Confirm::new()
        .with_prompt("Restrict to active hours?")
        .default(false)
        .interact()?
    {
        let start: String = Input::new()
            .with_prompt("Start hour (0-23)")
            .default("9".to_string())
            .interact_text()?;
        let end: String = Input::new()
            .with_prompt("End hour (0-23)")
            .default("22".to_string())
            .interact_text()?;
        Some(ActiveHoursConfig {
            start_hour: start.parse().unwrap_or(9),
            end_hour: end.parse().unwrap_or(22),
            utc_offset_hours: 0,
        })
    } else {
        None
    };

    // Handle duplicate heartbeat files
    cleanup_heartbeat_files();

    println!("  Heartbeat uses HEARTBEAT.md in your workspace — customize it to define what the agent checks.");

    Ok(Some(HeartbeatConfig {
        enabled: true,
        interval_secs: interval.parse().unwrap_or(1800),
        target_channel: if target_channel.is_empty() {
            None
        } else {
            Some(target_channel)
        },
        active_hours,
        ack_max_chars: 300,
        heartbeat_file: "HEARTBEAT.md".to_string(),
        prompt: None,
    }))
}

fn cleanup_heartbeat_files() {
    let home = match std::env::var("HOME").ok() {
        Some(h) => std::path::PathBuf::from(h),
        None => return,
    };
    let workspace = home.join(".ryvos");
    let lowercase = workspace.join("heartbeat.md");
    let uppercase = workspace.join("HEARTBEAT.md");

    if lowercase.exists() && !uppercase.exists() {
        if std::fs::rename(&lowercase, &uppercase).is_ok() {
            println!("  Renamed heartbeat.md → HEARTBEAT.md");
        }
    }
}
