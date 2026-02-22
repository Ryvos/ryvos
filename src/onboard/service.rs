use std::path::Path;

use anyhow::Result;

use super::OnboardingMode;

pub async fn install(config_path: &Path, mode: &OnboardingMode, non_interactive: bool) -> Result<()> {
    if cfg!(target_os = "linux") {
        install_systemd(config_path, mode, non_interactive).await
    } else if cfg!(target_os = "macos") {
        install_launchd(config_path, mode, non_interactive).await
    } else {
        if !non_interactive {
            println!("  Service install not available on this platform.");
        }
        Ok(())
    }
}

async fn install_systemd(config_path: &Path, mode: &OnboardingMode, non_interactive: bool) -> Result<()> {
    // Check systemd user session
    let status = tokio::process::Command::new("systemctl")
        .args(["--user", "status"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    if status.is_err() {
        if !non_interactive {
            println!("  systemd user session not available. Skipping service install.");
        }
        return Ok(());
    }

    // Check lingering
    let user = std::env::var("USER").unwrap_or_default();
    let linger_check = tokio::process::Command::new("loginctl")
        .args(["show-user", &user, "--property=Linger"])
        .output()
        .await;

    if let Ok(output) = linger_check {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.contains("Linger=yes") {
            let enable = if non_interactive {
                true
            } else {
                dialoguer::Confirm::new()
                    .with_prompt("Enable systemd lingering? (recommended for daemon mode)")
                    .default(true)
                    .interact()?
            };

            if enable {
                let result = tokio::process::Command::new("loginctl")
                    .args(["enable-linger", &user])
                    .status()
                    .await;
                match result {
                    Ok(s) if s.success() => println!("  Lingering enabled."),
                    _ => println!("  Warning: Could not enable lingering. Run: loginctl enable-linger {user}"),
                }
            }
        }
    }

    let should_install = if non_interactive {
        true
    } else {
        match mode {
            OnboardingMode::QuickStart => true,
            OnboardingMode::Manual => {
                dialoguer::Confirm::new()
                    .with_prompt("Install systemd service?")
                    .default(true)
                    .interact()?
            }
        }
    };

    if !should_install {
        return Ok(());
    }

    let binary_path = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "ryvos".to_string());
    let config_display = config_path.display();

    let unit = format!(
        r#"[Unit]
Description=Ryvos AI Agent Daemon
After=network.target

[Service]
Type=simple
ExecStart={binary_path} --config {config_display} daemon
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=ryvos=info

[Install]
WantedBy=default.target
"#
    );

    let service_dir = dirs_home()
        .map(|h| h.join(".config/systemd/user"))
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    std::fs::create_dir_all(&service_dir)?;

    let service_path = service_dir.join("ryvos.service");
    std::fs::write(&service_path, &unit)?;
    println!("  Wrote {}", service_path.display());

    // Reload and enable
    let reload = tokio::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .await;
    if let Ok(s) = reload {
        if !s.success() {
            println!("  Warning: daemon-reload failed");
        }
    }

    let enable = tokio::process::Command::new("systemctl")
        .args(["--user", "enable", "--now", "ryvos.service"])
        .status()
        .await;
    match enable {
        Ok(s) if s.success() => println!("  Service enabled and started."),
        _ => {
            println!("  Warning: Could not enable service. Run manually:");
            println!("    systemctl --user enable --now ryvos.service");
            return Ok(());
        }
    }

    // Health check
    println!("  Checking service health...");
    for _ in 0..5 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let check = tokio::process::Command::new("systemctl")
            .args(["--user", "is-active", "ryvos.service"])
            .output()
            .await;
        if let Ok(output) = check {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim() == "active" {
                println!("  Service is active.");
                return Ok(());
            }
        }
    }
    println!("  Warning: Service may not be running. Check: systemctl --user status ryvos.service");

    Ok(())
}

async fn install_launchd(config_path: &Path, mode: &OnboardingMode, non_interactive: bool) -> Result<()> {
    let should_install = if non_interactive {
        true
    } else {
        match mode {
            OnboardingMode::QuickStart => true,
            OnboardingMode::Manual => {
                dialoguer::Confirm::new()
                    .with_prompt("Install launchd service?")
                    .default(true)
                    .interact()?
            }
        }
    };

    if !should_install {
        return Ok(());
    }

    let home = dirs_home()
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    let binary_path = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "ryvos".to_string());
    let config_display = config_path.display().to_string();
    let home_display = home.display().to_string();

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.ryvos.agent</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary_path}</string>
        <string>--config</string>
        <string>{config_display}</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{home_display}/.ryvos/daemon.log</string>
    <key>StandardErrorPath</key>
    <string>{home_display}/.ryvos/daemon.err</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>ryvos=info</string>
    </dict>
</dict>
</plist>
"#
    );

    let launch_agents_dir = home.join("Library/LaunchAgents");
    std::fs::create_dir_all(&launch_agents_dir)?;

    // Ensure log directory exists
    let log_dir = home.join(".ryvos");
    std::fs::create_dir_all(&log_dir)?;

    let plist_path = launch_agents_dir.join("com.ryvos.agent.plist");
    std::fs::write(&plist_path, &plist)?;
    println!("  Wrote {}", plist_path.display());

    // Load the agent
    let load = tokio::process::Command::new("launchctl")
        .args(["load", &plist_path.display().to_string()])
        .status()
        .await;
    match load {
        Ok(s) if s.success() => println!("  LaunchAgent loaded."),
        _ => {
            println!("  Warning: Could not load LaunchAgent. Run manually:");
            println!("    launchctl load {}", plist_path.display());
            return Ok(());
        }
    }

    // Health check
    println!("  Checking service health...");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let check = tokio::process::Command::new("launchctl")
        .args(["list"])
        .output()
        .await;
    if let Ok(output) = check {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("com.ryvos.agent") {
            println!("  LaunchAgent is running.");
        } else {
            println!("  Warning: LaunchAgent may not be running. Check: launchctl list | grep ryvos");
        }
    }

    Ok(())
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(std::path::PathBuf::from)
}
