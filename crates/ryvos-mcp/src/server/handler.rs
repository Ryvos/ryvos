use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router, ServerHandler};

use ryvos_memory::VikingClient;

use super::audit_reader::AuditReader;
use super::tools::*;

/// Ryvos MCP Server handler.
///
/// Exposes Viking memory, file-based memory, and audit trail tools
/// to CLI providers (claude-code, copilot) via the MCP protocol.
#[derive(Clone)]
pub struct RyvosServerHandler {
    pub(crate) viking: Option<Arc<VikingClient>>,
    pub(crate) audit: Option<Arc<AuditReader>>,
    pub(crate) workspace: std::path::PathBuf,
    tool_router: ToolRouter<Self>,
}

impl RyvosServerHandler {
    /// Create a new Ryvos MCP server handler.
    pub fn new(
        viking: Option<Arc<VikingClient>>,
        audit: Option<Arc<AuditReader>>,
        workspace: std::path::PathBuf,
    ) -> Self {
        Self {
            viking,
            audit,
            workspace,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RyvosServerHandler {}

#[tool_router(router = tool_router)]
impl RyvosServerHandler {
    // ── Viking Memory Tools ──

    /// Search Viking hierarchical memory with semantic + path-based retrieval.
    #[tool(
        name = "viking_search",
        description = "Search Viking hierarchical memory. Returns ranked results with relevance scores."
    )]
    async fn viking_search(&self, params: Parameters<VikingSearchParams>) -> String {
        let Some(ref viking) = self.viking else {
            return "Viking memory not available. Enable [openviking] in config.toml.".to_string();
        };
        let limit = params.0.limit.unwrap_or(10);
        viking::search(viking, &params.0.query, params.0.directory.as_deref(), limit).await
    }

    /// Read a viking:// memory path at L0 (summary), L1 (details), or L2 (full content).
    #[tool(
        name = "viking_read",
        description = "Read a Viking memory path at a specific detail level. L0=summary, L1=details, L2=full."
    )]
    async fn viking_read(&self, params: Parameters<VikingReadParams>) -> String {
        let Some(ref viking) = self.viking else {
            return "Viking memory not available. Enable [openviking] in config.toml.".to_string();
        };
        let level = params.0.level.as_deref().unwrap_or("L1");
        viking::read(viking, &params.0.path, level).await
    }

    /// Write or update a memory entry at a viking:// path.
    #[tool(
        name = "viking_write",
        description = "Write or update a memory entry in Viking. Use for persisting facts, preferences, patterns."
    )]
    async fn viking_write(&self, params: Parameters<VikingWriteParams>) -> String {
        let Some(ref viking) = self.viking else {
            return "Viking memory not available. Enable [openviking] in config.toml.".to_string();
        };
        viking::write(
            viking,
            &params.0.path,
            &params.0.content,
            params.0.tags.as_deref(),
        )
        .await
    }

    /// List Viking memory directory contents with summaries.
    #[tool(
        name = "viking_list",
        description = "List Viking memory directory structure. Returns paths with L0 summaries."
    )]
    async fn viking_list(&self, params: Parameters<VikingListParams>) -> String {
        let Some(ref viking) = self.viking else {
            return "Viking memory not available. Enable [openviking] in config.toml.".to_string();
        };
        let path = params.0.path.as_deref().unwrap_or("viking://");
        viking::list(viking, path).await
    }

    // ── File-Based Memory Tools ──

    /// Read MEMORY.md or a named memory file from the workspace.
    #[tool(
        name = "memory_get",
        description = "Read a memory file. Without a name, reads MEMORY.md. With a name, reads memory/{name}.md."
    )]
    async fn memory_get(&self, params: Parameters<MemoryGetParams>) -> String {
        memory::get(&self.workspace, params.0.name.as_deref())
    }

    /// Append a timestamped note to MEMORY.md.
    #[tool(
        name = "memory_write",
        description = "Append a timestamped note to persistent memory (MEMORY.md)."
    )]
    async fn memory_write(&self, params: Parameters<MemoryWriteParams>) -> String {
        memory::write(&self.workspace, &params.0.note)
    }

    /// Append a timestamped entry to today's daily log.
    #[tool(
        name = "daily_log_write",
        description = "Append a timestamped entry to today's daily log (memory/YYYY-MM-DD.md)."
    )]
    async fn daily_log_write(&self, params: Parameters<DailyLogWriteParams>) -> String {
        memory::daily_log_write(&self.workspace, &params.0.entry)
    }

    // ── Audit Trail Tools ──

    /// Query recent tool executions from the audit trail.
    #[tool(
        name = "audit_query",
        description = "Query recent tool executions — shows tool name, input, outcome, and timestamp."
    )]
    async fn audit_query(&self, params: Parameters<AuditQueryParams>) -> String {
        let Some(ref audit) = self.audit else {
            return "Audit trail not available. Ensure the daemon is running.".to_string();
        };
        let limit = params.0.limit.unwrap_or(20);
        audit::query(audit, limit).await
    }

    /// Get aggregate tool call statistics from the audit trail.
    #[tool(
        name = "audit_stats",
        description = "Get aggregate tool call statistics — counts per tool, total calls."
    )]
    async fn audit_stats(&self) -> String {
        let Some(ref audit) = self.audit else {
            return "Audit trail not available. Ensure the daemon is running.".to_string();
        };
        audit::stats(audit).await
    }
}
