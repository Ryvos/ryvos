use std::path::Path;
use tokio::sync::Mutex;

/// Lightweight read-only access to the Ryvos audit trail SQLite database.
/// Opens in SQLITE_OPEN_READ_ONLY mode — safe for concurrent reads while
/// the daemon writes (WAL mode).
pub struct AuditReader {
    conn: Mutex<rusqlite::Connection>,
}

#[derive(Debug, serde::Serialize)]
pub struct AuditEntryLite {
    pub timestamp: String,
    pub session_id: String,
    pub tool_name: String,
    pub input_summary: String,
    pub outcome: String,
}

impl AuditReader {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub async fn recent_entries(&self, limit: usize) -> Result<Vec<AuditEntryLite>, String> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT timestamp, session_id, tool_name, input_summary, outcome
                 FROM audit_log
                 ORDER BY timestamp DESC
                 LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;

        let entries = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok(AuditEntryLite {
                    timestamp: row.get(0)?,
                    session_id: row.get(1)?,
                    tool_name: row.get(2)?,
                    input_summary: row.get(3)?,
                    outcome: row.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;

        entries
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub async fn tool_counts(&self) -> Result<Vec<(String, u64)>, String> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT tool_name, COUNT(*) as cnt
                 FROM audit_log
                 GROUP BY tool_name
                 ORDER BY cnt DESC
                 LIMIT 20",
            )
            .map_err(|e| e.to_string())?;

        let counts = stmt
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((name, count as u64))
            })
            .map_err(|e| e.to_string())?;

        counts
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
}
