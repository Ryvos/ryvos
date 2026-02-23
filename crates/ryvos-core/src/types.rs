use std::sync::Arc;

use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::security::{ApprovalRequest, SecurityTier};

/// Unique session identifier.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_str(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Role in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A single content block in a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },

    #[serde(rename = "thinking")]
    Thinking { thinking: String },
}

/// A chat message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,
}

impl ChatMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            timestamp: Some(Utc::now()),
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::Text { text: text.into() }],
            timestamp: Some(Utc::now()),
        }
    }

    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<String>, is_error: bool) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
                is_error,
            }],
            timestamp: Some(Utc::now()),
        }
    }

    /// Extract all text content from this message.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Extract all tool use blocks from this message.
    pub fn tool_uses(&self) -> Vec<(&str, &str, &serde_json::Value)> {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => Some((id.as_str(), name.as_str(), input)),
                _ => None,
            })
            .collect()
    }
}

/// Stop reason from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
}

/// A streaming delta from the LLM.
#[derive(Debug, Clone)]
pub enum StreamDelta {
    /// A chunk of text content.
    TextDelta(String),

    /// A chunk of thinking/reasoning content.
    ThinkingDelta(String),

    /// Start of a tool use block.
    ToolUseStart {
        index: usize,
        id: String,
        name: String,
    },

    /// A chunk of tool use input JSON.
    ToolInputDelta { index: usize, delta: String },

    /// The response is complete.
    Stop(StopReason),

    /// Usage information.
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },

    /// Message ID from the API.
    MessageId(String),
}

/// Result of a tool execution.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

/// Tool definition for sending to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Context passed to tools during execution.
#[derive(Clone)]
pub struct ToolContext {
    pub session_id: SessionId,
    pub working_dir: std::path::PathBuf,
    pub store: Option<Arc<dyn crate::traits::SessionStore>>,
    pub agent_spawner: Option<Arc<dyn AgentSpawner>>,
    pub sandbox_config: Option<crate::config::SandboxConfig>,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("session_id", &self.session_id)
            .field("working_dir", &self.working_dir)
            .field("store", &self.store.is_some())
            .field("agent_spawner", &self.agent_spawner.is_some())
            .field("sandbox_config", &self.sandbox_config)
            .finish()
    }
}

/// Trait for spawning sub-agents without circular dependencies.
pub trait AgentSpawner: Send + Sync + 'static {
    fn spawn(&self, prompt: String) -> BoxFuture<'_, crate::error::Result<String>>;
}

/// An incoming message from any channel.
#[derive(Debug, Clone)]
pub struct MessageEnvelope {
    pub id: String,
    pub session_id: SessionId,
    pub channel: String,
    pub sender: String,
    pub text: String,
    pub timestamp: DateTime<Utc>,
}

/// Content for outgoing messages.
#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    Streaming { delta: String, done: bool },
}

/// Search result from memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub rank: f64,
}

/// Agent event broadcast to all subscribers.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Agent run started.
    RunStarted { session_id: SessionId },
    /// Text streaming from LLM.
    TextDelta(String),
    /// Tool execution started.
    ToolStart { name: String, input: serde_json::Value },
    /// Tool execution completed.
    ToolEnd { name: String, result: ToolResult },
    /// Agent turn completed.
    TurnComplete { turn: usize },
    /// Agent run completed.
    RunComplete {
        session_id: SessionId,
        total_turns: usize,
        input_tokens: u64,
        output_tokens: u64,
    },
    /// Agent run failed.
    RunError { error: String },
    /// Cron job fired.
    CronFired { job_id: String, prompt: String },
    /// Approval requested for a tool call.
    ApprovalRequested { request: ApprovalRequest },
    /// Approval resolved (approved or denied).
    ApprovalResolved { request_id: String, approved: bool },
    /// Tool blocked by security policy.
    ToolBlocked { name: String, tier: SecurityTier, reason: String },
    /// Guardian detected a stall (no progress for N seconds).
    GuardianStall { session_id: SessionId, turn: usize, elapsed_secs: u64 },
    /// Guardian detected a doom loop (same tool called repeatedly).
    GuardianDoomLoop { session_id: SessionId, tool_name: String, consecutive_calls: usize },
    /// Guardian budget alert (soft warning or hard stop).
    GuardianBudgetAlert { session_id: SessionId, used_tokens: u64, budget_tokens: u64, is_hard_stop: bool },
    /// Guardian injected a corrective hint.
    GuardianHint { session_id: SessionId, message: String },
    /// Token usage update from the agent loop.
    UsageUpdate { input_tokens: u64, output_tokens: u64 },
}

/// Thinking level for extended thinking / reasoning tokens.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl ThinkingLevel {
    /// Budget tokens for Anthropic extended thinking.
    pub fn budget_tokens(&self) -> u32 {
        match self {
            Self::Off => 0,
            Self::Low => 4096,
            Self::Medium => 10240,
            Self::High => 32768,
        }
    }

    /// Reasoning effort string for OpenAI o-series models.
    pub fn reasoning_effort(&self) -> &str {
        match self {
            Self::Off => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}
