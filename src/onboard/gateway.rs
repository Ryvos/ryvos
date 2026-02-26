use anyhow::Result;
use dialoguer::{Confirm, Input, Password, Select};
use ryvos_core::config::{ApiKeyConfig, ApiKeyRole, GatewayConfig};

pub fn configure() -> Result<Option<GatewayConfig>> {
    let enable = Confirm::new()
        .with_prompt("Enable WebSocket gateway?")
        .default(false)
        .interact()?;

    if !enable {
        return Ok(None);
    }

    let port: String = Input::new()
        .with_prompt("Gateway port")
        .default("18789".to_string())
        .validate_with(|input: &String| -> std::result::Result<(), String> {
            input
                .parse::<u16>()
                .map(|_| ())
                .map_err(|_| "Must be a valid port number (1-65535)".to_string())
        })
        .interact_text()?;

    let tailscale_ip = detect_tailscale();

    let mut bind_options: Vec<String> = vec![
        "Loopback (127.0.0.1)".to_string(),
        "LAN (0.0.0.0)".to_string(),
    ];
    if let Some(ref ip) = tailscale_ip {
        bind_options.push(format!("Tailscale ({ip})"));
    }
    bind_options.push("Custom IP".to_string());

    let bind_refs: Vec<&str> = bind_options.iter().map(|s| s.as_str()).collect();
    let bind_choice = Select::new()
        .with_prompt("Gateway bind address")
        .items(&bind_refs)
        .default(0)
        .interact()?;

    let custom_idx = bind_options.len() - 1;
    let tailscale_idx = if tailscale_ip.is_some() {
        Some(2)
    } else {
        None
    };

    let ip = if bind_choice == 0 {
        "127.0.0.1".to_string()
    } else if bind_choice == 1 {
        "0.0.0.0".to_string()
    } else if tailscale_idx == Some(bind_choice) {
        tailscale_ip.unwrap()
    } else if bind_choice == custom_idx {
        Input::new()
            .with_prompt("Custom IP address")
            .interact_text()?
    } else {
        unreachable!()
    };

    let bind = format!("{ip}:{port}");

    let auth_options = &["Token (recommended)", "Password", "None"];
    let auth_choice = Select::new()
        .with_prompt("Gateway auth")
        .items(auth_options)
        .default(0)
        .interact()?;

    let (token, password) = match auth_choice {
        0 => {
            let input: String = Input::new()
                .with_prompt("Gateway token (blank to auto-generate)")
                .allow_empty(true)
                .interact_text()?;

            let t = if input.is_empty() {
                let t = format!("{:x}{:x}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
                println!("  Generated token: {t}");
                t
            } else {
                input
            };
            (Some(t), None)
        }
        1 => {
            let p = Password::new()
                .with_prompt("Gateway password")
                .with_confirmation("Confirm password", "Passwords don't match")
                .interact()?;
            (None, Some(p))
        }
        _ => (None, None),
    };

    // API key creation
    let mut api_keys = Vec::new();
    let create_key = Confirm::new()
        .with_prompt("Create an API key for the Web UI?")
        .default(false)
        .interact()?;

    if create_key {
        let key_name: String = Input::new()
            .with_prompt("Key name")
            .default("web-ui".to_string())
            .interact_text()?;

        let role_options = &["viewer", "operator (default)", "admin"];
        let role_choice = Select::new()
            .with_prompt("Key role")
            .items(role_options)
            .default(1)
            .interact()?;

        let role = match role_choice {
            0 => ApiKeyRole::Viewer,
            2 => ApiKeyRole::Admin,
            _ => ApiKeyRole::Operator,
        };

        let key = format!(
            "rk_{:x}{:x}",
            uuid::Uuid::new_v4().as_u128(),
            uuid::Uuid::new_v4().as_u128() >> 64
        );
        println!("  Generated key: {key}");

        api_keys.push(ApiKeyConfig {
            name: key_name,
            key,
            role,
        });
    }

    Ok(Some(GatewayConfig {
        bind,
        token,
        password,
        api_keys,
        webhooks: None,
    }))
}

fn detect_tailscale() -> Option<String> {
    let output = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()?;
    if output.status.success() {
        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !ip.is_empty() {
            Some(ip)
        } else {
            None
        }
    } else {
        None
    }
}
