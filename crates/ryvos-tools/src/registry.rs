use std::collections::HashMap;
use std::sync::Arc;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolDefinition, ToolResult};

/// Registry of available tools.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool.
    pub fn register(&mut self, tool: impl Tool) {
        let name = tool.name().to_string();
        self.tools.insert(name, Arc::new(tool));
    }

    /// Unregister a tool by name.
    pub fn unregister(&mut self, name: &str) -> bool {
        self.tools.remove(name).is_some()
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// List all registered tools.
    pub fn list(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Get tool definitions for sending to the LLM.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Execute a tool by name.
    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult> {
        let tool = self
            .get(name)
            .ok_or_else(|| RyvosError::ToolNotFound(name.to_string()))?;

        let timeout = std::time::Duration::from_secs(tool.timeout_secs());

        match tokio::time::timeout(timeout, tool.execute(input, ctx)).await {
            Ok(result) => result,
            Err(_) => Err(RyvosError::ToolTimeout {
                tool: name.to_string(),
                timeout_secs: tool.timeout_secs(),
            }),
        }
    }

    /// Create a registry with all built-in tools registered.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        // ── Original 12 tools ───────────────────────────────────
        registry.register(crate::builtin::bash::BashTool);
        registry.register(crate::builtin::read::ReadTool);
        registry.register(crate::builtin::write::WriteTool);
        registry.register(crate::builtin::edit::EditTool);
        registry.register(crate::builtin::memory_search::MemorySearchTool);
        registry.register(crate::builtin::memory_write::MemoryWriteTool);
        registry.register(crate::builtin::spawn_agent::SpawnAgentTool);
        registry.register(crate::builtin::glob::GlobTool);
        registry.register(crate::builtin::grep::GrepTool);
        registry.register(crate::builtin::web_fetch::WebFetchTool);
        registry.register(crate::builtin::apply_patch::ApplyPatchTool);

        // ── Sessions (5) ────────────────────────────────────────
        registry.register(crate::builtin::sessions::SessionListTool);
        registry.register(crate::builtin::sessions::SessionHistoryTool);
        registry.register(crate::builtin::sessions::SessionSendTool);
        registry.register(crate::builtin::sessions::SessionSpawnTool);
        registry.register(crate::builtin::sessions::SessionStatusTool);

        // ── Memory (3) ──────────────────────────────────────────
        registry.register(crate::builtin::memory::MemoryGetTool);
        registry.register(crate::builtin::memory::DailyLogWriteTool);
        registry.register(crate::builtin::memory::MemoryDeleteTool);

        // ── File System (9) ─────────────────────────────────────
        registry.register(crate::builtin::filesystem::FileInfoTool);
        registry.register(crate::builtin::filesystem::FileCopyTool);
        registry.register(crate::builtin::filesystem::FileMoveTool);
        registry.register(crate::builtin::filesystem::FileDeleteTool);
        registry.register(crate::builtin::filesystem::DirListTool);
        registry.register(crate::builtin::filesystem::DirCreateTool);
        registry.register(crate::builtin::filesystem::FileWatchTool);
        registry.register(crate::builtin::filesystem::ArchiveCreateTool);
        registry.register(crate::builtin::filesystem::ArchiveExtractTool);

        // ── Git (6) ─────────────────────────────────────────────
        registry.register(crate::builtin::git::GitStatusTool);
        registry.register(crate::builtin::git::GitDiffTool);
        registry.register(crate::builtin::git::GitLogTool);
        registry.register(crate::builtin::git::GitCommitTool);
        registry.register(crate::builtin::git::GitBranchTool);
        registry.register(crate::builtin::git::GitCloneTool);

        // ── Code/Dev (4) ────────────────────────────────────────
        registry.register(crate::builtin::code::CodeFormatTool);
        registry.register(crate::builtin::code::CodeLintTool);
        registry.register(crate::builtin::code::TestRunTool);
        registry.register(crate::builtin::code::CodeOutlineTool);

        // ── Network/HTTP (4) ────────────────────────────────────
        registry.register(crate::builtin::network::HttpRequestTool);
        registry.register(crate::builtin::network::HttpDownloadTool);
        registry.register(crate::builtin::network::DnsLookupTool);
        registry.register(crate::builtin::network::NetworkCheckTool);

        // ── System (5) ──────────────────────────────────────────
        registry.register(crate::builtin::system::ProcessListTool);
        registry.register(crate::builtin::system::ProcessKillTool);
        registry.register(crate::builtin::system::EnvGetTool);
        registry.register(crate::builtin::system::SystemInfoTool);
        registry.register(crate::builtin::system::DiskUsageTool);

        // ── Data/Transform (8) ──────────────────────────────────
        registry.register(crate::builtin::data::JsonQueryTool);
        registry.register(crate::builtin::data::CsvParseTool);
        registry.register(crate::builtin::data::YamlConvertTool);
        registry.register(crate::builtin::data::TomlConvertTool);
        registry.register(crate::builtin::data::Base64CodecTool);
        registry.register(crate::builtin::data::HashComputeTool);
        registry.register(crate::builtin::data::RegexReplaceTool);
        registry.register(crate::builtin::data::TextDiffTool);

        // ── Scheduling (3) ──────────────────────────────────────
        registry.register(crate::builtin::scheduling::CronListTool);
        registry.register(crate::builtin::scheduling::CronAddTool);
        registry.register(crate::builtin::scheduling::CronRemoveTool);

        // ── Database (2) ────────────────────────────────────────
        registry.register(crate::builtin::database::SqliteQueryTool);
        registry.register(crate::builtin::database::SqliteSchemaTool);

        // ── Communication (1) ───────────────────────────────────
        registry.register(crate::builtin::notification::NotificationSendTool);

        // ── Browser Automation (5) ──────────────────────────────
        crate::builtin::browser::register_browser_tools(&mut registry);

        // ── Viking Memory (4) — tools check for Viking at execution time ──
        registry.register(crate::builtin::viking::VikingSearchTool);
        registry.register(crate::builtin::viking::VikingReadTool);
        registry.register(crate::builtin::viking::VikingWriteTool);
        registry.register(crate::builtin::viking::VikingListTool);

        // Disabled: use MCP servers instead
        // // ── Google Workspace (6) ────────────────────────────────
        // registry.register(crate::builtin::google::GmailInboxTool);
        // registry.register(crate::builtin::google::GmailReadTool);
        // registry.register(crate::builtin::google::GmailSendTool);
        // registry.register(crate::builtin::google::CalendarListTool);
        // registry.register(crate::builtin::google::CalendarCreateTool);
        // registry.register(crate::builtin::google::DriveSearchTool);

        // Disabled: use MCP servers instead
        // // ── Notion (4) ─────────────────────────────────────────
        // registry.register(crate::builtin::notion::NotionSearchTool);
        // registry.register(crate::builtin::notion::NotionReadPageTool);
        // registry.register(crate::builtin::notion::NotionCreatePageTool);
        // registry.register(crate::builtin::notion::NotionQueryDatabaseTool);

        // Disabled: use MCP servers instead
        // // ── Jira (3) ───────────────────────────────────────────
        // registry.register(crate::builtin::jira::JiraSearchTool);
        // registry.register(crate::builtin::jira::JiraCreateIssueTool);
        // registry.register(crate::builtin::jira::JiraUpdateIssueTool);

        // Disabled: use MCP servers instead
        // // ── Linear (3) ─────────────────────────────────────────
        // registry.register(crate::builtin::linear::LinearSearchTool);
        // registry.register(crate::builtin::linear::LinearCreateIssueTool);
        // registry.register(crate::builtin::linear::LinearListProjectsTool);

        registry
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryvos_test_utils::{test_tool_context, MockTool};

    #[test]
    fn registry_register_and_get() {
        let mut registry = ToolRegistry::new();
        registry.register(MockTool::new("my_tool"));
        assert!(registry.get("my_tool").is_some());
        assert!(registry.get("other").is_none());
    }

    #[test]
    fn registry_unregister() {
        let mut registry = ToolRegistry::new();
        registry.register(MockTool::new("removable"));
        assert!(registry.unregister("removable"));
        assert!(registry.get("removable").is_none());
        // Unregistering non-existent returns false
        assert!(!registry.unregister("removable"));
    }

    #[test]
    fn registry_list_returns_all_names() {
        let mut registry = ToolRegistry::new();
        registry.register(MockTool::new("alpha"));
        registry.register(MockTool::new("beta"));
        registry.register(MockTool::new("gamma"));
        let mut names = registry.list();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn registry_definitions_returns_all() {
        let mut registry = ToolRegistry::new();
        registry.register(MockTool::new("tool_a").with_description("desc A"));
        registry.register(MockTool::new("tool_b").with_description("desc B"));
        let defs = registry.definitions();
        assert_eq!(defs.len(), 2);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"tool_a"));
        assert!(names.contains(&"tool_b"));
    }

    #[tokio::test]
    async fn registry_execute_calls_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(MockTool::new("exec_tool"));
        let ctx = test_tool_context();
        let result = registry
            .execute("exec_tool", serde_json::json!({"x": 1}), ctx)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content, "mock output");
    }

    #[tokio::test]
    async fn registry_execute_unknown_tool_errors() {
        let registry = ToolRegistry::new();
        let ctx = test_tool_context();
        let result = registry
            .execute("no_such_tool", serde_json::json!({}), ctx)
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn registry_with_builtins_has_tools() {
        let registry = ToolRegistry::with_builtins();
        let names = registry.list();
        // Spot-check a few known builtins
        assert!(names.contains(&"read"));
        assert!(names.contains(&"write"));
        assert!(names.contains(&"edit"));
        assert!(names.contains(&"glob"));
        assert!(names.contains(&"json_query"));
        assert!(names.contains(&"csv_parse"));
        assert!(names.contains(&"file_info"));
        assert!(names.contains(&"bash"));
        // Should have many tools
        assert!(names.len() > 40);
    }

    #[test]
    fn registry_register_overwrites_same_name() {
        let mut registry = ToolRegistry::new();
        registry.register(MockTool::new("dup").with_description("first"));
        registry.register(MockTool::new("dup").with_description("second"));
        let defs = registry.definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].description, "second");
    }
}
