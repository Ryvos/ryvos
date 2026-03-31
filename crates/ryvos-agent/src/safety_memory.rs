use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Severity levels for safety incidents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// What happened after a tool action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SafetyOutcome {
    /// Action was fine.
    Harmless,
    /// Agent caught a potential issue before damage occurred.
    NearMiss { what_could_have_happened: String },
    /// Something actually went wrong.
    Incident {
        what_happened: String,
        severity: Severity,
    },
    /// User explicitly corrected the agent.
    UserCorrected { feedback: String },
}

/// A lesson learned from a past action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyLesson {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub outcome: SafetyOutcome,
    pub reflection: String,
    pub principle_violated: Option<String>,
    pub corrective_rule: String,
    pub confidence: f64,
    pub times_applied: u32,
}

/// SQLite-backed safety memory store.
pub struct SafetyMemory {
    conn: Mutex<Connection>,
}

impl SafetyMemory {
    /// Open or create the safety memory database.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS safety_lessons (
                 id TEXT PRIMARY KEY,
                 timestamp TEXT NOT NULL,
                 action TEXT NOT NULL,
                 outcome TEXT NOT NULL,
                 reflection TEXT NOT NULL,
                 principle_violated TEXT,
                 corrective_rule TEXT NOT NULL,
                 confidence REAL NOT NULL DEFAULT 0.8,
                 times_applied INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_lessons_action ON safety_lessons(action);
             CREATE INDEX IF NOT EXISTS idx_lessons_confidence ON safety_lessons(confidence DESC);",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Create an in-memory store for testing.
    pub fn in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS safety_lessons (
                 id TEXT PRIMARY KEY,
                 timestamp TEXT NOT NULL,
                 action TEXT NOT NULL,
                 outcome TEXT NOT NULL,
                 reflection TEXT NOT NULL,
                 principle_violated TEXT,
                 corrective_rule TEXT NOT NULL,
                 confidence REAL NOT NULL DEFAULT 0.8,
                 times_applied INTEGER NOT NULL DEFAULT 0
             );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Record a new safety lesson.
    pub async fn record_lesson(&self, lesson: &SafetyLesson) -> Result<(), String> {
        let conn = self.conn.lock().await;
        let outcome_json = serde_json::to_string(&lesson.outcome).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR REPLACE INTO safety_lessons (id, timestamp, action, outcome, reflection, principle_violated, corrective_rule, confidence, times_applied)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                lesson.id,
                lesson.timestamp.to_rfc3339(),
                lesson.action,
                outcome_json,
                lesson.reflection,
                lesson.principle_violated,
                lesson.corrective_rule,
                lesson.confidence,
                lesson.times_applied,
            ],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Retrieve lessons relevant to a given tool name or action pattern.
    /// Returns up to `limit` lessons ordered by confidence (highest first).
    pub async fn relevant_lessons(
        &self,
        tool_name: &str,
        limit: usize,
    ) -> Result<Vec<SafetyLesson>, String> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, action, outcome, reflection, principle_violated, corrective_rule, confidence, times_applied
             FROM safety_lessons
             WHERE action LIKE '%' || ?1 || '%'
             ORDER BY confidence DESC, times_applied DESC
             LIMIT ?2"
        ).map_err(|e| e.to_string())?;

        let lessons = stmt
            .query_map(rusqlite::params![tool_name, limit as i64], |row| {
                let outcome_str: String = row.get(3)?;
                let outcome: SafetyOutcome =
                    serde_json::from_str(&outcome_str).unwrap_or(SafetyOutcome::Harmless);
                let ts_str: String = row.get(1)?;
                let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                Ok(SafetyLesson {
                    id: row.get(0)?,
                    timestamp,
                    action: row.get(2)?,
                    outcome,
                    reflection: row.get(4)?,
                    principle_violated: row.get(5)?,
                    corrective_rule: row.get(6)?,
                    confidence: row.get(7)?,
                    times_applied: row.get(8)?,
                })
            })
            .map_err(|e| e.to_string())?;

        lessons
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Increment the times_applied counter for a lesson (when it prevents a repeat).
    pub async fn reinforce(&self, lesson_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE safety_lessons SET times_applied = times_applied + 1 WHERE id = ?1",
            rusqlite::params![lesson_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Get all high-confidence lessons (for context injection).
    pub async fn high_confidence_lessons(
        &self,
        min_confidence: f64,
        limit: usize,
    ) -> Result<Vec<SafetyLesson>, String> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, action, outcome, reflection, principle_violated, corrective_rule, confidence, times_applied
             FROM safety_lessons
             WHERE confidence >= ?1
             ORDER BY times_applied DESC, confidence DESC
             LIMIT ?2"
        ).map_err(|e| e.to_string())?;

        let lessons = stmt
            .query_map(rusqlite::params![min_confidence, limit as i64], |row| {
                let outcome_str: String = row.get(3)?;
                let outcome: SafetyOutcome =
                    serde_json::from_str(&outcome_str).unwrap_or(SafetyOutcome::Harmless);
                let ts_str: String = row.get(1)?;
                let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                Ok(SafetyLesson {
                    id: row.get(0)?,
                    timestamp,
                    action: row.get(2)?,
                    outcome,
                    reflection: row.get(4)?,
                    principle_violated: row.get(5)?,
                    corrective_rule: row.get(6)?,
                    confidence: row.get(7)?,
                    times_applied: row.get(8)?,
                })
            })
            .map_err(|e| e.to_string())?;

        lessons
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Prune low-confidence lessons that have never been applied.
    pub async fn prune_stale(
        &self,
        max_age_days: i64,
        min_confidence: f64,
    ) -> Result<usize, String> {
        let conn = self.conn.lock().await;
        let cutoff = (Utc::now() - chrono::Duration::days(max_age_days)).to_rfc3339();
        let affected = conn.execute(
            "DELETE FROM safety_lessons WHERE confidence < ?1 AND times_applied = 0 AND timestamp < ?2",
            rusqlite::params![min_confidence, cutoff],
        ).map_err(|e| e.to_string())?;
        Ok(affected)
    }

    /// Format lessons as context string for injection into system prompt.
    pub async fn format_for_context(&self, tool_names: &[String], limit: usize) -> String {
        let mut all_lessons = Vec::new();
        for name in tool_names {
            if let Ok(lessons) = self.relevant_lessons(name, limit).await {
                all_lessons.extend(lessons);
            }
        }
        // Also include high-confidence global lessons
        if let Ok(global) = self.high_confidence_lessons(0.9, 3).await {
            for lesson in global {
                if !all_lessons.iter().any(|l| l.id == lesson.id) {
                    all_lessons.push(lesson);
                }
            }
        }

        if all_lessons.is_empty() {
            return String::new();
        }

        // Sort by confidence desc, take top `limit`
        all_lessons.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all_lessons.truncate(limit);

        let mut output = String::from("# Lessons from Past Experience\n");
        for lesson in &all_lessons {
            let date = lesson.timestamp.format("%Y-%m-%d");
            output.push_str(&format!(
                "- [{}] {}: {}\n",
                date, lesson.action, lesson.corrective_rule
            ));
        }
        output
    }
}

/// Assess the outcome of a tool execution based on heuristics.
pub fn assess_outcome(
    tool_name: &str,
    _input: &serde_json::Value,
    result: &str,
    is_error: bool,
) -> SafetyOutcome {
    if !is_error {
        return SafetyOutcome::Harmless;
    }

    let lower = result.to_lowercase();

    // Check for signs of damage
    if lower.contains("permission denied") || lower.contains("operation not permitted") {
        return SafetyOutcome::Incident {
            what_happened: format!("Permission denied while executing {}", tool_name),
            severity: Severity::Medium,
        };
    }
    if lower.contains("no such file or directory")
        && (tool_name == "file_delete" || tool_name.contains("delete"))
    {
        return SafetyOutcome::Incident {
            what_happened: format!("File not found after delete operation via {}", tool_name),
            severity: Severity::Low,
        };
    }
    if lower.contains("cannot remove") || lower.contains("failed to remove") {
        return SafetyOutcome::Incident {
            what_happened: format!("Removal failed via {}", tool_name),
            severity: Severity::Medium,
        };
    }
    if lower.contains("data loss") || lower.contains("corrupted") {
        return SafetyOutcome::Incident {
            what_happened: format!("Potential data issue via {}", tool_name),
            severity: Severity::High,
        };
    }

    // Generic error — not necessarily a safety incident
    SafetyOutcome::Harmless
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_and_retrieve() {
        let mem = SafetyMemory::in_memory().unwrap();
        let lesson = SafetyLesson {
            id: "test-1".to_string(),
            timestamp: Utc::now(),
            action: "bash(rm -rf ./build)".to_string(),
            outcome: SafetyOutcome::NearMiss {
                what_could_have_happened: "Could have deleted source files".to_string(),
            },
            reflection: "Should use cargo clean instead".to_string(),
            principle_violated: Some("Proportionality".to_string()),
            corrective_rule: "Use cargo clean instead of rm -rf for build artifacts".to_string(),
            confidence: 0.9,
            times_applied: 0,
        };
        mem.record_lesson(&lesson).await.unwrap();

        let results = mem.relevant_lessons("bash", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].corrective_rule,
            "Use cargo clean instead of rm -rf for build artifacts"
        );
    }

    #[tokio::test]
    async fn test_reinforce() {
        let mem = SafetyMemory::in_memory().unwrap();
        let lesson = SafetyLesson {
            id: "test-2".to_string(),
            timestamp: Utc::now(),
            action: "file_delete".to_string(),
            outcome: SafetyOutcome::UserCorrected {
                feedback: "Don't delete config files".to_string(),
            },
            reflection: "Config files are user-created".to_string(),
            principle_violated: Some("Preservation".to_string()),
            corrective_rule: "Always read a file before deleting to check if it's user-created"
                .to_string(),
            confidence: 1.0,
            times_applied: 0,
        };
        mem.record_lesson(&lesson).await.unwrap();
        mem.reinforce("test-2").await.unwrap();

        let results = mem.relevant_lessons("file_delete", 10).await.unwrap();
        assert_eq!(results[0].times_applied, 1);
    }

    #[tokio::test]
    async fn test_format_for_context() {
        let mem = SafetyMemory::in_memory().unwrap();
        let lesson = SafetyLesson {
            id: "test-3".to_string(),
            timestamp: Utc::now(),
            action: "bash".to_string(),
            outcome: SafetyOutcome::Incident {
                what_happened: "Deleted wrong file".to_string(),
                severity: Severity::Medium,
            },
            reflection: "Should have checked first".to_string(),
            principle_violated: None,
            corrective_rule: "Read files before deleting".to_string(),
            confidence: 0.95,
            times_applied: 2,
        };
        mem.record_lesson(&lesson).await.unwrap();

        let ctx = mem.format_for_context(&["bash".to_string()], 5).await;
        assert!(ctx.contains("Lessons from Past Experience"));
        assert!(ctx.contains("Read files before deleting"));
    }

    #[test]
    fn test_assess_outcome_harmless() {
        let result = assess_outcome("bash", &serde_json::json!({}), "hello world", false);
        assert!(matches!(result, SafetyOutcome::Harmless));
    }

    #[test]
    fn test_assess_outcome_permission_denied() {
        let result = assess_outcome("bash", &serde_json::json!({}), "permission denied", true);
        assert!(matches!(
            result,
            SafetyOutcome::Incident {
                severity: Severity::Medium,
                ..
            }
        ));
    }
}
