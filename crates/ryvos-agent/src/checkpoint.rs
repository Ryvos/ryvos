use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use ryvos_core::types::ChatMessage;

/// A single checkpoint snapshot.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    /// Session being checkpointed.
    pub session_id: String,
    /// Unique run identifier (distinguishes retries within a session).
    pub run_id: String,
    /// Turn number at which the checkpoint was taken.
    pub turn: usize,
    /// Serialized conversation messages (JSON).
    pub messages_json: String,
    /// Total tokens consumed so far.
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    /// When the checkpoint was created.
    pub timestamp: DateTime<Utc>,
}

/// Persistent checkpoint store backed by SQLite.
pub struct CheckpointStore {
    conn: Mutex<Connection>,
}

impl CheckpointStore {
    /// Open or create the checkpoint database.
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create checkpoint directory: {}", e))?;
        }

        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open checkpoint store: {}", e))?;

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;

             CREATE TABLE IF NOT EXISTS checkpoints (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 session_id TEXT NOT NULL,
                 run_id TEXT NOT NULL,
                 turn INTEGER NOT NULL,
                 messages_json TEXT NOT NULL,
                 total_input_tokens INTEGER NOT NULL DEFAULT 0,
                 total_output_tokens INTEGER NOT NULL DEFAULT 0,
                 timestamp TEXT NOT NULL
             );

             CREATE INDEX IF NOT EXISTS idx_cp_session_run
                 ON checkpoints(session_id, run_id, turn DESC);",
        )
        .map_err(|e| format!("Failed to initialize checkpoint schema: {}", e))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Save a checkpoint (upserts by session_id + run_id).
    pub fn save(&self, cp: &Checkpoint) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        // Delete older checkpoints for this run (keep only the latest)
        conn.execute(
            "DELETE FROM checkpoints WHERE session_id = ?1 AND run_id = ?2",
            params![cp.session_id, cp.run_id],
        )
        .map_err(|e| format!("Failed to clean old checkpoints: {}", e))?;

        conn.execute(
            "INSERT INTO checkpoints (session_id, run_id, turn, messages_json, total_input_tokens, total_output_tokens, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                cp.session_id,
                cp.run_id,
                cp.turn as i64,
                cp.messages_json,
                cp.total_input_tokens as i64,
                cp.total_output_tokens as i64,
                cp.timestamp.to_rfc3339(),
            ],
        )
        .map_err(|e| format!("Failed to save checkpoint: {}", e))?;

        Ok(())
    }

    /// Load the latest checkpoint for a session (any run_id).
    pub fn load_latest(&self, session_id: &str) -> Result<Option<Checkpoint>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT session_id, run_id, turn, messages_json, total_input_tokens, total_output_tokens, timestamp
                 FROM checkpoints
                 WHERE session_id = ?1
                 ORDER BY timestamp DESC
                 LIMIT 1",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let result = stmt
            .query_row(params![session_id], |row| {
                let ts_str: String = row.get(6)?;
                Ok(Checkpoint {
                    session_id: row.get(0)?,
                    run_id: row.get(1)?,
                    turn: row.get::<_, i64>(2)? as usize,
                    messages_json: row.get(3)?,
                    total_input_tokens: row.get::<_, i64>(4)? as u64,
                    total_output_tokens: row.get::<_, i64>(5)? as u64,
                    timestamp: DateTime::parse_from_rfc3339(&ts_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            })
            .ok();

        Ok(result)
    }

    /// Delete all checkpoints for a session.
    pub fn delete(&self, session_id: &str) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let deleted = conn
            .execute(
                "DELETE FROM checkpoints WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(|e| format!("Failed to delete checkpoints: {}", e))?;
        Ok(deleted)
    }

    /// Delete a specific run's checkpoint.
    pub fn delete_run(&self, session_id: &str, run_id: &str) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let deleted = conn
            .execute(
                "DELETE FROM checkpoints WHERE session_id = ?1 AND run_id = ?2",
                params![session_id, run_id],
            )
            .map_err(|e| format!("Failed to delete checkpoint: {}", e))?;
        Ok(deleted)
    }

    /// Serialize messages to JSON for storage.
    pub fn serialize_messages(messages: &[ChatMessage]) -> Result<String, String> {
        serde_json::to_string(messages).map_err(|e| format!("Failed to serialize messages: {}", e))
    }

    /// Deserialize messages from stored JSON.
    pub fn deserialize_messages(json: &str) -> Result<Vec<ChatMessage>, String> {
        serde_json::from_str(json).map_err(|e| format!("Failed to deserialize messages: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryvos_core::types::ChatMessage;

    fn temp_store() -> CheckpointStore {
        let dir =
            std::env::temp_dir().join(format!("ryvos_checkpoint_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        CheckpointStore::open(&dir.join("checkpoints.db")).unwrap()
    }

    #[test]
    fn test_save_and_load() {
        let store = temp_store();

        let messages = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant_text("hi there"),
        ];
        let messages_json = CheckpointStore::serialize_messages(&messages).unwrap();

        let cp = Checkpoint {
            session_id: "sess-1".to_string(),
            run_id: "run-1".to_string(),
            turn: 3,
            messages_json: messages_json.clone(),
            total_input_tokens: 100,
            total_output_tokens: 50,
            timestamp: Utc::now(),
        };

        store.save(&cp).unwrap();

        let loaded = store.load_latest("sess-1").unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.session_id, "sess-1");
        assert_eq!(loaded.run_id, "run-1");
        assert_eq!(loaded.turn, 3);
        assert_eq!(loaded.total_input_tokens, 100);

        let restored = CheckpointStore::deserialize_messages(&loaded.messages_json).unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].text(), "hello");
    }

    #[test]
    fn test_save_overwrites_same_run() {
        let store = temp_store();

        let cp1 = Checkpoint {
            session_id: "sess-1".to_string(),
            run_id: "run-1".to_string(),
            turn: 1,
            messages_json: "[]".to_string(),
            total_input_tokens: 10,
            total_output_tokens: 5,
            timestamp: Utc::now(),
        };
        store.save(&cp1).unwrap();

        let cp2 = Checkpoint {
            session_id: "sess-1".to_string(),
            run_id: "run-1".to_string(),
            turn: 3,
            messages_json: "[{}]".to_string(),
            total_input_tokens: 30,
            total_output_tokens: 15,
            timestamp: Utc::now(),
        };
        store.save(&cp2).unwrap();

        let loaded = store.load_latest("sess-1").unwrap().unwrap();
        assert_eq!(loaded.turn, 3);
        assert_eq!(loaded.total_input_tokens, 30);
    }

    #[test]
    fn test_delete() {
        let store = temp_store();

        let cp = Checkpoint {
            session_id: "sess-del".to_string(),
            run_id: "run-1".to_string(),
            turn: 1,
            messages_json: "[]".to_string(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            timestamp: Utc::now(),
        };
        store.save(&cp).unwrap();

        assert!(store.load_latest("sess-del").unwrap().is_some());

        let deleted = store.delete("sess-del").unwrap();
        assert_eq!(deleted, 1);

        assert!(store.load_latest("sess-del").unwrap().is_none());
    }

    #[test]
    fn test_resume_from_checkpoint() {
        let store = temp_store();

        let original_messages = vec![
            ChatMessage::user("Write a hello world script"),
            ChatMessage::assistant_text("I'll create that for you."),
        ];
        let json = CheckpointStore::serialize_messages(&original_messages).unwrap();

        let cp = Checkpoint {
            session_id: "sess-resume".to_string(),
            run_id: "run-1".to_string(),
            turn: 2,
            messages_json: json,
            total_input_tokens: 500,
            total_output_tokens: 200,
            timestamp: Utc::now(),
        };
        store.save(&cp).unwrap();

        // Simulate resume: load checkpoint, deserialize, continue
        let loaded = store.load_latest("sess-resume").unwrap().unwrap();
        let messages = CheckpointStore::deserialize_messages(&loaded.messages_json).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(loaded.turn, 2);
        assert_eq!(loaded.total_input_tokens, 500);

        // After successful run, delete checkpoint
        store.delete_run("sess-resume", "run-1").unwrap();
        assert!(store.load_latest("sess-resume").unwrap().is_none());
    }

    #[test]
    fn test_load_nonexistent() {
        let store = temp_store();
        let loaded = store.load_latest("nonexistent").unwrap();
        assert!(loaded.is_none());
    }
}
