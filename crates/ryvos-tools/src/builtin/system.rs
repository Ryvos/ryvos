use futures::future::BoxFuture;
use serde::Deserialize;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

// ── ProcessListTool ─────────────────────────────────────────────

pub struct ProcessListTool;

impl Tool for ProcessListTool {
    fn name(&self) -> &str {
        "process_list"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "List running processes."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let output = tokio::process::Command::new("ps")
                .args(["aux", "--sort=-%mem"])
                .output()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "process_list".into(),
                    message: e.to_string(),
                })?;
            let text = String::from_utf8_lossy(&output.stdout);
            let truncated: String = text.lines().take(50).collect::<Vec<_>>().join("\n");
            Ok(ToolResult::success(truncated))
        })
    }
}

// ── ProcessKillTool ─────────────────────────────────────────────

pub struct ProcessKillTool;

#[derive(Deserialize)]
struct KillInput {
    pid: u32,
    #[serde(default = "default_signal")]
    signal: String,
}
fn default_signal() -> String {
    "SIGTERM".into()
}

impl Tool for ProcessKillTool {
    fn name(&self) -> &str {
        "process_kill"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T3
    }
    fn description(&self) -> &str {
        "Send a signal to a process."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pid": { "type": "integer", "description": "Process ID" },
                "signal": { "type": "string", "description": "Signal name (default: SIGTERM)" }
            },
            "required": ["pid"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: KillInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let sig = format!("-{}", p.signal.strip_prefix("SIG").unwrap_or(&p.signal));
            let output = tokio::process::Command::new("kill")
                .args([&sig, &p.pid.to_string()])
                .output()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "process_kill".into(),
                    message: e.to_string(),
                })?;
            if output.status.success() {
                Ok(ToolResult::success(format!(
                    "Sent {} to PID {}",
                    p.signal, p.pid
                )))
            } else {
                Ok(ToolResult::error(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ))
            }
        })
    }
}

// ── EnvGetTool ──────────────────────────────────────────────────

pub struct EnvGetTool;

#[derive(Deserialize)]
struct EnvInput {
    #[serde(default)]
    name: Option<String>,
}

impl Tool for EnvGetTool {
    fn name(&self) -> &str {
        "env_get"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Get an environment variable, or list all."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "name": { "type": "string", "description": "Var name. Omit to list all." } }
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: EnvInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            if let Some(name) = p.name {
                match std::env::var(&name) {
                    Ok(val) => Ok(ToolResult::success(format!("{}={}", name, val))),
                    Err(_) => Ok(ToolResult::error(format!("'{}' is not set", name))),
                }
            } else {
                let mut vars: Vec<_> = std::env::vars().collect();
                vars.sort_by(|a, b| a.0.cmp(&b.0));
                let output = vars
                    .into_iter()
                    .take(100)
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(ToolResult::success(output))
            }
        })
    }
}

// ── SystemInfoTool ──────────────────────────────────────────────

pub struct SystemInfoTool;

impl Tool for SystemInfoTool {
    fn name(&self) -> &str {
        "system_info"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Get system information (OS, CPU, memory)."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let uname = tokio::process::Command::new("uname")
                .arg("-a")
                .output()
                .await
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_else(|| "unavailable".into());
            let meminfo = tokio::fs::read_to_string("/proc/meminfo")
                .await
                .ok()
                .map(|s| s.lines().take(3).collect::<Vec<_>>().join("\n"))
                .unwrap_or_else(|| "unavailable".into());
            let cpuinfo = tokio::fs::read_to_string("/proc/cpuinfo")
                .await
                .ok()
                .map(|s| {
                    let model = s
                        .lines()
                        .find(|l| l.starts_with("model name"))
                        .unwrap_or("unknown");
                    let count = s.lines().filter(|l| l.starts_with("processor")).count();
                    format!("{} ({} cores)", model, count)
                })
                .unwrap_or_else(|| "unavailable".into());
            Ok(ToolResult::success(format!(
                "Kernel: {}\nMemory:\n{}\nCPU: {}",
                uname.trim(),
                meminfo,
                cpuinfo
            )))
        })
    }
}

// ── DiskUsageTool ───────────────────────────────────────────────

pub struct DiskUsageTool;

impl Tool for DiskUsageTool {
    fn name(&self) -> &str {
        "disk_usage"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Show disk usage information."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let output = tokio::process::Command::new("df")
                .arg("-h")
                .output()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "disk_usage".into(),
                    message: e.to_string(),
                })?;
            Ok(ToolResult::success(
                String::from_utf8_lossy(&output.stdout).to_string(),
            ))
        })
    }
}
