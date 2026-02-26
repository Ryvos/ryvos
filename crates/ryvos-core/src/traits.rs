use futures::future::BoxFuture;
use futures::stream::BoxStream;
use tokio::sync::mpsc;

use crate::config::ModelConfig;
use crate::error::Result;
use crate::types::*;

/// LLM client — multi-provider streaming.
pub trait LlmClient: Send + Sync + 'static {
    /// Send a chat request and receive a stream of deltas.
    fn chat_stream(
        &self,
        config: &ModelConfig,
        messages: Vec<ChatMessage>,
        tools: &[ToolDefinition],
    ) -> BoxFuture<'_, Result<BoxStream<'_, Result<StreamDelta>>>>;
}

/// Tool — extensible tool execution.
pub trait Tool: Send + Sync + 'static {
    /// Tool name (used in LLM tool calls).
    fn name(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str;

    /// JSON Schema for tool input.
    fn input_schema(&self) -> serde_json::Value;

    /// Execute the tool with given input and context.
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>>;

    /// Timeout in seconds for this tool.
    fn timeout_secs(&self) -> u64 {
        30
    }

    /// Whether this tool requires sandboxed execution.
    fn requires_sandbox(&self) -> bool {
        false
    }

    /// Security tier for this tool (default: T1 — workspace writes).
    fn tier(&self) -> crate::security::SecurityTier {
        crate::security::SecurityTier::T1
    }
}

/// Channel adapter — multi-platform messaging.
pub trait ChannelAdapter: Send + Sync + 'static {
    /// Adapter name (e.g., "telegram", "discord").
    fn name(&self) -> &str;

    /// Start receiving messages, sending them via the provided sender.
    fn start(&self, tx: mpsc::Sender<MessageEnvelope>) -> BoxFuture<'_, Result<()>>;

    /// Send a message to a session.
    fn send(&self, session: &SessionId, content: &MessageContent) -> BoxFuture<'_, Result<()>>;

    /// Send an approval request with platform-native interactive UI (buttons).
    /// Returns true if the adapter handled it with rich UI, false to fall back to text.
    fn send_approval(
        &self,
        session: &SessionId,
        request: &crate::security::ApprovalRequest,
    ) -> BoxFuture<'_, Result<bool>> {
        let _ = (session, request);
        Box::pin(async { Ok(false) })
    }

    /// Stop the adapter gracefully.
    fn stop(&self) -> BoxFuture<'_, Result<()>>;
}

/// Session store — persistence backend.
pub trait SessionStore: Send + Sync + 'static {
    /// Append messages to a session.
    fn append_messages(&self, sid: &SessionId, msgs: &[ChatMessage]) -> BoxFuture<'_, Result<()>>;

    /// Load message history for a session.
    fn load_history(
        &self,
        sid: &SessionId,
        limit: usize,
    ) -> BoxFuture<'_, Result<Vec<ChatMessage>>>;

    /// Full-text search across all sessions.
    fn search(&self, query: &str, limit: usize) -> BoxFuture<'_, Result<Vec<SearchResult>>>;
}
