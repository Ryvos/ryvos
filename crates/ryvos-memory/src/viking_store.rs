use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection};
use tracing::debug;

use crate::viking::{ContextLevel, VikingDirEntry, VikingMeta, VikingResult};

/// SQLite-backed storage for Viking hierarchical memory entries.
pub struct VikingStore {
    conn: Mutex<Connection>,
}

impl VikingStore {
    /// Open or create the Viking store at the given path.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create Viking DB directory: {}", e))?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;

             CREATE TABLE IF NOT EXISTS viking_entries (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 user_id TEXT NOT NULL,
                 path TEXT NOT NULL,
                 content TEXT NOT NULL,
                 content_l1 TEXT,
                 content_l0 TEXT,
                 category TEXT,
                 tags TEXT NOT NULL DEFAULT '[]',
                 source_session TEXT,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL,
                 UNIQUE(user_id, path)
             );

             CREATE INDEX IF NOT EXISTS idx_viking_user_path ON viking_entries(user_id, path);
             CREATE INDEX IF NOT EXISTS idx_viking_path_prefix ON viking_entries(path);

             CREATE VIRTUAL TABLE IF NOT EXISTS viking_fts USING fts5(
                 content,
                 path,
                 user_id,
                 tokenize='porter unicode61'
             );
             -- FTS sync is manual in write/delete methods to handle upserts correctly",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Create an in-memory store for testing.
    pub fn in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS viking_entries (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 user_id TEXT NOT NULL,
                 path TEXT NOT NULL,
                 content TEXT NOT NULL,
                 content_l1 TEXT,
                 content_l0 TEXT,
                 category TEXT,
                 tags TEXT NOT NULL DEFAULT '[]',
                 source_session TEXT,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL,
                 UNIQUE(user_id, path)
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS viking_fts USING fts5(
                 content, path UNINDEXED, user_id UNINDEXED,
                 content_rowid=id, tokenize='porter unicode61'
             );
             -- FTS5 sync is done manually in write/delete methods to handle upserts correctly",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Generate L0 summary from content (first sentence, max 100 chars).
    fn generate_l0(content: &str) -> String {
        let trimmed = content.trim();
        // Find first sentence ending
        let end = trimmed
            .find(". ")
            .or_else(|| trimmed.find(".\n"))
            .map(|i| i + 1)
            .unwrap_or_else(|| trimmed.len().min(100));
        trimmed[..end.min(100)].to_string()
    }

    /// Generate L1 content from full content (first 500 chars or 3 paragraphs).
    fn generate_l1(content: &str) -> String {
        let trimmed = content.trim();
        // Take first 3 paragraphs
        let paragraphs: Vec<&str> = trimmed.split("\n\n").take(3).collect();
        let l1 = paragraphs.join("\n\n");
        if l1.len() <= 500 {
            l1
        } else {
            trimmed[..500].to_string()
        }
    }

    /// Write (upsert) a memory entry.
    pub fn write(
        &self,
        user_id: &str,
        path: &str,
        content: &str,
        meta: &VikingMeta,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let now = Utc::now().to_rfc3339();
        let l0 = Self::generate_l0(content);
        let l1 = Self::generate_l1(content);
        let tags_json = serde_json::to_string(&meta.tags).unwrap_or_else(|_| "[]".to_string());
        let created = meta.created_at.as_deref().unwrap_or(&now);
        let updated = meta.updated_at.as_deref().unwrap_or(&now);

        // For upsert: delete existing entry first (handles both data + FTS cleanly)
        // Then insert fresh. This avoids FTS trigger conflicts with ON CONFLICT.
        let _ = self.delete_inner(&conn, user_id, path);

        conn.execute(
            "INSERT INTO viking_entries (user_id, path, content, content_l1, content_l0, category, tags, source_session, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![user_id, path, content, l1, l0, meta.category, tags_json, meta.source_session, created, updated],
        )
        .map_err(|e| format!("Viking write failed: {}", e))?;

        // Sync FTS manually
        let new_id: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO viking_fts(rowid, content, path, user_id) VALUES (?1, ?2, ?3, ?4)",
            params![new_id, content, path, user_id],
        )
        .map_err(|e| format!("Viking FTS insert failed: {}", e))?;

        debug!(path = path, "Viking entry written");
        Ok(())
    }

    /// Read a memory entry at the given detail level.
    pub fn read(&self, user_id: &str, path: &str, level: ContextLevel) -> Result<VikingResult, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let content_col = match level {
            ContextLevel::L0 => "content_l0",
            ContextLevel::L1 => "content_l1",
            ContextLevel::L2 => "content",
        };
        let sql = format!(
            "SELECT path, COALESCE({}, content) as content FROM viking_entries WHERE user_id = ?1 AND path = ?2",
            content_col
        );
        conn.query_row(&sql, params![user_id, path], |row| {
            Ok(VikingResult {
                path: row.get(0)?,
                content: row.get(1)?,
                level,
                relevance_score: 1.0,
                trajectory: vec![],
            })
        })
        .map_err(|e| format!("Viking read failed: {}", e))
    }

    /// Search entries using FTS5, optionally filtered by directory prefix.
    pub fn search(
        &self,
        user_id: &str,
        query: &str,
        directory: Option<&str>,
        limit: usize,
    ) -> Result<Vec<VikingResult>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;

        // Sanitize query for FTS5 (escape double quotes, wrap terms)
        let fts_query = query
            .split_whitespace()
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ");

        if fts_query.is_empty() {
            return Ok(vec![]);
        }

        // Search FTS table, then look up L1 content from entries table
        let base_sql = if let Some(dir) = directory {
            format!(
                "SELECT f.path, f.content, bm25(viking_fts) as score
                 FROM viking_fts f
                 WHERE viking_fts MATCH ?1 AND f.user_id = ?2 AND f.path LIKE '{}%'
                 ORDER BY score
                 LIMIT ?3",
                dir.replace('\'', "''")
            )
        } else {
            "SELECT f.path, f.content, bm25(viking_fts) as score
             FROM viking_fts f
             WHERE viking_fts MATCH ?1 AND f.user_id = ?2
             ORDER BY score
             LIMIT ?3"
                .to_string()
        };

        let mut stmt = conn.prepare(&base_sql).map_err(|e| e.to_string())?;
        let results = stmt
            .query_map(params![fts_query, user_id, limit as i64], |row| {
                let score: f64 = row.get(2)?;
                let content: String = row.get(1)?;
                Ok(VikingResult {
                    path: row.get(0)?,
                    content: content.chars().take(500).collect(),
                    level: ContextLevel::L1,
                    relevance_score: (-score).max(0.0).min(1.0),
                    trajectory: vec!["fts5".to_string()],
                })
            })
            .map_err(|e| e.to_string())?;

        results.collect::<std::result::Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// List directory contents with L0 summaries.
    /// Groups entries by the next path segment after the given prefix.
    pub fn list_directory(&self, user_id: &str, path: &str) -> Result<Vec<VikingDirEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let prefix = if path.ends_with('/') { path.to_string() } else { format!("{}/", path) };

        let mut stmt = conn
            .prepare(
                "SELECT path, content_l0 FROM viking_entries WHERE user_id = ?1 AND path LIKE ?2 ORDER BY path",
            )
            .map_err(|e| e.to_string())?;

        let rows: Vec<(String, String)> = stmt
            .query_map(params![user_id, format!("{}%", prefix)], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1).unwrap_or_default()))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        // Group by next path segment
        let mut entries: std::collections::BTreeMap<String, (bool, usize, Option<String>)> =
            std::collections::BTreeMap::new();

        for (entry_path, l0) in &rows {
            let suffix = &entry_path[prefix.len()..];
            let next_segment = suffix.split('/').next().unwrap_or(suffix);
            let remaining = &suffix[next_segment.len()..];
            let is_dir = !remaining.is_empty();
            let full_child_path = format!("{}{}", prefix, next_segment);

            let entry = entries.entry(full_child_path).or_insert((false, 0, None));
            if is_dir {
                entry.0 = true; // has children = directory
            }
            entry.1 += 1;
            if entry.2.is_none() && !l0.is_empty() {
                entry.2 = Some(l0.clone());
            }
        }

        Ok(entries
            .into_iter()
            .map(|(path, (is_dir, count, summary))| VikingDirEntry {
                path,
                is_directory: is_dir,
                summary: if is_dir {
                    Some(format!("{} entries", count))
                } else {
                    summary
                },
                child_count: if is_dir { Some(count) } else { None },
            })
            .collect())
    }

    /// Delete an entry by path.
    /// Internal delete that works with a pre-locked connection.
    fn delete_inner(&self, conn: &Connection, user_id: &str, path: &str) -> bool {
        // Remove from FTS by finding the FTS rowid that matches this path
        if let Ok(fts_rowid) = conn.query_row(
            "SELECT rowid FROM viking_fts WHERE path = ?1 AND user_id = ?2 LIMIT 1",
            params![path, user_id],
            |row| row.get::<_, i64>(0),
        ) {
            if let Ok(fts_content) = conn.query_row(
                "SELECT content FROM viking_fts WHERE rowid = ?1",
                params![fts_rowid],
                |row| row.get::<_, String>(0),
            ) {
                let _ = conn.execute(
                    "INSERT INTO viking_fts(viking_fts, rowid, content, path, user_id) VALUES ('delete', ?1, ?2, ?3, ?4)",
                    params![fts_rowid, fts_content, path, user_id],
                );
            }
        }
        // Delete from entries table
        conn.execute(
            "DELETE FROM viking_entries WHERE user_id = ?1 AND path = ?2",
            params![user_id, path],
        )
        .map(|n| n > 0)
        .unwrap_or(false)
    }

    /// Delete an entry by path.
    pub fn delete(&self, user_id: &str, path: &str) -> Result<bool, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        Ok(self.delete_inner(&conn, user_id, path))
    }

    /// Iterate on a transcript — extract memories into categories using keyword heuristics.
    pub fn iterate(&self, user_id: &str, transcript: &str) -> Result<usize, String> {
        let paragraphs: Vec<&str> = transcript
            .split("\n\n")
            .filter(|p| p.trim().len() >= 20)
            .collect();

        let mut count = 0;
        let now = Utc::now();

        for (i, para) in paragraphs.iter().enumerate() {
            let lower = para.to_lowercase();
            let category = if lower.contains("prefer") || lower.contains("like") || lower.contains("don't like") || lower.contains("always use") {
                "user/preferences"
            } else if lower.contains("name is") || lower.contains("i am") || lower.contains("my ") || lower.contains("i'm ") {
                "user/profile"
            } else if lower.contains("ip ") || lower.contains("server") || lower.contains("password") || lower.contains("key") || lower.contains("path") {
                "user/entities"
            } else if lower.contains("learned") || lower.contains("pattern") || lower.contains("rule") || lower.contains("always ") {
                "agent/patterns"
            } else if lower.contains("error") || lower.contains("failed") || lower.contains("bug") || lower.contains("fix") {
                "agent/cases"
            } else {
                "agent/events"
            };

            let path = format!("viking://{}/{}-{}", category, now.format("%Y%m%d-%H%M%S"), i);
            let meta = VikingMeta {
                category: Some(category.to_string()),
                ..Default::default()
            };

            if self.write(user_id, &path, para, &meta).is_ok() {
                count += 1;
            }
        }

        debug!(count = count, "Viking iteration extracted memories");
        Ok(count)
    }

    /// Count total entries for a user.
    pub fn count(&self, user_id: &str) -> Result<u64, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM viking_entries WHERE user_id = ?1",
                params![user_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(count as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_and_read() {
        let store = VikingStore::in_memory().unwrap();
        let meta = VikingMeta::default();
        store.write("user1", "viking://user/profile/name", "My name is Alice and I work at Acme Corp.", &meta).unwrap();

        let result = store.read("user1", "viking://user/profile/name", ContextLevel::L2).unwrap();
        assert_eq!(result.content, "My name is Alice and I work at Acme Corp.");
        assert_eq!(result.path, "viking://user/profile/name");

        let l0 = store.read("user1", "viking://user/profile/name", ContextLevel::L0).unwrap();
        assert!(l0.content.len() <= 100);
    }

    #[test]
    fn test_search() {
        let store = VikingStore::in_memory().unwrap();
        let meta = VikingMeta::default();
        store.write("user1", "viking://user/entities/server", "The production server IP is 10.0.0.1", &meta).unwrap();
        store.write("user1", "viking://agent/events/deploy", "Deployed version 2.0 to production", &meta).unwrap();

        let results = store.search("user1", "production", None, 10).unwrap();
        assert!(results.len() >= 1);
        assert!(results.iter().any(|r| r.content.contains("production")));
    }

    #[test]
    fn test_search_with_directory() {
        let store = VikingStore::in_memory().unwrap();
        let meta = VikingMeta::default();
        store.write("user1", "viking://user/entities/a", "Apple fruit", &meta).unwrap();
        store.write("user1", "viking://agent/events/b", "Banana event with apple", &meta).unwrap();

        let results = store.search("user1", "apple", Some("viking://user/"), 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].path.starts_with("viking://user/"));
    }

    #[test]
    fn test_list_directory() {
        let store = VikingStore::in_memory().unwrap();
        let meta = VikingMeta::default();
        store.write("user1", "viking://user/profile/name", "Alice", &meta).unwrap();
        store.write("user1", "viking://user/profile/role", "Engineer", &meta).unwrap();
        store.write("user1", "viking://user/preferences/theme", "Dark mode", &meta).unwrap();

        let entries = store.list_directory("user1", "viking://user/").unwrap();
        assert_eq!(entries.len(), 2); // profile (dir) + preferences (dir)
        assert!(entries.iter().all(|e| e.is_directory));
    }

    #[test]
    fn test_delete() {
        let store = VikingStore::in_memory().unwrap();
        let meta = VikingMeta::default();
        store.write("user1", "viking://test/entry", "Test content", &meta).unwrap();
        assert!(store.read("user1", "viking://test/entry", ContextLevel::L2).is_ok());
        assert!(store.delete("user1", "viking://test/entry").unwrap());
        assert!(store.read("user1", "viking://test/entry", ContextLevel::L2).is_err());
    }

    #[test]
    fn test_iterate() {
        let store = VikingStore::in_memory().unwrap();
        let transcript = "My name is Bob and I'm a developer.\n\nI prefer using Rust over Python for systems work.\n\nThe server crashed with error code 500 yesterday.";
        let count = store.iterate("user1", transcript).unwrap();
        assert_eq!(count, 3);

        let total = store.count("user1").unwrap();
        assert_eq!(total, 3);
    }

    #[test]
    fn test_l0_generation() {
        assert_eq!(VikingStore::generate_l0("Hello world. This is a test."), "Hello world.");
        assert_eq!(VikingStore::generate_l0("Short"), "Short");
        let long = "A".repeat(200);
        assert_eq!(VikingStore::generate_l0(&long).len(), 100);
    }

    #[test]
    fn test_upsert() {
        let store = VikingStore::in_memory().unwrap();
        let meta = VikingMeta::default();
        store.write("user1", "viking://test/x", "Version 1", &meta).unwrap();
        store.write("user1", "viking://test/x", "Version 2", &meta).unwrap();

        let result = store.read("user1", "viking://test/x", ContextLevel::L2).unwrap();
        assert_eq!(result.content, "Version 2");

        assert_eq!(store.count("user1").unwrap(), 1); // No duplicate
    }
}
