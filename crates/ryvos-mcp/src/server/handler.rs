use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};

use ryvos_agent::{FailureJournal, SafetyMemory};
use ryvos_memory::VikingClient;

use super::audit_reader::AuditReader;
use super::tools::*;

/// Ryvos MCP Server handler.
///
/// Exposes Viking memory, file-based memory, audit trail, safety lessons,
/// and failure journal tools to CLI providers via MCP.
#[derive(Clone)]
pub struct RyvosServerHandler {
    pub(crate) viking: Option<Arc<VikingClient>>,
    pub(crate) audit: Option<Arc<AuditReader>>,
    pub(crate) safety_memory: Option<Arc<SafetyMemory>>,
    pub(crate) failure_journal: Option<Arc<FailureJournal>>,
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
            safety_memory: None,
            failure_journal: None,
            workspace,
            tool_router: Self::tool_router(),
        }
    }

    /// Set the safety memory store for lesson inspection.
    pub fn with_safety_memory(mut self, sm: Arc<SafetyMemory>) -> Self {
        self.safety_memory = Some(sm);
        self
    }

    /// Set the failure journal for decision/failure inspection.
    pub fn with_failure_journal(mut self, fj: Arc<FailureJournal>) -> Self {
        self.failure_journal = Some(fj);
        self
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RyvosServerHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation {
                name: "ryvos".into(),
                title: Some("Ryvos Agent".into()),
                version: env!("CARGO_PKG_VERSION").into(),
                description: Some("Persistent agent memory & audit tools".into()),
                icons: None,
                website_url: Some("https://ryvos.dev".into()),
            },
            instructions: Some("Ryvos agent memory & audit tools. Use viking_* to read/write persistent memory, audit_* to inspect tool history.".into()),
        }
    }
}

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
        viking::search(
            viking,
            &params.0.query,
            params.0.directory.as_deref(),
            limit,
        )
        .await
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

    // ── Safety & Healing Introspection Tools ──

    /// List or search safety lessons learned by the agent.
    #[tool(
        name = "safety_lessons_list",
        description = "List or search safety lessons — rules the agent learned from past incidents and corrections."
    )]
    async fn safety_lessons_list(&self, params: Parameters<SafetyLessonsParams>) -> String {
        let Some(ref sm) = self.safety_memory else {
            return "Safety memory not available. Ensure the daemon is running.".to_string();
        };
        let limit = params.0.limit.unwrap_or(20);
        safety::list_lessons(sm, params.0.search.as_deref(), limit).await
    }

    /// Query agent decision audit trail.
    #[tool(
        name = "decisions_query",
        description = "Query the agent's decision audit trail — shows what tools were chosen, alternatives considered, and outcomes."
    )]
    async fn decisions_query(&self, params: Parameters<DecisionsQueryParams>) -> String {
        let Some(ref fj) = self.failure_journal else {
            return "Failure journal not available. Ensure the daemon is running.".to_string();
        };
        let limit = params.0.limit.unwrap_or(20);
        healing::query_decisions(fj, params.0.session_id.as_deref(), limit).await
    }

    /// Search failure patterns in the healing journal.
    #[tool(
        name = "failure_patterns",
        description = "Search failure patterns — shows tool errors, patterns, and context for self-healing analysis."
    )]
    async fn failure_patterns(&self, params: Parameters<FailurePatternsParams>) -> String {
        let Some(ref fj) = self.failure_journal else {
            return "Failure journal not available. Ensure the daemon is running.".to_string();
        };
        let limit = params.0.limit.unwrap_or(20);
        healing::query_failures(fj, params.0.pattern.as_deref(), params.0.tool.as_deref(), limit)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::ServerHandler;

    fn test_handler() -> RyvosServerHandler {
        RyvosServerHandler::new(None, None, std::path::PathBuf::from("/tmp/ryvos-test"))
    }

    #[test]
    fn get_info_server_name() {
        let handler = test_handler();
        let info = handler.get_info();
        assert_eq!(info.server_info.name, "ryvos");
    }

    #[test]
    fn get_info_version_matches_cargo() {
        let handler = test_handler();
        let info = handler.get_info();
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn get_info_capabilities_include_tools() {
        let handler = test_handler();
        let info = handler.get_info();
        assert!(info.capabilities.tools.is_some());
    }

    #[test]
    fn get_info_instructions_present() {
        let handler = test_handler();
        let info = handler.get_info();
        let instructions = info.instructions.expect("instructions should be present");
        assert!(instructions.contains("viking"));
        assert!(instructions.contains("audit"));
    }

    #[test]
    fn get_info_title_and_description() {
        let handler = test_handler();
        let info = handler.get_info();
        assert_eq!(info.server_info.title.as_deref(), Some("Ryvos Agent"));
        assert!(info
            .server_info
            .description
            .as_ref()
            .unwrap()
            .contains("memory"));
    }
}
