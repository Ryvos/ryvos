use anyhow::Result;
use dialoguer::Input;
use ryvos_core::config::{DmPolicy, WhatsAppConfig};

use super::channels::{prompt_dm_policy, prompt_token_or_env};

pub fn configure_whatsapp() -> Result<WhatsAppConfig> {
    println!();
    println!("  WhatsApp Cloud API setup:");
    println!("  1) Create a Meta Business app at developers.facebook.com");
    println!("  2) Add the WhatsApp product, generate a permanent access token");
    println!("  3) Note your Phone Number ID from the API setup page");
    println!();

    let access_token = prompt_token_or_env("WhatsApp access token", "WHATSAPP_ACCESS_TOKEN")?;
    let phone_number_id: String = Input::new()
        .with_prompt("Phone Number ID")
        .interact_text()?;
    let verify_token: String = Input::new()
        .with_prompt("Webhook verify token (you choose this)")
        .interact_text()?;

    let dm_policy = prompt_dm_policy()?;

    let allowed_users = if dm_policy == DmPolicy::Allowlist {
        let input: String = Input::new()
            .with_prompt("Allowed phone numbers (E.164, comma-separated, blank for all)")
            .allow_empty(true)
            .interact_text()?;

        if input.trim().is_empty() {
            vec![]
        } else {
            input
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
    } else {
        vec![]
    };

    Ok(WhatsAppConfig {
        access_token,
        phone_number_id,
        verify_token,
        dm_policy,
        allowed_users,
    })
}
