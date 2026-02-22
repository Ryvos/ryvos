use thiserror::Error;

#[derive(Debug, Error)]
pub enum RyvosError {
    // LLM errors
    #[error("LLM request failed: {0}")]
    LlmRequest(String),

    #[error("LLM streaming error: {0}")]
    LlmStream(String),

    #[error("LLM provider not supported: {0}")]
    UnsupportedProvider(String),

    #[error("LLM response parse error: {0}")]
    LlmParse(String),

    // Tool errors
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Tool execution failed: {tool}: {message}")]
    ToolExecution { tool: String, message: String },

    #[error("Tool timeout after {timeout_secs}s: {tool}")]
    ToolTimeout { tool: String, timeout_secs: u64 },

    #[error("Tool input validation failed: {0}")]
    ToolValidation(String),

    // Agent errors
    #[error("Agent exceeded max turns ({0})")]
    MaxTurnsExceeded(usize),

    #[error("Agent exceeded max duration ({0}s)")]
    MaxDurationExceeded(u64),

    #[error("Agent cancelled")]
    Cancelled,

    // Config errors
    #[error("Config error: {0}")]
    Config(String),

    #[error("Config file not found: {0}")]
    ConfigNotFound(String),

    // Storage errors
    #[error("Database error: {0}")]
    Database(String),

    // Channel errors
    #[error("Channel error: {channel}: {message}")]
    Channel { channel: String, message: String },

    // Gateway errors
    #[error("Gateway error: {0}")]
    Gateway(String),

    // Security errors
    #[error("Tool blocked by security policy: {tool} (tier {tier})")]
    ToolBlocked { tool: String, tier: String },

    #[error("Approval denied for tool {tool}: {reason}")]
    ApprovalDenied { tool: String, reason: String },

    #[error("Approval timeout for tool: {tool}")]
    ApprovalTimeout { tool: String },

    // MCP errors
    #[error("MCP error: {0}")]
    Mcp(String),

    // I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    // JSON errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, RyvosError>;
