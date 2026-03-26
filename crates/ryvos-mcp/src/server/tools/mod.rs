pub mod audit;
pub mod memory;
pub mod viking;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Viking Tool Parameters ──

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct VikingSearchParams {
    /// Natural language search query
    pub query: String,
    /// Restrict search to a viking:// directory (optional)
    pub directory: Option<String>,
    /// Max results (default 10)
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct VikingReadParams {
    /// Viking path (e.g., viking://user/preferences)
    pub path: String,
    /// Detail level: L0, L1, or L2 (default L1)
    pub level: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct VikingWriteParams {
    /// Viking path (e.g., viking://user/entities/server-ips)
    pub path: String,
    /// Content to write
    pub content: String,
    /// Optional tags for categorization
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct VikingListParams {
    /// Viking directory path (default: viking://)
    pub path: Option<String>,
}

// ── Memory Tool Parameters ──

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MemoryGetParams {
    /// Memory file name (without .md extension). Omit for MEMORY.md.
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct MemoryWriteParams {
    /// The note to append to persistent memory
    pub note: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DailyLogWriteParams {
    /// Log entry text (timestamp added automatically)
    pub entry: String,
}

// ── Audit Tool Parameters ──

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AuditQueryParams {
    /// Number of recent entries to return (default 20)
    pub limit: Option<usize>,
}
