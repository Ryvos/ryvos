use anyhow::Result;
use dialoguer::{Confirm, Input, Select};
use ryvos_core::config::{ChannelsConfig, DiscordConfig, DmPolicy, SlackConfig, TelegramConfig};

use super::OnboardingMode;

pub fn configure(mode: &OnboardingMode) -> Result<ChannelsConfig> {
    let mut telegram = None;
    let mut discord = None;
    let mut slack = None;

    match mode {
        OnboardingMode::QuickStart => {
            let options = &["Telegram", "Discord", "Slack", "Skip"];
            let choice = Select::new()
                .with_prompt("Configure a chat channel?")
                .items(options)
                .default(3)
                .interact()?;

            match choice {
                0 => telegram = Some(configure_telegram()?),
                1 => discord = Some(configure_discord()?),
                2 => slack = Some(configure_slack()?),
                _ => {}
            }
        }
        OnboardingMode::Manual => loop {
            let options = &["Telegram", "Discord", "Slack", "Finished"];
            let choice = Select::new()
                .with_prompt("Add a channel")
                .items(options)
                .default(3)
                .interact()?;

            match choice {
                0 => telegram = Some(configure_telegram()?),
                1 => discord = Some(configure_discord()?),
                2 => slack = Some(configure_slack()?),
                _ => break,
            }
        },
    }

    Ok(ChannelsConfig { telegram, discord, slack })
}

fn prompt_dm_policy() -> Result<DmPolicy> {
    let options = &[
        "Allowlist (recommended \u{2014} only listed user IDs)",
        "Open (any user can message the bot)",
        "Disabled (ignore all DMs)",
    ];
    let choice = Select::new()
        .with_prompt("DM access policy")
        .items(options)
        .default(0)
        .interact()?;

    Ok(match choice {
        0 => DmPolicy::Allowlist,
        1 => DmPolicy::Open,
        2 => DmPolicy::Disabled,
        _ => unreachable!(),
    })
}

fn prompt_token_or_env(label: &str, env_var: &str) -> Result<String> {
    if let Ok(_existing) = std::env::var(env_var) {
        let use_existing = Confirm::new()
            .with_prompt(format!("Found {} env var. Use it?", env_var))
            .default(true)
            .interact()?;

        if use_existing {
            return Ok(format!("${{{}}}", env_var));
        }
    }

    Ok(Input::new()
        .with_prompt(label)
        .interact_text()?)
}

fn configure_telegram() -> Result<TelegramConfig> {
    println!();
    println!("  Telegram setup:");
    println!("  1) Open Telegram -> @BotFather -> /newbot");
    println!("  2) Copy the bot token");
    println!();

    let bot_token = prompt_token_or_env("Telegram bot token", "TELEGRAM_BOT_TOKEN")?;
    let dm_policy = prompt_dm_policy()?;

    let allowed_users = if dm_policy == DmPolicy::Allowlist {
        let allowed_input: String = Input::new()
            .with_prompt("Allowed user IDs (comma-separated, blank for all)")
            .allow_empty(true)
            .interact_text()?;

        if allowed_input.trim().is_empty() {
            vec![]
        } else {
            allowed_input
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect()
        }
    } else {
        vec![]
    };

    Ok(TelegramConfig {
        bot_token,
        allowed_users,
        dm_policy,
    })
}

fn configure_discord() -> Result<DiscordConfig> {
    println!();
    println!("  Discord setup:");
    println!("  1) Discord Developer Portal -> New Application -> Bot");
    println!("  2) Copy the bot token");
    println!();

    let bot_token = prompt_token_or_env("Discord bot token", "DISCORD_BOT_TOKEN")?;
    let dm_policy = prompt_dm_policy()?;

    let allowed_users = if dm_policy == DmPolicy::Allowlist {
        let allowed_input: String = Input::new()
            .with_prompt("Allowed user IDs (comma-separated, blank for all)")
            .allow_empty(true)
            .interact_text()?;

        if allowed_input.trim().is_empty() {
            vec![]
        } else {
            allowed_input
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect()
        }
    } else {
        vec![]
    };

    Ok(DiscordConfig {
        bot_token,
        dm_policy,
        allowed_users,
    })
}

fn configure_slack() -> Result<SlackConfig> {
    println!();
    println!("  Slack setup:");
    println!("  1) Create a Slack app at api.slack.com/apps");
    println!("  2) Enable Socket Mode, add Event Subscriptions (message.im)");
    println!("  3) Install to workspace, copy Bot Token (xoxb-) and App Token (xapp-)");
    println!();

    let bot_token = prompt_token_or_env("Slack bot token", "SLACK_BOT_TOKEN")?;
    let app_token = prompt_token_or_env("Slack app token", "SLACK_APP_TOKEN")?;
    let dm_policy = prompt_dm_policy()?;

    let allowed_users = if dm_policy == DmPolicy::Allowlist {
        let allowed_input: String = Input::new()
            .with_prompt("Allowed Slack user IDs (comma-separated, blank for all)")
            .allow_empty(true)
            .interact_text()?;

        if allowed_input.trim().is_empty() {
            vec![]
        } else {
            allowed_input
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
    } else {
        vec![]
    };

    Ok(SlackConfig {
        bot_token,
        app_token,
        dm_policy,
        allowed_users,
    })
}
