use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use ryvos_core::types::{Decision, DecisionOutcome};

/// A record of a tool failure for pattern analysis.
#[derive(Debug, Clone)]
pub struct FailureRecord {
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub tool_name: String,
    pub error: String,
    pub input_summary: String,
    pub turn: usize,
}

/// Persistent journal of tool failures for self-healing pattern detection.
pub struct FailureJournal {
    conn: Mutex<Connection>,
}

impl FailureJournal {
    /// Open or create the failure journal database.
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create journal directory: {}", e))?;
        }

        let conn = Connection::open(path)
            .map_err(|e| format!("Failed to open failure journal: {}", e))?;

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;

             CREATE TABLE IF NOT EXISTS failure_journal (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 timestamp TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 tool_name TEXT NOT NULL,
                 error TEXT NOT NULL,
                 input_summary TEXT NOT NULL,
                 turn INTEGER NOT NULL
             );

             CREATE INDEX IF NOT EXISTS idx_fj_tool
                 ON failure_journal(tool_name, timestamp);

             CREATE TABLE IF NOT EXISTS success_journal (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 timestamp TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 tool_name TEXT NOT NULL
             );

             CREATE INDEX IF NOT EXISTS idx_sj_tool
                 ON success_journal(tool_name, timestamp);

             CREATE TABLE IF NOT EXISTS decisions (
                 id TEXT PRIMARY KEY,
                 timestamp TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 turn INTEGER NOT NULL,
                 description TEXT NOT NULL,
                 chosen_option TEXT NOT NULL,
                 alternatives_json TEXT NOT NULL DEFAULT '[]',
                 outcome_json TEXT
             );

             CREATE INDEX IF NOT EXISTS idx_dec_session
                 ON decisions(session_id, timestamp);",
        )
        .map_err(|e| format!("Failed to initialize journal schema: {}", e))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Record a tool failure.
    pub fn record(&self, rec: FailureRecord) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO failure_journal (timestamp, session_id, tool_name, error, input_summary, turn)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                rec.timestamp.to_rfc3339(),
                rec.session_id,
                rec.tool_name,
                rec.error,
                rec.input_summary,
                rec.turn as i64,
            ],
        )
        .map_err(|e| format!("Failed to record failure: {}", e))?;
        Ok(())
    }

    /// Record a tool success (for health tracking).
    pub fn record_success(&self, session_id: &str, tool_name: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO success_journal (timestamp, session_id, tool_name) VALUES (?1, ?2, ?3)",
            params![Utc::now().to_rfc3339(), session_id, tool_name],
        )
        .map_err(|e| format!("Failed to record success: {}", e))?;
        Ok(())
    }

    /// Find past failure patterns for a specific tool.
    pub fn find_patterns(&self, tool: &str, limit: usize) -> Result<Vec<FailureRecord>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT timestamp, session_id, tool_name, error, input_summary, turn
                 FROM failure_journal
                 WHERE tool_name = ?1
                 ORDER BY timestamp DESC
                 LIMIT ?2",
            )
            .map_err(|e| format!("Failed to query patterns: {}", e))?;

        let rows = stmt
            .query_map(params![tool, limit as i64], |row| {
                let ts_str: String = row.get(0)?;
                Ok(FailureRecord {
                    timestamp: DateTime::parse_from_rfc3339(&ts_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    session_id: row.get(1)?,
                    tool_name: row.get(2)?,
                    error: row.get(3)?,
                    input_summary: row.get(4)?,
                    turn: row.get::<_, i64>(5)? as usize,
                })
            })
            .map_err(|e| format!("Failed to fetch patterns: {}", e))?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|e| e.to_string())?);
        }
        Ok(records)
    }

    /// Record a decision made during an agent run.
    pub fn record_decision(&self, decision: &Decision) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let alts_json = serde_json::to_string(&decision.alternatives).unwrap_or_default();
        let outcome_json = decision
            .outcome
            .as_ref()
            .and_then(|o| serde_json::to_string(o).ok());
        conn.execute(
            "INSERT INTO decisions (id, timestamp, session_id, turn, description, chosen_option, alternatives_json, outcome_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                decision.id,
                decision.timestamp.to_rfc3339(),
                decision.session_id,
                decision.turn as i64,
                decision.description,
                decision.chosen_option,
                alts_json,
                outcome_json,
            ],
        )
        .map_err(|e| format!("Failed to record decision: {}", e))?;
        Ok(())
    }

    /// Update a decision's outcome after execution.
    pub fn update_decision_outcome(
        &self,
        decision_id: &str,
        outcome: &DecisionOutcome,
    ) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let outcome_json = serde_json::to_string(outcome).unwrap_or_default();
        conn.execute(
            "UPDATE decisions SET outcome_json = ?1 WHERE id = ?2",
            params![outcome_json, decision_id],
        )
        .map_err(|e| format!("Failed to update decision outcome: {}", e))?;
        Ok(())
    }

    /// Load decisions for a session.
    pub fn load_decisions(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<Decision>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, timestamp, session_id, turn, description, chosen_option, alternatives_json, outcome_json
                 FROM decisions
                 WHERE session_id = ?1
                 ORDER BY timestamp DESC
                 LIMIT ?2",
            )
            .map_err(|e| format!("Failed to query decisions: {}", e))?;

        let rows = stmt
            .query_map(params![session_id, limit as i64], |row| {
                let ts_str: String = row.get(0)?;
                let alts_str: String = row.get(6)?;
                let outcome_str: Option<String> = row.get(7)?;

                Ok(Decision {
                    id: row.get(0)?,
                    timestamp: DateTime::parse_from_rfc3339(&ts_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    session_id: row.get(2)?,
                    turn: row.get::<_, i64>(3)? as usize,
                    description: row.get(4)?,
                    chosen_option: row.get(5)?,
                    alternatives: serde_json::from_str(&alts_str).unwrap_or_default(),
                    outcome: outcome_str.and_then(|s| serde_json::from_str(&s).ok()),
                })
            })
            .map_err(|e| format!("Failed to fetch decisions: {}", e))?;

        let mut decisions = Vec::new();
        for row in rows {
            decisions.push(row.map_err(|e| e.to_string())?);
        }
        Ok(decisions)
    }

    /// Get health statistics per tool since a given time.
    /// Returns a map of tool_name -> (successes, failures).
    pub fn tool_health(
        &self,
        since: DateTime<Utc>,
    ) -> Result<HashMap<String, (usize, usize)>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let since_str = since.to_rfc3339();
        let mut health: HashMap<String, (usize, usize)> = HashMap::new();

        // Count successes
        {
            let mut stmt = conn
                .prepare(
                    "SELECT tool_name, COUNT(*) FROM success_journal
                     WHERE timestamp >= ?1
                     GROUP BY tool_name",
                )
                .map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map(params![since_str], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
                })
                .map_err(|e| e.to_string())?;

            for row in rows {
                let (tool, count) = row.map_err(|e| e.to_string())?;
                health.entry(tool).or_insert((0, 0)).0 = count;
            }
        }

        // Count failures
        {
            let mut stmt = conn
                .prepare(
                    "SELECT tool_name, COUNT(*) FROM failure_journal
                     WHERE timestamp >= ?1
                     GROUP BY tool_name",
                )
                .map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map(params![since_str], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
                })
                .map_err(|e| e.to_string())?;

            for row in rows {
                let (tool, count) = row.map_err(|e| e.to_string())?;
                health.entry(tool).or_insert((0, 0)).1 = count;
            }
        }

        Ok(health)
    }
}

/// Generate a pattern-aware reflexion hint using past failure history.
pub fn reflexion_hint_with_history(
    tool_name: &str,
    failure_count: usize,
    past: &[FailureRecord],
) -> ryvos_core::types::ChatMessage {
    let mut text = format!(
        "The tool `{}` has failed {} times in a row.",
        tool_name, failure_count
    );

    if !past.is_empty() {
        text.push_str("\n\nIn past sessions, this tool failed with these patterns:");
        for (i, rec) in past.iter().take(3).enumerate() {
            text.push_str(&format!(
                "\n  {}. [{}] Error: {}",
                i + 1,
                rec.timestamp.format("%Y-%m-%d %H:%M"),
                truncate_str(&rec.error, 150),
            ));
        }
        text.push_str("\n\nBased on these patterns, try a different approach or use a different tool.");
    } else {
        text.push_str(" Try a different approach or use a different tool to accomplish the task.");
    }

    ryvos_core::types::ChatMessage {
        role: ryvos_core::types::Role::User,
        content: vec![ryvos_core::types::ContentBlock::Text { text }],
        timestamp: Some(Utc::now()),
        metadata: None,
    }
}

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_journal() -> FailureJournal {
        let dir = std::env::temp_dir().join(format!("ryvos_healing_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        FailureJournal::open(&dir.join("healing.db")).unwrap()
    }

    #[test]
    fn record_and_find_patterns() {
        let journal = temp_journal();

        journal
            .record(FailureRecord {
                timestamp: Utc::now(),
                session_id: "test-sess".into(),
                tool_name: "bash".into(),
                error: "command not found".into(),
                input_summary: "rm -rf /".into(),
                turn: 1,
            })
            .unwrap();

        let patterns = journal.find_patterns("bash", 10).unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].error, "command not found");
    }

    #[test]
    fn tool_health_counts() {
        let journal = temp_journal();
        let since = Utc::now() - chrono::Duration::hours(1);

        journal
            .record_success("sess1", "read")
            .unwrap();
        journal
            .record_success("sess1", "read")
            .unwrap();
        journal
            .record(FailureRecord {
                timestamp: Utc::now(),
                session_id: "sess1".into(),
                tool_name: "read".into(),
                error: "file not found".into(),
                input_summary: "read /nonexistent".into(),
                turn: 0,
            })
            .unwrap();

        let health = journal.tool_health(since).unwrap();
        let (successes, failures) = health.get("read").unwrap();
        assert_eq!(*successes, 2);
        assert_eq!(*failures, 1);
    }

    #[test]
    fn test_decision_record_roundtrip() {
        let journal = temp_journal();

        let decision = Decision {
            id: "dec-1".to_string(),
            timestamp: Utc::now(),
            session_id: "sess-1".to_string(),
            turn: 3,
            description: "Which tool to use for file read".to_string(),
            chosen_option: "read".to_string(),
            alternatives: vec![
                ryvos_core::types::DecisionOption {
                    name: "bash cat".to_string(),
                    confidence: Some(0.3),
                },
            ],
            outcome: None,
        };

        journal.record_decision(&decision).unwrap();

        // Update outcome
        let outcome = DecisionOutcome {
            tokens_used: 150,
            latency_ms: 42,
            succeeded: true,
        };
        journal.update_decision_outcome("dec-1", &outcome).unwrap();

        // Load back
        let decisions = journal.load_decisions("sess-1", 10).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].chosen_option, "read");
        assert!(decisions[0].outcome.is_some());
        assert!(decisions[0].outcome.as_ref().unwrap().succeeded);
    }

    #[test]
    fn hint_with_no_history() {
        let hint = super::reflexion_hint_with_history("bash", 3, &[]);
        let text = hint.text();
        assert!(text.contains("bash"));
        assert!(text.contains("3"));
        assert!(text.contains("different approach"));
    }

    #[test]
    fn hint_with_past_patterns() {
        let records = vec![FailureRecord {
            timestamp: Utc::now(),
            session_id: "s1".into(),
            tool_name: "bash".into(),
            error: "permission denied".into(),
            input_summary: "sudo rm".into(),
            turn: 2,
        }];
        let hint = super::reflexion_hint_with_history("bash", 3, &records);
        let text = hint.text();
        assert!(text.contains("past sessions"));
        assert!(text.contains("permission denied"));
    }
}
