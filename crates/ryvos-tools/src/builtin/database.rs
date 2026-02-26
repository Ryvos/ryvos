use std::path::PathBuf;

use futures::future::BoxFuture;
use serde::Deserialize;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

fn resolve(p: &str, wd: &std::path::Path) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        path
    } else {
        wd.join(path)
    }
}

// ── SqliteQueryTool ─────────────────────────────────────────────

pub struct SqliteQueryTool;

#[derive(Deserialize)]
struct SqliteQueryInput {
    database: String,
    query: String,
}

impl Tool for SqliteQueryTool {
    fn name(&self) -> &str {
        "sqlite_query"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn timeout_secs(&self) -> u64 {
        30
    }
    fn description(&self) -> &str {
        "Execute a SQL query on a SQLite database."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "database": { "type": "string", "description": "Path to SQLite database file" },
                "query": { "type": "string", "description": "SQL query to execute" }
            },
            "required": ["database", "query"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: SqliteQueryInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let db = resolve(&p.database, &ctx.working_dir);
            let output = tokio::process::Command::new("sqlite3")
                .args(["-header", "-json", &db.to_string_lossy(), &p.query])
                .output()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "sqlite_query".into(),
                    message: format!("sqlite3 not found: {}", e),
                })?;
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).to_string();
                if text.is_empty() {
                    Ok(ToolResult::success(
                        "Query executed successfully (no rows returned).".to_string(),
                    ))
                } else {
                    Ok(ToolResult::success(text))
                }
            } else {
                Ok(ToolResult::error(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ))
            }
        })
    }
}

// ── SqliteSchemaTool ────────────────────────────────────────────

pub struct SqliteSchemaTool;

#[derive(Deserialize)]
struct SqliteSchemaInput {
    database: String,
}

impl Tool for SqliteSchemaTool {
    fn name(&self) -> &str {
        "sqlite_schema"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Show the schema of a SQLite database (tables and columns)."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "database": { "type": "string", "description": "Path to SQLite database file" } },
            "required": ["database"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: SqliteSchemaInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let db = resolve(&p.database, &ctx.working_dir);
            let output = tokio::process::Command::new("sqlite3")
                .args([&db.to_string_lossy(), "SELECT type, name, sql FROM sqlite_master WHERE type IN ('table', 'view') ORDER BY type, name;"])
                .output()
                .await
                .map_err(|e| RyvosError::ToolExecution { tool: "sqlite_schema".into(), message: format!("sqlite3 not found: {}", e) })?;
            if output.status.success() {
                Ok(ToolResult::success(
                    String::from_utf8_lossy(&output.stdout).to_string(),
                ))
            } else {
                Ok(ToolResult::error(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ))
            }
        })
    }
}
