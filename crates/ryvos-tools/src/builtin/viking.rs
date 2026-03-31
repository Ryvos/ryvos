use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::json;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

// ── VikingSearchTool ──────────────────────────────────────────

pub struct VikingSearchTool;

#[derive(Deserialize)]
struct VikingSearchInput {
    query: String,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    10
}

impl Tool for VikingSearchTool {
    fn name(&self) -> &str {
        "viking_search"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Search Viking hierarchical memory with semantic + path-based retrieval. \
         Returns results ranked by relevance with retrieval trajectory."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Natural language search query" },
                "directory": { "type": "string", "description": "Optional: restrict search to a viking:// directory" },
                "limit": { "type": "integer", "description": "Max results (default 10)", "default": 10 }
            },
            "required": ["query"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: VikingSearchInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            let viking = ctx
                .viking_client
                .as_ref()
                .and_then(|c| c.downcast_ref::<std::sync::Arc<ryvos_memory::VikingClient>>())
                .ok_or_else(|| RyvosError::ToolExecution {
                    tool: "viking_search".into(),
                    message:
                        "OpenViking not configured. Enable it in config.toml under [openviking]."
                            .into(),
                })?;

            match viking
                .search(&params.query, params.directory.as_deref(), params.limit)
                .await
            {
                Ok(results) => {
                    if results.is_empty() {
                        Ok(ToolResult::success("No results found in Viking memory."))
                    } else {
                        let formatted: Vec<String> = results
                            .iter()
                            .map(|r| {
                                format!(
                                    "[score:{:.2}] {}\n{}",
                                    r.relevance_score, r.path, r.content
                                )
                            })
                            .collect();
                        Ok(ToolResult::success(formatted.join("\n---\n")))
                    }
                }
                Err(e) => Ok(ToolResult::error(format!("Viking search failed: {}", e))),
            }
        })
    }
}

// ── VikingReadTool ────────────────────────────────────────────

pub struct VikingReadTool;

#[derive(Deserialize)]
struct VikingReadInput {
    path: String,
    #[serde(default = "default_level")]
    level: String,
}

fn default_level() -> String {
    "L1".to_string()
}

impl Tool for VikingReadTool {
    fn name(&self) -> &str {
        "viking_read"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Read a viking:// memory path at L0 (summary), L1 (details), or L2 (full content)."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Viking path (e.g., viking://user/preferences/)" },
                "level": { "type": "string", "description": "Detail level: L0, L1, or L2", "enum": ["L0", "L1", "L2"], "default": "L1" }
            },
            "required": ["path"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: VikingReadInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            let viking = ctx
                .viking_client
                .as_ref()
                .and_then(|c| c.downcast_ref::<std::sync::Arc<ryvos_memory::VikingClient>>())
                .ok_or_else(|| RyvosError::ToolExecution {
                    tool: "viking_read".into(),
                    message: "OpenViking not configured.".into(),
                })?;

            let level = match params.level.as_str() {
                "L0" => ryvos_memory::viking::ContextLevel::L0,
                "L2" => ryvos_memory::viking::ContextLevel::L2,
                _ => ryvos_memory::viking::ContextLevel::L1,
            };

            match viking.read_memory(&params.path, level).await {
                Ok(result) => Ok(ToolResult::success(format!(
                    "Path: {}\nLevel: {}\nScore: {:.2}\n\n{}",
                    result.path, result.level, result.relevance_score, result.content
                ))),
                Err(e) => Ok(ToolResult::error(format!("Viking read failed: {}", e))),
            }
        })
    }
}

// ── VikingWriteTool ───────────────────────────────────────────

pub struct VikingWriteTool;

#[derive(Deserialize)]
struct VikingWriteInput {
    path: String,
    content: String,
    #[serde(default)]
    tags: Vec<String>,
}

impl Tool for VikingWriteTool {
    fn name(&self) -> &str {
        "viking_write"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T1
    }
    fn description(&self) -> &str {
        "Write or update a memory entry at a viking:// path. \
         Use for persisting long-term facts, preferences, and patterns."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Viking path (e.g., viking://user/entities/server-ips)" },
                "content": { "type": "string", "description": "Content to write" },
                "tags": { "type": "array", "items": { "type": "string" }, "description": "Optional tags" }
            },
            "required": ["path", "content"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: VikingWriteInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            let viking = ctx
                .viking_client
                .as_ref()
                .and_then(|c| c.downcast_ref::<std::sync::Arc<ryvos_memory::VikingClient>>())
                .ok_or_else(|| RyvosError::ToolExecution {
                    tool: "viking_write".into(),
                    message: "OpenViking not configured.".into(),
                })?;

            let meta = ryvos_memory::viking::VikingMeta {
                tags: params.tags,
                updated_at: Some(chrono::Utc::now().to_rfc3339()),
                ..Default::default()
            };

            match viking
                .write_memory(&params.path, &params.content, Some(meta))
                .await
            {
                Ok(()) => Ok(ToolResult::success(format!("Written to {}", params.path))),
                Err(e) => Ok(ToolResult::error(format!("Viking write failed: {}", e))),
            }
        })
    }
}

// ── VikingListTool ────────────────────────────────────────────

pub struct VikingListTool;

#[derive(Deserialize)]
struct VikingListInput {
    #[serde(default = "default_viking_root")]
    path: String,
}

fn default_viking_root() -> String {
    "viking://".to_string()
}

impl Tool for VikingListTool {
    fn name(&self) -> &str {
        "viking_list"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "List Viking memory directory contents with L0 summaries."
    }
    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Viking directory path (default: viking://)", "default": "viking://" }
            }
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: VikingListInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            let viking = ctx
                .viking_client
                .as_ref()
                .and_then(|c| c.downcast_ref::<std::sync::Arc<ryvos_memory::VikingClient>>())
                .ok_or_else(|| RyvosError::ToolExecution {
                    tool: "viking_list".into(),
                    message: "OpenViking not configured.".into(),
                })?;

            match viking.list_directory(&params.path).await {
                Ok(entries) => {
                    if entries.is_empty() {
                        Ok(ToolResult::success(format!(
                            "Empty directory: {}",
                            params.path
                        )))
                    } else {
                        let formatted: Vec<String> = entries
                            .iter()
                            .map(|e| {
                                let icon = if e.is_directory {
                                    "\u{1f4c1}"
                                } else {
                                    "\u{1f4c4}"
                                };
                                let summary = e.summary.as_deref().unwrap_or("");
                                format!("{} {} {}", icon, e.path, summary)
                            })
                            .collect();
                        Ok(ToolResult::success(formatted.join("\n")))
                    }
                }
                Err(e) => Ok(ToolResult::error(format!("Viking list failed: {}", e))),
            }
        })
    }
}
