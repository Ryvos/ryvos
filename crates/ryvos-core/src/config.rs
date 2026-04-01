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

/// Daily log configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyLogsConfig {
    #[serde(default = "default_daily_logs_enabled")]
    pub enabled: bool,
    #[serde(default = "default_daily_logs_retention")]
    pub retention_days: u32,
    #[serde(default)]
    pub log_dir: Option<String>,
}

fn default_daily_logs_enabled() -> bool {
    true
}
fn default_daily_logs_retention() -> u32 {
    30
}

impl Default for DailyLogsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 30,
            log_dir: None,
        }
    }
}

/// Skill registry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    #[serde(default = "default_registry_url")]
    pub url: String,
    #[serde(default)]
    pub cache_dir: Option<String>,
}

fn default_registry_url() -> String {
    "https://raw.githubusercontent.com/Ryvos/registry/main/index.json".to_string()
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            url: default_registry_url(),
            cache_dir: None,
        }
    }
}

/// Dollar-based budget configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Monthly budget in cents (0 = unlimited).
    pub monthly_budget_cents: u64,
    /// Soft warning at this percentage of budget.
    #[serde(default = "default_warn_pct")]
    pub warn_pct: u8,
    /// Hard stop at this percentage of budget.
    #[serde(default = "default_hard_stop_pct")]
    pub hard_stop_pct: u8,
    /// Per-model pricing overrides (cents per million tokens).
    #[serde(default)]
    pub pricing: HashMap<String, ModelPricing>,
}

fn default_warn_pct() -> u8 {
    80
}
fn default_hard_stop_pct() -> u8 {
    100
}

/// Per-model pricing override (cents per million tokens).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_cents_per_mtok: u64,
    pub output_cents_per_mtok: u64,
}

/// Webhook configuration for the gateway.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebhookConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: Option<String>,
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
    #[serde(default)]
    pub daily_logs: Option<DailyLogsConfig>,
    #[serde(default)]
    pub registry: Option<RegistryConfig>,
    #[serde(default)]
    pub budget: Option<BudgetConfig>,
    /// OpenViking hierarchical memory configuration.
    #[serde(default)]
    pub openviking: Option<OpenVikingConfig>,
    /// Google Workspace integration.
    #[serde(default)]
    pub google: Option<GoogleConfig>,
    /// Notion integration.
    #[serde(default)]
    pub notion: Option<NotionConfig>,
    /// Jira integration.
    #[serde(default)]
    pub jira: Option<JiraConfig>,
    /// Linear integration.
    #[serde(default)]
    pub linear: Option<LinearConfig>,
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
    /// Per-agent model routing overrides (agent_id → model config).
    #[serde(default)]
    pub model_overrides: HashMap<String, ModelConfig>,
    /// Opt-out of memory flush before context compaction.
    #[serde(default)]
    pub disable_memory_flush: Option<bool>,
    /// Director orchestration configuration.
    #[serde(default)]
    pub director: Option<DirectorConfig>,
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
            model_overrides: HashMap::new(),
            disable_memory_flush: None,
            director: Some(DirectorConfig::default()),
        }
    }
}

fn default_enable_summarization() -> bool {
    true
}

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

fn default_checkpoint_enabled() -> bool {
    true
}

/// Director orchestration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectorConfig {
    /// Enable Director orchestration (default: true).
    #[serde(default = "default_director_enabled")]
    pub enabled: bool,
    /// Maximum evolution cycles before giving up (default: 3).
    #[serde(default = "default_max_evolution_cycles")]
    pub max_evolution_cycles: u32,
    /// Number of semantic failures before triggering evolution (default: 3).
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: usize,
    /// Model override for the Director's planning LLM (defaults to main model).
    #[serde(default)]
    pub model: Option<ModelConfig>,
}

fn default_director_enabled() -> bool {
    true
}
fn default_max_evolution_cycles() -> u32 {
    3
}
fn default_failure_threshold() -> usize {
    3
}

impl Default for DirectorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_evolution_cycles: default_max_evolution_cycles(),
            failure_threshold: default_failure_threshold(),
            model: None,
        }
    }
}

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

fn default_log_enabled() -> bool {
    true
}
fn default_log_level() -> u8 {
    2
}

fn default_guardian_enabled() -> bool {
    true
}
fn default_doom_loop_threshold() -> usize {
    3
}
fn default_stall_timeout_secs() -> u64 {
    120
}
fn default_token_budget() -> u64 {
    0
}
fn default_token_warn_pct() -> u8 {
    80
}

fn default_max_turns() -> usize {
    25
}
fn default_max_duration() -> u64 {
    600
}
fn default_workspace() -> String {
    "~/.ryvos".to_string()
}
fn default_max_context_tokens() -> usize {
    80_000
}
fn default_max_tool_output_tokens() -> usize {
    4_000
}
fn default_reflexion_failure_threshold() -> usize {
    3
}
fn default_parallel_tools() -> bool {
    true
}

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
    /// Azure OpenAI resource name (e.g., "my-resource").
    #[serde(default)]
    pub azure_resource: Option<String>,
    /// Azure OpenAI deployment name.
    #[serde(default)]
    pub azure_deployment: Option<String>,
    /// Azure OpenAI API version (e.g., "2024-02-15-preview").
    #[serde(default)]
    pub azure_api_version: Option<String>,
    /// AWS region for Bedrock (e.g., "us-east-1").
    #[serde(default)]
    pub aws_region: Option<String>,
    /// Extra headers to send with every LLM request.
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    /// Path to claude CLI binary (for claude-code provider).
    #[serde(default)]
    pub claude_command: Option<String>,
    /// Allowed tools for Claude CLI subprocess (for claude-code provider).
    /// When set, replaces --dangerously-skip-permissions with --allowedTools.
    /// Example: ["Read", "Glob", "Grep", "WebSearch", "WebFetch"]
    #[serde(default)]
    pub cli_allowed_tools: Vec<String>,
    /// Permission mode for Claude CLI subprocess (default: "plan" for read-only).
    /// Options: "default", "plan", "dontAsk", "bypassPermissions"
    #[serde(default)]
    pub cli_permission_mode: Option<String>,
    /// Path to copilot CLI binary (for copilot provider).
    #[serde(default)]
    pub copilot_command: Option<String>,
    /// Runtime-only: CLI session ID for --resume (not serialized to config).
    #[serde(skip)]
    pub cli_session_id: Option<String>,
}

fn default_provider() -> String {
    "anthropic".to_string()
}
fn default_max_tokens() -> u32 {
    8192
}
fn default_temperature() -> f32 {
    0.0
}

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

fn default_max_retries() -> u32 {
    3
}
fn default_initial_backoff() -> u64 {
    1000
}
fn default_max_backoff() -> u64 {
    30000
}

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

fn default_active_start_hour() -> u8 {
    9
}
fn default_active_end_hour() -> u8 {
    22
}

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

fn default_heartbeat_interval() -> u64 {
    1800
}
fn default_ack_max_chars() -> usize {
    300
}
fn default_heartbeat_file() -> String {
    "HEARTBEAT.md".to_string()
}

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

fn default_sandbox_mode() -> String {
    "docker".to_string()
}
fn default_sandbox_image() -> String {
    "ubuntu:24.04".to_string()
}
fn default_sandbox_memory() -> u64 {
    512
}
fn default_sandbox_timeout() -> u64 {
    120
}
fn default_mount_workspace() -> bool {
    true
}

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

fn default_search_provider() -> String {
    "tavily".to_string()
}

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

fn default_embedding_dims() -> usize {
    1536
}

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
    #[serde(default)]
    pub webhooks: Option<WebhookConfig>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            token: None,
            password: None,
            api_keys: vec![],
            webhooks: None,
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

fn default_bind() -> String {
    "127.0.0.1:18789".to_string()
}

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

fn default_auto_connect() -> bool {
    true
}
fn default_mcp_timeout() -> u64 {
    120
}

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
    #[serde(default)]
    pub whatsapp: Option<WhatsAppConfig>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppConfig {
    /// Permanent access token from Meta Business.
    pub access_token: String,
    /// Phone number ID from Meta Business.
    pub phone_number_id: String,
    /// Verify token for webhook handshake (you choose this).
    pub verify_token: String,
    #[serde(default)]
    pub dm_policy: DmPolicy,
    /// Allowed phone numbers (E.164 format, e.g., "15551234567").
    #[serde(default)]
    pub allowed_users: Vec<String>,
}

/// Security configuration — self-learning safety model.
///
/// No tools are ever blocked. Safety comes from constitutional self-governance,
/// safety memory (Reflexion), and post-hoc accountability via audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// **Deprecated.** Retained for config backward compat. No effect.
    #[serde(default = "default_security_auto_approve")]
    pub auto_approve_up_to: SecurityTier,
    /// **Deprecated.** Retained for config backward compat. No effect.
    #[serde(default)]
    pub deny_above: Option<SecurityTier>,
    /// Timeout in seconds for soft checkpoint acknowledgment.
    #[serde(default = "default_security_timeout")]
    pub approval_timeout_secs: u64,
    /// Per-tool tier overrides. Retained for config compat.
    #[serde(default)]
    pub tool_overrides: HashMap<String, SecurityTier>,
    /// **Deprecated.** Regex patterns no longer block tools.
    #[serde(default)]
    pub dangerous_patterns: Vec<DangerousPattern>,
    /// Sub-agent policy overrides. Retained for config compat.
    #[serde(default)]
    pub sub_agent_policy: Option<SubAgentPolicyConfig>,
    /// Optional soft checkpoints: tools listed here pause to explain
    /// reasoning before executing. The agent is NEVER blocked.
    #[serde(default)]
    pub pause_before: Vec<String>,
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
            deny_above: None, // Nothing denied
            approval_timeout_secs: 60,
            tool_overrides: HashMap::new(),
            dangerous_patterns: vec![],
            sub_agent_policy: None,
            pause_before: vec![],
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
            pause_before: self.pause_before.clone(),
        }
    }

    /// Build a SecurityPolicy for sub-agents (same as parent — no restrictions).
    pub fn sub_agent_policy(&self) -> SecurityPolicy {
        self.to_policy()
    }
}

/// Sub-agent security policy overrides. Retained for config backward compat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentPolicyConfig {
    #[serde(default = "default_sub_agent_auto_approve")]
    pub auto_approve_up_to: SecurityTier,
    #[serde(default)]
    pub deny_above: Option<SecurityTier>,
}

fn default_sub_agent_auto_approve() -> SecurityTier {
    SecurityTier::T0
}

/// OpenViking configuration for hierarchical memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenVikingConfig {
    /// Enable OpenViking integration (default: false).
    #[serde(default)]
    pub enabled: bool,
    /// Base URL for the OpenViking service.
    #[serde(default = "default_viking_url")]
    pub base_url: String,
    /// User ID for Viking (default: "ryvos-default").
    #[serde(default = "default_viking_user")]
    pub user_id: String,
    /// Auto-extract memories after sessions (default: true).
    #[serde(default = "default_auto_iterate")]
    pub auto_iterate: bool,
}

fn default_viking_url() -> String {
    "http://localhost:1933".to_string()
}
fn default_viking_user() -> String {
    "ryvos-default".to_string()
}
fn default_auto_iterate() -> bool {
    true
}

impl Default for OpenVikingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_viking_url(),
            user_id: default_viking_user(),
            auto_iterate: true,
        }
    }
}

/// Google Workspace integration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleConfig {
    /// Path to OAuth client_secret.json file.
    pub client_secret_path: String,
    /// Path to stored OAuth tokens.
    #[serde(default = "default_google_tokens_path")]
    pub tokens_path: String,
    /// Enable Gmail tools.
    #[serde(default = "default_true")]
    pub gmail: bool,
    /// Enable Calendar tools.
    #[serde(default = "default_true")]
    pub calendar: bool,
    /// Enable Drive tools.
    #[serde(default = "default_true")]
    pub drive: bool,
    /// Enable Contacts tools.
    #[serde(default)]
    pub contacts: bool,
}

fn default_google_tokens_path() -> String {
    "~/.ryvos/credentials/google/tokens.json".to_string()
}
fn default_true() -> bool {
    true
}

/// Notion integration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotionConfig {
    /// Notion API key (ntn_...).
    pub api_key: String,
}

/// Jira integration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JiraConfig {
    /// Atlassian instance URL (e.g., "https://myorg.atlassian.net").
    pub base_url: String,
    /// User email for Jira API auth.
    pub email: String,
    /// API token from id.atlassian.com.
    pub api_token: String,
}

/// Linear integration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearConfig {
    /// Linear API key from linear.app/settings/api.
    pub api_key: String,
}

impl AppConfig {
    /// Load config from a TOML file, with env var expansion.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|_| RyvosError::ConfigNotFound(path.display().to_string()))?;

        // Expand ${ENV_VAR} references
        let expanded = expand_env_vars(&content);

        toml::from_str(&expanded).map_err(|e| RyvosError::Config(e.to_string()))
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
    fn test_director_config_defaults() {
        let toml_str = r#"
[model]
model_id = "claude-sonnet-4-20250514"
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        let director = config.agent.director.unwrap();
        assert!(director.enabled);
        assert_eq!(director.max_evolution_cycles, 3);
        assert_eq!(director.failure_threshold, 3);
    }

    #[test]
    fn test_director_config_explicit() {
        let toml_str = r#"
[model]
model_id = "claude-sonnet-4-20250514"

[agent.director]
enabled = true
max_evolution_cycles = 5
failure_threshold = 2
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        let director = config.agent.director.unwrap();
        assert!(director.enabled);
        assert_eq!(director.max_evolution_cycles, 5);
        assert_eq!(director.failure_threshold, 2);
        assert!(director.model.is_none());
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

    #[test]
    fn test_cli_allowed_tools_defaults_empty() {
        let toml_str = r#"
[model]
model_id = "claude-sonnet-4-20250514"
provider = "claude-code"
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert!(config.model.cli_allowed_tools.is_empty());
        assert!(config.model.cli_permission_mode.is_none());
    }

    #[test]
    fn test_cli_allowed_tools_from_toml() {
        let toml_str = r#"
[model]
model_id = "claude-sonnet-4-20250514"
provider = "claude-code"
cli_allowed_tools = ["Read", "Glob", "Grep"]
cli_permission_mode = "dontAsk"
"#;
        let config: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.model.cli_allowed_tools, vec!["Read", "Glob", "Grep"]);
        assert_eq!(config.model.cli_permission_mode.as_deref(), Some("dontAsk"));
    }
}
