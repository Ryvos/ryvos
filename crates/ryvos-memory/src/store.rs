use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::SessionStore;
use ryvos_core::types::{ChatMessage, SearchResult, SessionId};

use crate::embeddings::cosine_similarity;

/// SQLite-backed session store with FTS5 full-text search.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open or create a SQLite database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        // Create parent directories
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                RyvosError::Database(format!("Failed to create db directory: {}", e))
            })?;
        }

        let conn = Connection::open(path)
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        // Enable WAL mode for better concurrent performance
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        // Create tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session
                ON messages(session_id, id);

            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                content,
                session_id UNINDEXED,
                role UNINDEXED,
                timestamp UNINDEXED,
                content_rowid=id,
                tokenize='porter unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(rowid, content, session_id, role, timestamp)
                VALUES (new.id, new.content, new.session_id, new.role, new.timestamp);
            END;

            CREATE TABLE IF NOT EXISTS embeddings (
                id INTEGER PRIMARY KEY,
                message_id INTEGER REFERENCES messages(id),
                embedding BLOB NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_embeddings_msg ON embeddings(message_id);",
        )
        .map_err(|e| RyvosError::Database(e.to_string()))?;

        debug!(path = %path.display(), "SQLite store opened");
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory database (for testing).
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        conn.execute_batch(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );

            CREATE INDEX idx_messages_session ON messages(session_id, id);

            CREATE VIRTUAL TABLE messages_fts USING fts5(
                content,
                session_id UNINDEXED,
                role UNINDEXED,
                timestamp UNINDEXED,
                content_rowid=id,
                tokenize='porter unicode61'
            );

            CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
                INSERT INTO messages_fts(rowid, content, session_id, role, timestamp)
                VALUES (new.id, new.content, new.session_id, new.role, new.timestamp);
            END;

            CREATE TABLE IF NOT EXISTS embeddings (
                id INTEGER PRIMARY KEY,
                message_id INTEGER REFERENCES messages(id),
                embedding BLOB NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_embeddings_msg ON embeddings(message_id);",
        )
        .map_err(|e| RyvosError::Database(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl SqliteStore {
    /// Store an embedding vector for a message.
    pub fn store_embedding(&self, message_id: i64, embedding: &[f32]) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| RyvosError::Database(e.to_string()))?;
        let blob: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        conn.execute(
            "INSERT INTO embeddings (message_id, embedding) VALUES (?1, ?2)",
            params![message_id, blob],
        )
        .map_err(|e| RyvosError::Database(e.to_string()))?;
        Ok(())
    }

    /// Search for messages similar to a query vector using cosine similarity.
    /// Returns (session_id, role, content, timestamp, similarity) sorted by similarity descending.
    pub fn search_similar(&self, query_vec: &[f32], limit: usize) -> Result<Vec<SearchResult>> {
        let conn = self.conn.lock().map_err(|e| RyvosError::Database(e.to_string()))?;

        let mut stmt = conn
            .prepare(
                "SELECT e.embedding, m.session_id, m.role, m.content, m.timestamp
                 FROM embeddings e
                 JOIN messages m ON m.id = e.message_id",
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                let blob: Vec<u8> = row.get(0)?;
                let session_id: String = row.get(1)?;
                let role: String = row.get(2)?;
                let content: String = row.get(3)?;
                let ts_str: String = row.get(4)?;
                Ok((blob, session_id, role, content, ts_str))
            })
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        let mut scored: Vec<(f32, SearchResult)> = Vec::new();

        for row in rows {
            let (blob, session_id, role, content, ts_str) =
                row.map_err(|e| RyvosError::Database(e.to_string()))?;

            let embedding: Vec<f32> = blob
                .chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect();

            let sim = cosine_similarity(query_vec, &embedding);

            let timestamp = chrono::DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            scored.push((
                sim,
                SearchResult {
                    session_id,
                    role,
                    content,
                    timestamp,
                    rank: sim as f64,
                },
            ));
        }

        // Sort by similarity descending
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        Ok(scored.into_iter().map(|(_, r)| r).collect())
    }
}

impl SessionStore for SqliteStore {
    fn append_messages(
        &self,
        sid: &SessionId,
        msgs: &[ChatMessage],
    ) -> BoxFuture<'_, Result<()>> {
        let sid = sid.0.clone();
        let msgs: Vec<_> = msgs
            .iter()
            .map(|m| {
                let role = match m.role {
                    ryvos_core::types::Role::System => "system",
                    ryvos_core::types::Role::User => "user",
                    ryvos_core::types::Role::Assistant => "assistant",
                    ryvos_core::types::Role::Tool => "tool",
                };
                let content = serde_json::to_string(&m.content).unwrap_or_default();
                let timestamp = m
                    .timestamp
                    .unwrap_or_else(Utc::now)
                    .to_rfc3339();
                (role.to_string(), content, timestamp)
            })
            .collect();

        Box::pin(async move {
            let conn = self
                .conn
                .lock()
                .map_err(|e| RyvosError::Database(e.to_string()))?;

            for (role, content, timestamp) in &msgs {
                conn.execute(
                    "INSERT INTO messages (session_id, role, content, timestamp) VALUES (?1, ?2, ?3, ?4)",
                    params![sid, role, content, timestamp],
                )
                .map_err(|e| RyvosError::Database(e.to_string()))?;
            }

            Ok(())
        })
    }

    fn load_history(
        &self,
        sid: &SessionId,
        limit: usize,
    ) -> BoxFuture<'_, Result<Vec<ChatMessage>>> {
        let sid = sid.0.clone();

        Box::pin(async move {
            let conn = self
                .conn
                .lock()
                .map_err(|e| RyvosError::Database(e.to_string()))?;

            let mut stmt = conn
                .prepare(
                    "SELECT role, content, timestamp FROM messages
                     WHERE session_id = ?1
                     ORDER BY id ASC
                     LIMIT ?2",
                )
                .map_err(|e| RyvosError::Database(e.to_string()))?;

            let rows = stmt
                .query_map(params![sid, limit as i64], |row| {
                    let role: String = row.get(0)?;
                    let content_str: String = row.get(1)?;
                    let ts_str: String = row.get(2)?;
                    Ok((role, content_str, ts_str))
                })
                .map_err(|e| RyvosError::Database(e.to_string()))?;

            let mut messages = Vec::new();
            for row in rows {
                let (role, content_str, ts_str) = row
                    .map_err(|e| RyvosError::Database(e.to_string()))?;

                let role = match role.as_str() {
                    "system" => ryvos_core::types::Role::System,
                    "user" => ryvos_core::types::Role::User,
                    "assistant" => ryvos_core::types::Role::Assistant,
                    "tool" => ryvos_core::types::Role::Tool,
                    _ => ryvos_core::types::Role::User,
                };

                let content = serde_json::from_str(&content_str).unwrap_or_default();
                let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc));

                messages.push(ChatMessage {
                    role,
                    content,
                    timestamp,
                    metadata: None,
                });
            }

            Ok(messages)
        })
    }

    fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> BoxFuture<'_, Result<Vec<SearchResult>>> {
        let query = query.to_string();

        Box::pin(async move {
            let conn = self
                .conn
                .lock()
                .map_err(|e| RyvosError::Database(e.to_string()))?;

            let mut stmt = conn
                .prepare(
                    "SELECT session_id, role, content, timestamp, rank
                     FROM messages_fts
                     WHERE messages_fts MATCH ?1
                     ORDER BY rank
                     LIMIT ?2",
                )
                .map_err(|e| RyvosError::Database(e.to_string()))?;

            let rows = stmt
                .query_map(params![query, limit as i64], |row| {
                    Ok(SearchResult {
                        session_id: row.get(0)?,
                        role: row.get(1)?,
                        content: row.get(2)?,
                        timestamp: {
                            let ts_str: String = row.get(3)?;
                            DateTime::parse_from_rfc3339(&ts_str)
                                .map(|dt| dt.with_timezone(&Utc))
                                .unwrap_or_else(|_| Utc::now())
                        },
                        rank: row.get(4)?,
                    })
                })
                .map_err(|e| RyvosError::Database(e.to_string()))?;

            let mut results = Vec::new();
            for row in rows {
                results.push(
                    row.map_err(|e| RyvosError::Database(e.to_string()))?,
                );
            }

            Ok(results)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryvos_core::types::ChatMessage;

    #[tokio::test]
    async fn test_store_and_retrieve() {
        let store = SqliteStore::in_memory().unwrap();
        let sid = SessionId::new();

        let msgs = vec![
            ChatMessage::user("Hello"),
            ChatMessage::assistant_text("Hi there!"),
        ];

        store.append_messages(&sid, &msgs).await.unwrap();
        let history = store.load_history(&sid, 100).await.unwrap();
        assert_eq!(history.len(), 2);
    }

    #[tokio::test]
    async fn test_fts_search() {
        let store = SqliteStore::in_memory().unwrap();
        let sid = SessionId::new();

        let msgs = vec![
            ChatMessage::user("How do I configure Rust logging?"),
            ChatMessage::assistant_text("Use the tracing crate for structured logging in Rust."),
        ];

        store.append_messages(&sid, &msgs).await.unwrap();

        let results = store.search("logging", 10).await.unwrap();
        assert!(!results.is_empty());
    }
}
