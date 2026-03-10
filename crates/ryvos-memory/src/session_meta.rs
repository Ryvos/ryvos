use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection};
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};

/// Persistent session metadata store (survives daemon restarts).
/// Thread-safe via Mutex wrapping the connection.
pub struct SessionMetaStore {
    conn: Mutex<Connection>,
}

/// A row from the session_meta table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionMeta {
    pub session_key: String,
    pub session_id: String,
    pub channel: String,
    pub cli_session_id: Option<String>,
    pub total_runs: u64,
    pub total_tokens: u64,
    pub billing_type: Option<String>,
    pub started_at: String,
    pub last_active: String,
}

impl SessionMetaStore {
    /// Open (or create) the session meta database.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| RyvosError::Database(e.to_string()))?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS session_meta (
                session_key TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                channel TEXT NOT NULL,
                cli_session_id TEXT,
                total_runs INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                billing_type TEXT,
                started_at TEXT NOT NULL,
                last_active TEXT NOT NULL
            );
            ",
        )
        .map_err(|e| RyvosError::Database(e.to_string()))?;

        debug!("SessionMetaStore opened at {}", path.display());
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Get or create session meta for a key.
    pub fn get_or_create(
        &self,
        session_key: &str,
        session_id: &str,
        channel: &str,
    ) -> Result<SessionMeta> {
        let conn = self.conn.lock().unwrap();
        let existing = Self::get_inner(&conn, session_key)?;
        if let Some(meta) = existing {
            // Update last_active
            conn.execute(
                    "UPDATE session_meta SET last_active = ?1 WHERE session_key = ?2",
                    params![Utc::now().to_rfc3339(), session_key],
                )
                .map_err(|e| RyvosError::Database(e.to_string()))?;
            return Ok(meta);
        }

        let now = Utc::now().to_rfc3339();
        conn.execute(
                "INSERT INTO session_meta (session_key, session_id, channel, started_at, last_active)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![session_key, session_id, channel, now, now],
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        Ok(SessionMeta {
            session_key: session_key.to_string(),
            session_id: session_id.to_string(),
            channel: channel.to_string(),
            cli_session_id: None,
            total_runs: 0,
            total_tokens: 0,
            billing_type: None,
            started_at: now.clone(),
            last_active: now,
        })
    }

    /// Get session meta by key.
    pub fn get(&self, session_key: &str) -> Result<Option<SessionMeta>> {
        let conn = self.conn.lock().unwrap();
        Self::get_inner(&conn, session_key)
    }

    /// Inner get (takes a connection reference to avoid double-locking).
    fn get_inner(conn: &Connection, session_key: &str) -> Result<Option<SessionMeta>> {
        let result = conn.query_row(
            "SELECT session_key, session_id, channel, cli_session_id, total_runs,
                    total_tokens, billing_type, started_at, last_active
             FROM session_meta WHERE session_key = ?1",
            params![session_key],
            |row| {
                Ok(SessionMeta {
                    session_key: row.get(0)?,
                    session_id: row.get(1)?,
                    channel: row.get(2)?,
                    cli_session_id: row.get(3)?,
                    total_runs: row.get::<_, i64>(4)? as u64,
                    total_tokens: row.get::<_, i64>(5)? as u64,
                    billing_type: row.get(6)?,
                    started_at: row.get(7)?,
                    last_active: row.get(8)?,
                })
            },
        );

        match result {
            Ok(meta) => Ok(Some(meta)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(RyvosError::Database(e.to_string())),
        }
    }

    /// Set the CLI session ID (for --resume).
    pub fn set_cli_session_id(
        &self,
        session_key: &str,
        cli_session_id: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
                "UPDATE session_meta SET cli_session_id = ?1, last_active = ?2 WHERE session_key = ?3",
                params![cli_session_id, Utc::now().to_rfc3339(), session_key],
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;
        Ok(())
    }

    /// Clear the CLI session ID (on resume failure).
    pub fn clear_cli_session_id(&self, session_key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
                "UPDATE session_meta SET cli_session_id = NULL WHERE session_key = ?1",
                params![session_key],
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;
        Ok(())
    }

    /// Record run completion stats.
    pub fn record_run_stats(
        &self,
        session_key: &str,
        tokens: u64,
        billing_type: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
                "UPDATE session_meta SET total_runs = total_runs + 1, total_tokens = total_tokens + ?1,
                 billing_type = ?2, last_active = ?3 WHERE session_key = ?4",
                params![tokens, billing_type, Utc::now().to_rfc3339(), session_key],
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;
        Ok(())
    }

    /// List all sessions.
    pub fn list(&self) -> Result<Vec<SessionMeta>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT session_key, session_id, channel, cli_session_id, total_runs,
                        total_tokens, billing_type, started_at, last_active
                 FROM session_meta ORDER BY last_active DESC",
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(SessionMeta {
                    session_key: row.get(0)?,
                    session_id: row.get(1)?,
                    channel: row.get(2)?,
                    cli_session_id: row.get(3)?,
                    total_runs: row.get::<_, i64>(4)? as u64,
                    total_tokens: row.get::<_, i64>(5)? as u64,
                    billing_type: row.get(6)?,
                    started_at: row.get(7)?,
                    last_active: row.get(8)?,
                })
            })
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| RyvosError::Database(e.to_string()))?);
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> (SessionMetaStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("session_meta.db");
        let store = SessionMetaStore::open(&db_path).unwrap();
        (store, dir)
    }

    #[test]
    fn create_and_get() {
        let (store, _dir) = test_store();
        let meta = store
            .get_or_create("tg:user:123", "session-abc", "telegram")
            .unwrap();
        assert_eq!(meta.session_id, "session-abc");
        assert_eq!(meta.channel, "telegram");
        assert!(meta.cli_session_id.is_none());
    }

    #[test]
    fn cli_session_id_lifecycle() {
        let (store, _dir) = test_store();
        store
            .get_or_create("tg:user:123", "s1", "telegram")
            .unwrap();

        store.set_cli_session_id("tg:user:123", "cli-xyz").unwrap();
        let meta = store.get("tg:user:123").unwrap().unwrap();
        assert_eq!(meta.cli_session_id.as_deref(), Some("cli-xyz"));

        store.clear_cli_session_id("tg:user:123").unwrap();
        let meta = store.get("tg:user:123").unwrap().unwrap();
        assert!(meta.cli_session_id.is_none());
    }

    #[test]
    fn record_stats() {
        let (store, _dir) = test_store();
        store
            .get_or_create("tg:user:123", "s1", "telegram")
            .unwrap();
        store
            .record_run_stats("tg:user:123", 5000, "api")
            .unwrap();
        store
            .record_run_stats("tg:user:123", 3000, "api")
            .unwrap();

        let meta = store.get("tg:user:123").unwrap().unwrap();
        assert_eq!(meta.total_runs, 2);
        assert_eq!(meta.total_tokens, 8000);
    }
}
