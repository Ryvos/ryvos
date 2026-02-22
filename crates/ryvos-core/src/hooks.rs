use tracing::warn;

/// Execute hook commands with environment variables.
/// Fire-and-forget: errors are logged, not propagated.
pub async fn run_hooks(commands: &[String], env_vars: &[(&str, &str)]) {
    for cmd in commands {
        let mut command = tokio::process::Command::new("sh");
        command.args(["-c", cmd]);
        for (key, val) in env_vars {
            command.env(key, val);
        }
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        match command.status().await {
            Ok(s) if !s.success() => warn!(hook = %cmd, code = s.code(), "Hook exited non-zero"),
            Err(e) => warn!(hook = %cmd, error = %e, "Hook failed to execute"),
            _ => {}
        }
    }
}
