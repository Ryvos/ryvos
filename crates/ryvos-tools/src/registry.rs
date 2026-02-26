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

        registry
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
