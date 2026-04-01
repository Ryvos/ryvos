use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationToken {
    pub app_id: String,
    pub provider: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_expiry: Option<String>,
    pub scopes: String,
    pub connected_at: String,
}

pub struct IntegrationStore {
    conn: Mutex<Connection>,
}

impl IntegrationStore {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS integrations (
                 app_id TEXT PRIMARY KEY,
                 provider TEXT NOT NULL,
                 access_token TEXT NOT NULL,
                 refresh_token TEXT,
                 token_expiry TEXT,
                 scopes TEXT NOT NULL DEFAULT '',
                 connected_at TEXT NOT NULL,
                 metadata TEXT NOT NULL DEFAULT '{}'
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub async fn save_token(&self, token: &IntegrationToken) -> Result<(), String> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR REPLACE INTO integrations (app_id, provider, access_token, refresh_token, token_expiry, scopes, connected_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                token.app_id,
                token.provider,
                token.access_token,
                token.refresh_token,
                token.token_expiry,
                token.scopes,
                token.connected_at,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn get_token(&self, app_id: &str) -> Result<Option<IntegrationToken>, String> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT app_id, provider, access_token, refresh_token, token_expiry, scopes, connected_at FROM integrations WHERE app_id = ?1",
            )
            .map_err(|e| e.to_string())?;
        let result = stmt.query_row(rusqlite::params![app_id], |row| {
            Ok(IntegrationToken {
                app_id: row.get(0)?,
                provider: row.get(1)?,
                access_token: row.get(2)?,
                refresh_token: row.get(3)?,
                token_expiry: row.get(4)?,
                scopes: row.get(5)?,
                connected_at: row.get(6)?,
            })
        });
        match result {
            Ok(token) => Ok(Some(token)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub async fn list_connected(&self) -> Result<Vec<IntegrationToken>, String> {
        let conn = self.conn.lock().await;
        let mut stmt = conn
            .prepare(
                "SELECT app_id, provider, access_token, refresh_token, token_expiry, scopes, connected_at FROM integrations ORDER BY connected_at DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(IntegrationToken {
                    app_id: row.get(0)?,
                    provider: row.get(1)?,
                    access_token: row.get(2)?,
                    refresh_token: row.get(3)?,
                    token_expiry: row.get(4)?,
                    scopes: row.get(5)?,
                    connected_at: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub async fn delete(&self, app_id: &str) -> Result<bool, String> {
        let conn = self.conn.lock().await;
        let affected = conn
            .execute(
                "DELETE FROM integrations WHERE app_id = ?1",
                rusqlite::params![app_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(affected > 0)
    }

    pub async fn is_connected(&self, app_id: &str) -> bool {
        self.get_token(app_id)
            .await
            .map(|t| t.is_some())
            .unwrap_or(false)
    }
}
