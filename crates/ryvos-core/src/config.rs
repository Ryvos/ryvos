use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, RyvosError};
use crate::security::{DangerousPattern, SecurityPolicy, SecurityTier};
use crate::types::ThinkingLevel;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WizardMetadata {
    #[serde(default)]
    pub last_run_at: Option<String>,
    #[serde(default)]
    pub last_run_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksConfig {
    #[serde(default)]
    pub on_start: Vec<String>,
    #[serde(default)]
    pub on_message: Vec<String>,
    #[serde(default)]
    pub on_tool_call: Vec<String>,
    #[serde(default)]
    pub on_response: Vec<String>,
    #[serde(default)]
    pub on_turn_complete: Vec<String>,
    #[serde(default)]
    pub on_tool_error: Vec<String>,
    #[serde(default)]
    pub on_session_start: Vec<String>,
    #[serde(default)]
    pub on_session_end: Vec<String>,
}

impl HooksConfig {
    pub fn is_empty(&self) -> bool {
        self.on_start.is_empty()
            && self.on_message.is_empty()
            && self.on_tool_call.is_empty()
            && self.on_response.is_empty()
            && self.on_turn_complete.is_empty()
            && self.on_tool_error.is_empty()
            && self.on_session_start.is_empty()
            && self.on_session_end.is_empty()
    }
}

/// Top-level Ryvos configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub agent: AgentConfig,
    pub model: ModelConfig,
    #[serde(default)]
    pub fallback_models: Vec<ModelConfig>,
    #[serde(default)]
    pub gateway: Option<GatewayConfig>,
    #[serde(default)]
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub mcp: Option<McpConfig>,
    #[serde(default)]
    pub hooks: Option<HooksConfig>,
    #[serde(default)]
    pub wizard: Option<WizardMetadata>,
    #[serde(default)]
    pub cron: Option<CronConfig>,
    #[serde(default)]
    pub heartbeat: Option<HeartbeatConfig>,
    #[serde(default)]
    pub web_search: Option<WebSearchConfig>,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub embedding: Option<EmbeddingConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    #[serde(default = "default_max_duration")]
    pub max_duration_secs: u64,
    #[serde(default = "default_workspace")]
    pub workspace: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
    #[serde(default = "default_max_tool_output_tokens")]
    pub max_tool_output_tokens: usize,
    #[serde(default = "default_reflexion_failure_threshold")]
    pub reflexion_failure_threshold: usize,
    #[serde(default = "default_parallel_tools")]
    pub parallel_tools: bool,
    #[serde(default = "default_enable_summarization")]
    pub enable_summarization: bool,
    #[serde(default)]
    pub sandbox: Option<SandboxConfig>,
    /// Enable LLM-as-judge self-evaluation after each run (default: false).
    #[serde(default)]
    pub enable_self_eval: bool,
    /// Guardian watchdog configuration.
    #[serde(default)]
    pub guardian: GuardianConfig,
    /// Runtime logging configuration.
    #[serde(default)]
    pub log: Option<LogConfig>,
    /// Checkpoint / resume configuration.
    #[serde(default)]
    pub checkpoint: Option<CheckpointConfig>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: default_max_turns(),
            max_duration_secs: default_max_duration(),
            workspace: default_workspace(),
            system_prompt: None,
            max_context_tokens: default_max_context_tokens(),
            max_tool_output_tokens: default_max_tool_output_tokens(),
            reflexion_failure_threshold: default_reflexion_failure_threshold(),
            parallel_tools: default_parallel_tools(),
            enable_summarization: default_enable_summarization(),
            sandbox: None,
            enable_self_eval: false,
            guardian: GuardianConfig::default(),
            log: None,
            checkpoint: None,
        }
    }
}

fn default_enable_summarization() -> bool { true }

/// Checkpoint / resume configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointConfig {
    /// Enable checkpointing (default: true when section is present).
    #[serde(default = "default_checkpoint_enabled")]
    pub enabled: bool,
    /// Directory for checkpoint files. Default: <workspace>/checkpoints
    #[serde(default)]
    pub checkpoint_dir: Option<String>,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            checkpoint_dir: None,
        }
    }
}

fn default_checkpoint_enabled() -> bool { true }

/// Guardian watchdog configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianConfig {
    /// Enable the guardian watchdog (default: true).
    #[serde(default = "default_guardian_enabled")]
    pub enabled: bool,
    /// Number of consecutive identical tool calls to trigger doom loop detection.
    #[serde(default = "default_doom_loop_threshold")]
    pub doom_loop_threshold: usize,
    /// Seconds without progress before triggering stall detection.
    #[serde(default = "default_stall_timeout_secs")]
    pub stall_timeout_secs: u64,
    /// Total token budget (0 = unlimited).
    #[serde(default = "default_token_budget")]
    pub token_budget: u64,
    /// Percentage of budget at which to emit a soft warning.
    #[serde(default = "default_token_warn_pct")]
    pub token_warn_pct: u8,
}

impl Default for GuardianConfig {
    fn default() -> Self {
        Self {
            enabled: default_guardian_enabled(),
            doom_loop_threshold: default_doom_loop_threshold(),
            stall_timeout_secs: default_stall_timeout_secs(),
            token_budget: default_token_budget(),
            token_warn_pct: default_token_warn_pct(),
        }
    }
}

/// JSONL runtime logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// Enable runtime logging (default: true when section is present).
    #[serde(default = "default_log_enabled")]
    pub enabled: bool,
    /// Directory for log files. Default: <workspace>/logs
    #[serde(default)]
    pub log_dir: Option<String>,
    /// Logging level: 1 = run summary only, 2 = per-turn, 3 = per-step (default: 2).
    #[serde(default = "default_log_level")]
    pub level: u8,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_dir: None,
            level: 2,
        }
    }
}

fn default_log_enabled() -> bool { true }
fn default_log_level() -> u8 { 2 }

fn default_guardian_enabled() -> bool { true }
fn default_doom_loop_threshold() -> usize { 3 }
fn default_stall_timeout_secs() -> u64 { 120 }
fn default_token_budget() -> u64 { 0 }
fn default_token_warn_pct() -> u8 { 80 }

fn default_max_turns() -> usize { 25 }
fn default_max_duration() -> u64 { 600 }
fn default_workspace() -> String { "~/.ryvos".to_string() }
fn default_max_context_tokens() -> usize { 80_000 }
fn default_max_tool_output_tokens() -> usize { 4_000 }
fn default_reflexion_failure_threshold() -> usize { 3 }
fn default_parallel_tools() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    pub model_id: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default)]
    pub thinking: ThinkingLevel,
    #[serde(default)]
    pub retry: Option<RetryConfig>,
}

fn default_provider() -> String { "anthropic".to_string() }
fn default_max_tokens() -> u32 { 8192 }
fn default_temperature() -> f32 { 0.0 }

/// Retry configuration for LLM requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_initial_backoff")]
    pub initial_backoff_ms: u64,
    #[serde(default = "default_max_backoff")]
    pub max_backoff_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            initial_backoff_ms: default_initial_backoff(),
            max_backoff_ms: default_max_backoff(),
        }
    }
}

fn default_max_retries() -> u32 { 3 }
fn default_initial_backoff() -> u64 { 1000 }
fn default_max_backoff() -> u64 { 30000 }

/// Active hours window for heartbeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveHoursConfig {
    /// Start hour (0-23). Default: 9
    #[serde(default = "default_active_start_hour")]
    pub start_hour: u8,
    /// End hour (0-23). Default: 22
    #[serde(default = "default_active_end_hour")]
    pub end_hour: u8,
    /// Simple UTC offset in hours (e.g., 2 for UTC+2). Default: 0
    #[serde(default)]
    pub utc_offset_hours: i32,
}

impl Default for ActiveHoursConfig {
    fn default() -> Self {
        Self {
            start_hour: default_active_start_hour(),
            end_hour: default_active_end_hour(),
            utc_offset_hours: 0,
        }
    }
}

fn default_active_start_hour() -> u8 { 9 }
fn default_active_end_hour() -> u8 { 22 }

/// Heartbeat configuration — periodic proactive agent checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    /// Enable heartbeat (default: false).
    #[serde(default)]
    pub enabled: bool,
    /// Interval between heartbeat checks in seconds. Default: 1800 (30 min).
    #[serde(default = "default_heartbeat_interval")]
    pub interval_secs: u64,
    /// Target channel for alerts (e.g., "telegram"). None = broadcast.
    #[serde(default)]
    pub target_channel: Option<String>,
    /// Restrict heartbeat to active hours window.
    #[serde(default)]
    pub active_hours: Option<ActiveHoursConfig>,
    /// Max response length (chars) to consider as an ack. Default: 300.
    #[serde(default = "default_ack_max_chars")]
    pub ack_max_chars: usize,
    /// Workspace file for heartbeat context. Default: "HEARTBEAT.md".
    #[serde(default = "default_heartbeat_file")]
    pub heartbeat_file: String,
    /// Custom prompt (overrides the default heartbeat prompt).
    #[serde(default)]
    pub prompt: Option<String>,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: default_heartbeat_interval(),
            target_channel: None,
            active_hours: None,
            ack_max_chars: default_ack_max_chars(),
            heartbeat_file: default_heartbeat_file(),
            prompt: None,
        }
    }
}

fn default_heartbeat_interval() -> u64 { 1800 }
fn default_ack_max_chars() -> usize { 300 }
fn default_heartbeat_file() -> String { "HEARTBEAT.md".to_string() }

/// Cron scheduler configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronConfig {
    #[serde(default)]
    pub jobs: Vec<CronJobConfig>,
}

/// A single cron job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobConfig {
    pub name: String,
    pub schedule: String,
    pub prompt: String,
    #[serde(default)]
    pub channel: Option<String>,
}

/// Sandbox configuration for bash tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_sandbox_mode")]
    pub mode: String,
    #[serde(default = "default_sandbox_image")]
    pub image: String,
    #[serde(default = "default_sandbox_memory")]
    pub memory_mb: u64,
    #[serde(default = "default_sandbox_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_mount_workspace")]
    pub mount_workspace: bool,
}

fn default_sandbox_mode() -> String { "docker".to_string() }
fn default_sandbox_image() -> String { "ubuntu:24.04".to_string() }
fn default_sandbox_memory() -> u64 { 512 }
fn default_sandbox_timeout() -> u64 { 120 }
fn default_mount_workspace() -> bool { true }

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: default_sandbox_mode(),
            image: default_sandbox_image(),
            memory_mb: default_sandbox_memory(),
            timeout_secs: default_sandbox_timeout(),
            mount_workspace: default_mount_workspace(),
        }
    }
}

/// Web search provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    #[serde(default = "default_search_provider")]
    pub provider: String,
    pub api_key: String,
}

fn default_search_provider() -> String { "tavily".to_string() }

/// Embedding model configuration for semantic memory search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Provider name: "openai", "ollama", or any OpenAI-compatible API.
    pub provider: String,
    /// Model name (e.g., "text-embedding-3-small", "nomic-embed-text").
    pub model: String,
    /// Base URL for the embedding API (e.g., "http://localhost:11434/v1").
    #[serde(default)]
    pub base_url: Option<String>,
    /// API key (optional, for cloud providers).
    #[serde(default)]
    pub api_key: Option<String>,
    /// Embedding dimensions (default: 1536).
    #[serde(default = "default_embedding_dims")]
    pub dimensions: usize,
}

fn default_embedding_dims() -> usize { 1536 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub api_keys: Vec<ApiKeyConfig>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            token: None,
            password: None,
            api_keys: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    pub name: String,
    pub key: String,
    #[serde(default)]
    pub role: ApiKeyRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyRole {
    /// Read sessions/history only
    Viewer,
    /// Read + send messages
    #[default]
    Operator,
    /// Full access
    Admin,
}

fn default_bind() -> String { "127.0.0.1:18789".to_string() }

/// MCP (Model Context Protocol) configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: std::collections::HashMap<String, McpServerConfig>,
}

/// Configuration for a single MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub transport: McpTransport,
    #[serde(default = "default_auto_connect")]
    pub auto_connect: bool,
    /// Allow server to request LLM inference (sampling). Default: false.
    #[serde(default)]
    pub allow_sampling: bool,
    /// Per-tool-call timeout in seconds. Default: 120.
    #[serde(default = "default_mcp_timeout")]
    pub timeout_secs: u64,
    /// Override security tier for all tools from this server.
    #[serde(default)]
    pub tier_override: Option<String>,
    /// Custom HTTP headers for SSE transport (e.g., auth tokens).
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

fn default_auto_connect() -> bool { true }
fn default_mcp_timeout() -> u64 { 120 }

/// MCP transport configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum McpTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: std::collections::HashMap<String, String>,
    },
    Sse {
        url: String,
    },
}

/// Project-level MCP server config from .mcp.json (OpenClaw-compatible).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpJsonConfig {
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: HashMap<String, McpJsonServerEntry>,
}

/// A single entry in .mcp.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpJsonServerEntry {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
}

impl McpJsonServerEntry {
    /// Convert to McpServerConfig.
    pub fn to_server_config(&self) -> Option<McpServerConfig> {
        let transport = if let Some(ref cmd) = self.command {
            McpTransport::Stdio {
                command: cmd.clone(),
                args: self.args.clone(),
                env: self.env.clone(),
            }
        } else if let Some(ref url) = self.url {
            McpTransport::Sse { url: url.clone() }
        } else {
            return None;
        };

        Some(McpServerConfig {
            transport,
            auto_connect: true,
            allow_sampling: false,
            timeout_secs: default_mcp_timeout(),
            tier_override: None,
            headers: HashMap::new(),
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelsConfig {
    #[serde(default)]
    pub telegram: Option<TelegramConfig>,
    #[serde(default)]
    pub discord: Option<DiscordConfig>,
    #[serde(default)]
    pub slack: Option<SlackConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DmPolicy {
    #[default]
    Allowlist,
    Open,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    #[serde(default)]
    pub allowed_users: Vec<i64>,
    #[serde(default)]
    pub dm_policy: DmPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub bot_token: String,
    #[serde(default)]
    pub dm_policy: DmPolicy,
    #[serde(default)]
    pub allowed_users: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    /// Bot token (xoxb-...) for Web API calls
    pub bot_token: String,
    /// App-level token (xapp-...) for Socket Mode
    pub app_token: String,
    #[serde(default)]
    pub dm_policy: DmPolicy,
    #[serde(default)]
    pub allowed_users: Vec<String>,
}

/// Security configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default = "default_security_auto_approve")]
    pub auto_approve_up_to: SecurityTier,
    #[serde(default)]
    pub deny_above: Option<SecurityTier>,
    #[serde(default = "default_security_timeout")]
    pub approval_timeout_secs: u64,
    #[serde(default)]
    pub tool_overrides: HashMap<String, SecurityTier>,
    #[serde(default = "SecurityPolicy::default_patterns")]
    pub dangerous_patterns: Vec<DangerousPattern>,
    #[serde(default)]
    pub sub_agent_policy: Option<SubAgentPolicyConfig>,
}

fn default_security_auto_approve() -> SecurityTier {
    SecurityTier::T1
}

fn default_security_timeout() -> u64 {
    60
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            auto_approve_up_to: SecurityTier::T1,
            deny_above: None,
            approval_timeout_secs: 60,
            tool_overrides: HashMap::new(),
            dangerous_patterns: SecurityPolicy::default_patterns(),
            sub_agent_policy: None,
        }
    }
}

impl SecurityConfig {
    /// Convert to a SecurityPolicy.
    pub fn to_policy(&self) -> SecurityPolicy {
        SecurityPolicy {
            auto_approve_up_to: self.auto_approve_up_to,
            deny_above: self.deny_above,
            approval_timeout_secs: self.approval_timeout_secs,
            tool_overrides: self.tool_overrides.clone(),
            dangerous_patterns: self.dangerous_patterns.clone(),
        }
    }

    /// Build a restricted SecurityPolicy for sub-agents.
    pub fn sub_agent_policy(&self) -> SecurityPolicy {
        if let Some(ref sub) = self.sub_agent_policy {
            SecurityPolicy {
                auto_approve_up_to: sub.auto_approve_up_to,
                deny_above: sub.deny_above,
                approval_timeout_secs: self.approval_timeout_secs,
                tool_overrides: self.tool_overrides.clone(),
                dangerous_patterns: self.dangerous_patterns.clone(),
            }
        } else {
            self.to_policy()
        }
    }
}

/// Sub-agent security policy overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentPolicyConfig {
    #[serde(default = "default_sub_agent_auto_approve")]
    pub auto_approve_up_to: SecurityTier,
    #[serde(default = "default_sub_agent_deny_above")]
    pub deny_above: Option<SecurityTier>,
}

fn default_sub_agent_auto_approve() -> SecurityTier {
    SecurityTier::T0
}

fn default_sub_agent_deny_above() -> Option<SecurityTier> {
    Some(SecurityTier::T2)
}

impl AppConfig {
    /// Load config from a TOML file, with env var expansion.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|_| RyvosError::ConfigNotFound(path.display().to_string()))?;

        // Expand ${ENV_VAR} references
        let expanded = expand_env_vars(&content);

        toml::from_str(&expanded)
            .map_err(|e| RyvosError::Config(e.to_string()))
    }

    /// Resolve the workspace directory (expand ~).
    pub fn workspace_dir(&self) -> PathBuf {
        let ws = &self.agent.workspace;
        if let Some(rest) = ws.strip_prefix("~/") {
            if let Some(home) = dirs_home() {
                return home.join(rest);
            }
        }
        PathBuf::from(ws)
    }
}

/// Expand `${ENV_VAR}` patterns in a string.
fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut var_name = String::new();
            for c in chars.by_ref() {
                if c == '}' {
                    break;
                }
                var_name.push(c);
            }
            match std::env::var(&var_name) {
                Ok(val) => result.push_str(&val),
                Err(_) => {
                    // Keep original if env var not set
                    result.push_str(&format!("${{{}}}", var_name));
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_vars() {
        std::env::set_var("TEST_RYVOS_VAR", "hello");
        let result = expand_env_vars("key = \"${TEST_RYVOS_VAR}\"");
        assert_eq!(result, "key = \"hello\"");
        std::env::remove_var("TEST_RYVOS_VAR");
    }

    #[test]
    fn test_expand_env_vars_missing() {
        let result = expand_env_vars("key = \"${NONEXISTENT_RYVOS_VAR}\"");
        assert_eq!(result, "key = \"${NONEXISTENT_RYVOS_VAR}\"");
    }

    #[test]
    fn test_agent_config_defaults_from_minimal_toml() {
        let toml_str = r#"
[model]
model_id = "claude-sonnet-4-20250514"
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.max_turns, 25);
        assert!(config.agent.system_prompt.is_none());
        assert_eq!(config.agent.max_context_tokens, 80_000);
        assert_eq!(config.agent.max_tool_output_tokens, 4_000);
        assert_eq!(config.agent.reflexion_failure_threshold, 3);
        assert!(config.agent.parallel_tools);
    }

    #[test]
    fn test_backward_compat_pre_phase3() {
        // Pre-Phase 3 config: no slack, no api_keys, no gateway.api_keys
        let toml_str = r#"
[model]
model_id = "claude-sonnet-4-20250514"
api_key = "sk-test"

[channels.telegram]
bot_token = "123:ABC"
allowed_users = [12345]

[gateway]
bind = "127.0.0.1:18789"
token = "my-token"
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert!(config.channels.slack.is_none());
        assert!(config.gateway.is_some());
        let gw = config.gateway.unwrap();
        assert!(gw.api_keys.is_empty());
        assert_eq!(gw.token, Some("my-token".to_string()));
    }

    #[test]
    fn test_full_config_with_all_phase3_fields() {
        let toml_str = r#"
[model]
model_id = "claude-sonnet-4-20250514"
api_key = "sk-test"

[gateway]
bind = "0.0.0.0:18789"
token = "my-token"

[[gateway.api_keys]]
name = "web-ui"
key = "rk_abc123"
role = "operator"

[[gateway.api_keys]]
name = "admin-key"
key = "rk_xyz789"
role = "admin"

[channels.telegram]
bot_token = "123:ABC"
allowed_users = [12345]

[channels.discord]
bot_token = "discord-token"

[channels.slack]
bot_token = "xoxb-slack"
app_token = "xapp-slack"
dm_policy = "open"
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        let gw = config.gateway.unwrap();
        assert_eq!(gw.api_keys.len(), 2);
        assert_eq!(gw.api_keys[0].name, "web-ui");
        assert_eq!(gw.api_keys[0].role, ApiKeyRole::Operator);
        assert_eq!(gw.api_keys[1].role, ApiKeyRole::Admin);
        assert!(config.channels.slack.is_some());
        let slack = config.channels.slack.unwrap();
        assert_eq!(slack.bot_token, "xoxb-slack");
        assert_eq!(slack.app_token, "xapp-slack");
        assert_eq!(slack.dm_policy, DmPolicy::Open);
    }
}
