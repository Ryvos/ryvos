use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::safety_memory::SafetyOutcome;

/// A single entry in the audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub tool_name: String,
    pub input_summary: String,
    pub output_summary: String,
    pub safety_reasoning: Option<String>,
    pub outcome: SafetyOutcome,
    pub lessons_available: Vec<String>,
}

/// Persistent audit trail — logs all tool actions for post-hoc accountability.
pub struct AuditTrail {
    conn: Mutex<Connection>,
}

impl AuditTrail {
    /// Open or create the audit trail database.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS audit_log (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 timestamp TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 tool_name TEXT NOT NULL,
                 input_summary TEXT NOT NULL,
                 output_summary TEXT NOT NULL,
                 safety_reasoning TEXT,
                 outcome TEXT NOT NULL,
                 lessons_available TEXT NOT NULL DEFAULT '[]'
             );
             CREATE INDEX IF NOT EXISTS idx_audit_session ON audit_log(session_id);
             CREATE INDEX IF NOT EXISTS idx_audit_tool ON audit_log(tool_name);
             CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_log(timestamp DESC);",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Create an in-memory audit trail for testing.
    pub fn in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_log (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 timestamp TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 tool_name TEXT NOT NULL,
                 input_summary TEXT NOT NULL,
                 output_summary TEXT NOT NULL,
                 safety_reasoning TEXT,
                 outcome TEXT NOT NULL,
                 lessons_available TEXT NOT NULL DEFAULT '[]'
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Log a tool call to the audit trail.
    pub async fn log_tool_call(&self, entry: &AuditEntry) -> Result<(), String> {
        let conn = self.conn.lock().await;
        let outcome_json = serde_json::to_string(&entry.outcome).map_err(|e| e.to_string())?;
        let lessons_json =
            serde_json::to_string(&entry.lessons_available).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO audit_log (timestamp, session_id, tool_name, input_summary, output_summary, safety_reasoning, outcome, lessons_available)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                entry.timestamp.to_rfc3339(),
                entry.session_id,
                entry.tool_name,
                entry.input_summary,
                entry.output_summary,
                entry.safety_reasoning,
                outcome_json,
                lessons_json,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Query recent audit entries for a session.
    pub async fn recent_entries(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<AuditEntry>, String> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT timestamp, session_id, tool_name, input_summary, output_summary, safety_reasoning, outcome, lessons_available
             FROM audit_log
             WHERE session_id = ?1
             ORDER BY timestamp DESC
             LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;

        let entries = stmt
            .query_map(rusqlite::params![session_id, limit as i64], |row| {
                let ts_str: String = row.get(0)?;
                let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let outcome_str: String = row.get(6)?;
                let outcome: SafetyOutcome =
                    serde_json::from_str(&outcome_str).unwrap_or(SafetyOutcome::Harmless);
                let lessons_str: String = row.get(7)?;
                let lessons: Vec<String> =
                    serde_json::from_str(&lessons_str).unwrap_or_default();
                Ok(AuditEntry {
                    timestamp,
                    session_id: row.get(1)?,
                    tool_name: row.get(2)?,
                    input_summary: row.get(3)?,
                    output_summary: row.get(4)?,
                    safety_reasoning: row.get(5)?,
                    outcome,
                    lessons_available: lessons,
                })
            })
            .map_err(|e| e.to_string())?;

        entries
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Count total audit entries (for dashboard).
    pub async fn total_entries(&self) -> Result<u64, String> {
        let conn = self.conn.lock().await;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        Ok(count as u64)
    }

    /// Query entries by tool name (for analysis).
    pub async fn entries_by_tool(
        &self,
        tool_name: &str,
        limit: usize,
    ) -> Result<Vec<AuditEntry>, String> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT timestamp, session_id, tool_name, input_summary, output_summary, safety_reasoning, outcome, lessons_available
             FROM audit_log
             WHERE tool_name = ?1
             ORDER BY timestamp DESC
             LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;

        let entries = stmt
            .query_map(rusqlite::params![tool_name, limit as i64], |row| {
                let ts_str: String = row.get(0)?;
                let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let outcome_str: String = row.get(6)?;
                let outcome: SafetyOutcome =
                    serde_json::from_str(&outcome_str).unwrap_or(SafetyOutcome::Harmless);
                let lessons_str: String = row.get(7)?;
                let lessons: Vec<String> =
                    serde_json::from_str(&lessons_str).unwrap_or_default();
                Ok(AuditEntry {
                    timestamp,
                    session_id: row.get(1)?,
                    tool_name: row.get(2)?,
                    input_summary: row.get(3)?,
                    output_summary: row.get(4)?,
                    safety_reasoning: row.get(5)?,
                    outcome,
                    lessons_available: lessons,
                })
            })
            .map_err(|e| e.to_string())?;

        entries
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_log_and_query() {
        let trail = AuditTrail::in_memory().unwrap();
        let entry = AuditEntry {
            timestamp: Utc::now(),
            session_id: "sess-1".to_string(),
            tool_name: "bash".to_string(),
            input_summary: "echo hello".to_string(),
            output_summary: "hello".to_string(),
            safety_reasoning: Some("Safe: echo is read-only".to_string()),
            outcome: SafetyOutcome::Harmless,
            lessons_available: vec![],
        };
        trail.log_tool_call(&entry).await.unwrap();

        let results = trail.recent_entries("sess-1", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tool_name, "bash");

        let count = trail.total_entries().await.unwrap();
        assert_eq!(count, 1);
    }
}
