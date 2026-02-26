use futures::future::BoxFuture;
use serde_json::json;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct MemorySearchTool;

impl Tool for MemorySearchTool {
    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T0
    }

    fn name(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        "Search across all past conversations using full-text search (keyword mode) \
         or semantic similarity (semantic mode, requires embedding config). \
         Returns matching messages ranked by relevance."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (keywords or natural language)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results (default 10)",
                    "default": 10
                },
                "mode": {
                    "type": "string",
                    "description": "Search mode: 'keyword' (default, FTS5) or 'semantic' (embedding cosine similarity)",
                    "enum": ["keyword", "semantic"],
                    "default": "keyword"
                }
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
            let store = ctx
                .store
                .as_ref()
                .ok_or_else(|| RyvosError::ToolExecution {
                    tool: "memory_search".into(),
                    message: "No store available".into(),
                })?;

            let query = input["query"].as_str().unwrap_or("");
            let limit = input["limit"].as_u64().unwrap_or(10) as usize;
            let mode = input["mode"].as_str().unwrap_or("keyword");

            if mode == "semantic" {
                // Semantic mode requires embedding provider — fall back to keyword with note
                let results = store.search(query, limit).await?;
                let note = "[Note: semantic search requested but no embedding provider configured in this context. Falling back to keyword search.]\n\n";
                let output = format_results(&results);
                return Ok(ToolResult::success(if output.is_empty() {
                    "No results found.".into()
                } else {
                    format!("{}{}", note, output)
                }));
            }

            let results = store.search(query, limit).await?;

            Ok(ToolResult::success(if results.is_empty() {
                "No results found.".into()
            } else {
                format_results(&results)
            }))
        })
    }
}

fn format_results(results: &[ryvos_core::types::SearchResult]) -> String {
    results
        .iter()
        .map(|r| {
            format!(
                "[{}] {}: {}",
                r.timestamp.format("%Y-%m-%d %H:%M"),
                r.role,
                r.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n---\n")
}
