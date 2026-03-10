use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::types::{BillingType, CostEvent, CostSummary};

/// SQLite store for cost events and run logs.
/// Thread-safe via Mutex wrapping the connection.
pub struct CostStore {
    conn: Mutex<Connection>,
}

impl CostStore {
    /// Open (or create) the cost database.
    pub fn open(path: &Path) -> Result<Self> {
        let conn =
            Connection::open(path).map_err(|e| RyvosError::Database(e.to_string()))?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS cost_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0,
                cost_cents INTEGER DEFAULT 0,
                billing_type TEXT DEFAULT 'api',
                model TEXT NOT NULL,
                provider TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_cost_ts ON cost_events(timestamp);

            CREATE TABLE IF NOT EXISTS run_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL UNIQUE,
                session_id TEXT NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT,
                input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0,
                total_turns INTEGER DEFAULT 0,
                billing_type TEXT DEFAULT 'api',
                model TEXT NOT NULL,
                provider TEXT NOT NULL,
                cost_cents INTEGER DEFAULT 0,
                status TEXT DEFAULT 'running'
            );
            ",
        )
        .map_err(|e| RyvosError::Database(e.to_string()))?;

        debug!("CostStore opened at {}", path.display());
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Record a single cost event.
    pub fn record_cost_event(&self, event: &CostEvent) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
                "INSERT INTO cost_events (run_id, session_id, timestamp, input_tokens, output_tokens, cost_cents, billing_type, model, provider)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    event.run_id,
                    event.session_id,
                    event.timestamp.to_rfc3339(),
                    event.input_tokens,
                    event.output_tokens,
                    event.cost_cents,
                    event.billing_type.to_string(),
                    event.model,
                    event.provider,
                ],
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;
        Ok(())
    }

    /// Get total spend in cents for the current calendar month.
    pub fn monthly_spend_cents(&self) -> Result<u64> {
        let now = Utc::now();
        let month_start = format!("{}-{:02}-01T00:00:00+00:00", now.format("%Y"), now.format("%m"));

        let conn = self.conn.lock().unwrap();
        let cents: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(cost_cents), 0) FROM cost_events WHERE timestamp >= ?1",
                params![month_start],
                |row| row.get(0),
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;
        Ok(cents as u64)
    }

    /// Get cost summary for a date range.
    pub fn cost_summary(&self, from: &DateTime<Utc>, to: &DateTime<Utc>) -> Result<CostSummary> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT COALESCE(SUM(cost_cents), 0), COALESCE(SUM(input_tokens), 0),
                        COALESCE(SUM(output_tokens), 0), COUNT(*)
                 FROM cost_events WHERE timestamp >= ?1 AND timestamp <= ?2",
                params![from.to_rfc3339(), to.to_rfc3339()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as u64,
                        row.get::<_, i64>(1)? as u64,
                        row.get::<_, i64>(2)? as u64,
                        row.get::<_, i64>(3)? as u64,
                    ))
                },
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        Ok(CostSummary {
            total_cost_cents: row.0,
            total_input_tokens: row.1,
            total_output_tokens: row.2,
            total_events: row.3,
            breakdown: std::collections::HashMap::new(),
        })
    }

    /// Get cost breakdown grouped by a field (model, provider, or day).
    pub fn cost_by_group(
        &self,
        from: &DateTime<Utc>,
        to: &DateTime<Utc>,
        group_by: &str,
    ) -> Result<Vec<(String, u64, u64, u64)>> {
        let group_col = match group_by {
            "model" => "model",
            "provider" => "provider",
            "day" => "DATE(timestamp)",
            _ => "model",
        };

        let sql = format!(
            "SELECT {col}, COALESCE(SUM(cost_cents), 0), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
             FROM cost_events WHERE timestamp >= ?1 AND timestamp <= ?2
             GROUP BY {col} ORDER BY SUM(cost_cents) DESC",
            col = group_col
        );

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![from.to_rfc3339(), to.to_rfc3339()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, i64>(2)? as u64,
                    row.get::<_, i64>(3)? as u64,
                ))
            })
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| RyvosError::Database(e.to_string()))?);
        }
        Ok(results)
    }

    /// Record a run start.
    pub fn record_run(
        &self,
        run_id: &str,
        session_id: &str,
        model: &str,
        provider: &str,
        billing_type: BillingType,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
                "INSERT OR IGNORE INTO run_log (run_id, session_id, start_time, model, provider, billing_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    run_id,
                    session_id,
                    Utc::now().to_rfc3339(),
                    model,
                    provider,
                    billing_type.to_string(),
                ],
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;
        Ok(())
    }

    /// Update a run with final stats.
    pub fn complete_run(
        &self,
        run_id: &str,
        input_tokens: u64,
        output_tokens: u64,
        total_turns: u64,
        cost_cents: u64,
        status: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
                "UPDATE run_log SET end_time = ?1, input_tokens = ?2, output_tokens = ?3,
                 total_turns = ?4, cost_cents = ?5, status = ?6 WHERE run_id = ?7",
                params![
                    Utc::now().to_rfc3339(),
                    input_tokens,
                    output_tokens,
                    total_turns,
                    cost_cents,
                    status,
                    run_id,
                ],
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;
        Ok(())
    }

    /// Get paginated run history.
    pub fn run_history(
        &self,
        limit: u64,
        offset: u64,
    ) -> Result<(Vec<serde_json::Value>, u64)> {
        let conn = self.conn.lock().unwrap();
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM run_log", [], |row| row.get(0))
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        let mut stmt = conn
            .prepare(
                "SELECT run_id, session_id, start_time, end_time, input_tokens, output_tokens,
                        total_turns, billing_type, model, provider, cost_cents, status
                 FROM run_log ORDER BY start_time DESC LIMIT ?1 OFFSET ?2",
            )
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![limit, offset], |row| {
                Ok(serde_json::json!({
                    "run_id": row.get::<_, String>(0)?,
                    "session_id": row.get::<_, String>(1)?,
                    "start_time": row.get::<_, String>(2)?,
                    "end_time": row.get::<_, Option<String>>(3)?,
                    "input_tokens": row.get::<_, i64>(4)?,
                    "output_tokens": row.get::<_, i64>(5)?,
                    "total_turns": row.get::<_, i64>(6)?,
                    "billing_type": row.get::<_, String>(7)?,
                    "model": row.get::<_, String>(8)?,
                    "provider": row.get::<_, String>(9)?,
                    "cost_cents": row.get::<_, i64>(10)?,
                    "status": row.get::<_, String>(11)?,
                }))
            })
            .map_err(|e| RyvosError::Database(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| RyvosError::Database(e.to_string()))?);
        }
        Ok((results, total as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> (CostStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("cost.db");
        let store = CostStore::open(&db_path).unwrap();
        (store, dir)
    }

    #[test]
    fn record_and_query_cost() {
        let (store, _dir) = test_store();
        let event = CostEvent {
            run_id: "r1".into(),
            session_id: "s1".into(),
            timestamp: Utc::now(),
            input_tokens: 1000,
            output_tokens: 500,
            cost_cents: 10,
            billing_type: BillingType::Api,
            model: "claude-sonnet-4".into(),
            provider: "anthropic".into(),
        };
        store.record_cost_event(&event).unwrap();

        let monthly = store.monthly_spend_cents().unwrap();
        assert_eq!(monthly, 10);
    }

    #[test]
    fn record_run_lifecycle() {
        let (store, _dir) = test_store();
        store
            .record_run("r1", "s1", "claude-sonnet-4", "anthropic", BillingType::Api)
            .unwrap();
        store
            .complete_run("r1", 1000, 500, 3, 10, "complete")
            .unwrap();

        let (runs, total) = store.run_history(10, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(runs[0]["status"], "complete");
    }
}
