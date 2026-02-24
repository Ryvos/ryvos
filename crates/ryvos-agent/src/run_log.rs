use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use ryvos_core::event::EventBus;
use ryvos_core::types::{AgentEvent, SessionId};

/// JSONL runtime logger.
///
/// Subscribes to the EventBus and writes structured log entries as JSONL
/// (one JSON object per line). This format is append-only and crash-resilient:
/// even if the process dies mid-run, all previously written lines are intact.
pub struct RunLogger {
    log_dir: PathBuf,
    level: u8,
}

/// A single log entry written to the JSONL file.
#[derive(Serialize)]
struct LogEntry {
    timestamp: String,
    session_id: String,
    event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<serde_json::Value>,
}

impl RunLogger {
    /// Create a new RunLogger.
    ///
    /// `log_dir` is the base directory; logs are written to
    /// `{log_dir}/{session_id}/{timestamp}.jsonl`.
    /// `level` controls verbosity: 1=summary, 2=per-turn, 3=per-step.
    pub fn new(log_dir: PathBuf, level: u8) -> Self {
        Self { log_dir, level }
    }

    /// Run the logger as a background task.
    ///
    /// Subscribes to the EventBus and writes JSONL until cancellation or RunComplete/RunError.
    pub async fn run(
        self,
        event_bus: Arc<EventBus>,
        session_id: SessionId,
        cancel: CancellationToken,
    ) {
        let session_dir = self.log_dir.join(&session_id.0);
        if let Err(e) = tokio::fs::create_dir_all(&session_dir).await {
            error!(error = %e, "Failed to create log directory");
            return;
        }

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let log_path = session_dir.join(format!("{}.jsonl", timestamp));

        let file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
        {
            Ok(f) => f,
            Err(e) => {
                error!(error = %e, path = %log_path.display(), "Failed to open log file");
                return;
            }
        };

        info!(path = %log_path.display(), "RunLogger started");

        let mut writer = tokio::io::BufWriter::new(file);
        let mut rx = event_bus.subscribe();
        let sid = session_id.0.clone();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    debug!("RunLogger cancelled");
                    break;
                }
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            let entry = match self.event_to_entry(&sid, &event) {
                                Some(e) => e,
                                None => continue,
                            };

                            if let Ok(json) = serde_json::to_string(&entry) {
                                let line = format!("{}\n", json);
                                if let Err(e) = writer.write_all(line.as_bytes()).await {
                                    error!(error = %e, "Failed to write log entry");
                                    break;
                                }
                                // Flush after each entry for crash resilience
                                if let Err(e) = writer.flush().await {
                                    error!(error = %e, "Failed to flush log");
                                }
                            }

                            // Stop logging after run completes or errors
                            if matches!(event, AgentEvent::RunComplete { .. } | AgentEvent::RunError { .. }) {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            debug!(skipped = n, "RunLogger lagged, skipped events");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            debug!("EventBus closed, RunLogger stopping");
                            break;
                        }
                    }
                }
            }
        }

        // Final flush
        writer.flush().await.ok();
        debug!(path = %log_path.display(), "RunLogger finished");
    }

    /// Convert an AgentEvent to a log entry (returns None if filtered by level).
    fn event_to_entry(&self, session_id: &str, event: &AgentEvent) -> Option<LogEntry> {
        let ts = Utc::now().to_rfc3339();

        match event {
            // L1: Always logged (run summary)
            AgentEvent::RunStarted { .. } => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "run_started".to_string(),
                turn: None,
                detail: None,
            }),
            AgentEvent::RunComplete {
                total_turns,
                input_tokens,
                output_tokens,
                ..
            } => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "run_complete".to_string(),
                turn: Some(*total_turns),
                detail: Some(serde_json::json!({
                    "total_turns": total_turns,
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                })),
            }),
            AgentEvent::RunError { error } => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "run_error".to_string(),
                turn: None,
                detail: Some(serde_json::json!({ "error": error })),
            }),

            // L2: Per-turn events (level >= 2)
            AgentEvent::TurnComplete { turn } if self.level >= 2 => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "turn_complete".to_string(),
                turn: Some(*turn),
                detail: None,
            }),
            AgentEvent::UsageUpdate {
                input_tokens,
                output_tokens,
            } if self.level >= 2 => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "usage_update".to_string(),
                turn: None,
                detail: Some(serde_json::json!({
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                })),
            }),
            AgentEvent::GoalEvaluated { evaluation, .. } if self.level >= 2 => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "goal_evaluated".to_string(),
                turn: None,
                detail: Some(serde_json::json!({
                    "overall_score": evaluation.overall_score,
                    "passed": evaluation.passed,
                })),
            }),

            // L3: Per-step events (level >= 3)
            AgentEvent::ToolStart { name, input } if self.level >= 3 => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "tool_start".to_string(),
                turn: None,
                detail: Some(serde_json::json!({
                    "tool": name,
                    "input": truncate_json(input, 500),
                })),
            }),
            AgentEvent::ToolEnd { name, result } if self.level >= 3 => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "tool_end".to_string(),
                turn: None,
                detail: Some(serde_json::json!({
                    "tool": name,
                    "is_error": result.is_error,
                    "content_preview": truncate_str(&result.content, 200),
                })),
            }),
            AgentEvent::ToolBlocked { name, tier, reason } if self.level >= 3 => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "tool_blocked".to_string(),
                turn: None,
                detail: Some(serde_json::json!({
                    "tool": name,
                    "tier": format!("{}", tier),
                    "reason": reason,
                })),
            }),
            AgentEvent::DecisionMade { decision } if self.level >= 3 => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "decision_made".to_string(),
                turn: Some(decision.turn),
                detail: Some(serde_json::json!({
                    "description": decision.description,
                    "chosen": decision.chosen_option,
                })),
            }),

            // Guardian events: always logged at L2+
            AgentEvent::GuardianStall { turn, elapsed_secs, .. } if self.level >= 2 => {
                Some(LogEntry {
                    timestamp: ts,
                    session_id: session_id.to_string(),
                    event_type: "guardian_stall".to_string(),
                    turn: Some(*turn),
                    detail: Some(serde_json::json!({ "elapsed_secs": elapsed_secs })),
                })
            }
            AgentEvent::GuardianDoomLoop {
                tool_name,
                consecutive_calls,
                ..
            } if self.level >= 2 => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "guardian_doom_loop".to_string(),
                turn: None,
                detail: Some(serde_json::json!({
                    "tool": tool_name,
                    "consecutive_calls": consecutive_calls,
                })),
            }),
            AgentEvent::GuardianBudgetAlert {
                used_tokens,
                budget_tokens,
                is_hard_stop,
                ..
            } if self.level >= 2 => Some(LogEntry {
                timestamp: ts,
                session_id: session_id.to_string(),
                event_type: "guardian_budget_alert".to_string(),
                turn: None,
                detail: Some(serde_json::json!({
                    "used_tokens": used_tokens,
                    "budget_tokens": budget_tokens,
                    "is_hard_stop": is_hard_stop,
                })),
            }),

            // Everything else: not logged (TextDelta, ThinkingDelta, etc.)
            _ => None,
        }
    }
}

/// Truncate a JSON value for logging.
fn truncate_json(value: &serde_json::Value, max_len: usize) -> String {
    let s = value.to_string();
    if s.len() <= max_len {
        s
    } else {
        format!("{}...", &s[..max_len])
    }
}

/// Truncate a string for logging.
fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jsonl_append_format() {
        let entry = LogEntry {
            timestamp: "2026-02-24T12:00:00Z".to_string(),
            session_id: "test-session".to_string(),
            event_type: "run_started".to_string(),
            turn: None,
            detail: None,
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("run_started"));
        assert!(json.contains("test-session"));
        // Should not contain "turn" or "detail" (they're None and skip_serializing_if)
        assert!(!json.contains("turn"));
        assert!(!json.contains("detail"));
    }

    #[test]
    fn test_log_entry_with_detail() {
        let entry = LogEntry {
            timestamp: "2026-02-24T12:00:00Z".to_string(),
            session_id: "sess-1".to_string(),
            event_type: "run_complete".to_string(),
            turn: Some(5),
            detail: Some(serde_json::json!({
                "total_turns": 5,
                "input_tokens": 1000,
                "output_tokens": 500,
            })),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"turn\":5"));
        assert!(json.contains("input_tokens"));
    }

    #[test]
    fn test_truncate_json() {
        let value = serde_json::json!({"key": "a very long string that should be truncated"});
        let result = truncate_json(&value, 20);
        assert!(result.len() <= 24); // 20 + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_event_level_filtering() {
        let logger = RunLogger::new(PathBuf::from("/tmp"), 1);
        let sid = "test";

        // L1 events always pass
        assert!(logger
            .event_to_entry(
                sid,
                &AgentEvent::RunStarted {
                    session_id: SessionId::from_string("test")
                }
            )
            .is_some());

        // L2 events filtered at level 1
        assert!(logger
            .event_to_entry(sid, &AgentEvent::TurnComplete { turn: 0 })
            .is_none());

        // L3 events filtered at level 1
        assert!(logger
            .event_to_entry(
                sid,
                &AgentEvent::ToolStart {
                    name: "bash".to_string(),
                    input: serde_json::Value::Null,
                }
            )
            .is_none());

        // Level 2 logger passes L2 events
        let logger2 = RunLogger::new(PathBuf::from("/tmp"), 2);
        assert!(logger2
            .event_to_entry(sid, &AgentEvent::TurnComplete { turn: 0 })
            .is_some());

        // Level 3 logger passes L3 events
        let logger3 = RunLogger::new(PathBuf::from("/tmp"), 3);
        assert!(logger3
            .event_to_entry(
                sid,
                &AgentEvent::ToolStart {
                    name: "bash".to_string(),
                    input: serde_json::Value::Null,
                }
            )
            .is_some());
    }
}
