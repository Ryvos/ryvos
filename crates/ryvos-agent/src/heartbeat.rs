use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use ryvos_core::config::HeartbeatConfig;
use ryvos_core::event::EventBus;
use ryvos_core::types::{AgentEvent, SessionId};

use crate::AgentRuntime;

/// Default prompt sent to the LLM during a heartbeat check.
const DEFAULT_PROMPT: &str =
    "Review the workspace. If everything is fine, respond with HEARTBEAT_OK. \
     If anything needs attention, describe it concisely.";

/// Ack patterns — if the response is short AND contains one of these, suppress it.
const ACK_PATTERNS: &[&str] = &[
    "HEARTBEAT_OK",
    "heartbeat_ok",
    "all good",
    "no issues",
    "nothing to report",
    "everything is fine",
    "all clear",
];

/// Periodic proactive agent check.
///
/// Fires at a configurable interval, reads `HEARTBEAT.md` from the workspace
/// for context, runs a mini agent turn, and either suppresses ack responses or
/// publishes a `HeartbeatAlert` event for routing to channels.
pub struct Heartbeat {
    config: HeartbeatConfig,
    runtime: Arc<AgentRuntime>,
    event_bus: Arc<EventBus>,
    cancel: CancellationToken,
    workspace: PathBuf,
}

impl Heartbeat {
    pub fn new(
        config: HeartbeatConfig,
        runtime: Arc<AgentRuntime>,
        event_bus: Arc<EventBus>,
        cancel: CancellationToken,
        workspace: PathBuf,
    ) -> Self {
        Self {
            config,
            runtime,
            event_bus,
            cancel,
            workspace,
        }
    }

    /// Run the heartbeat loop. Blocks until cancelled.
    pub async fn run(&self) {
        let interval = Duration::from_secs(self.config.interval_secs);
        info!(
            interval_secs = self.config.interval_secs,
            heartbeat_file = %self.config.heartbeat_file,
            "Heartbeat started"
        );

        loop {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = self.cancel.cancelled() => {
                    info!("Heartbeat shutting down");
                    break;
                }
            }

            // Check active hours
            if !self.is_within_active_hours() {
                info!("Heartbeat skipped — outside active hours");
                continue;
            }

            let now = Utc::now();
            let session_id =
                SessionId::from_string(&format!("heartbeat:{}", now.format("%Y%m%d-%H%M%S")));

            self.event_bus
                .publish(AgentEvent::HeartbeatFired { timestamp: now });

            let prompt = self.build_prompt();

            info!(session = %session_id, "Heartbeat firing");

            match self.runtime.run(&session_id, &prompt).await {
                Ok(response) => match evaluate_response(&response, self.config.ack_max_chars) {
                    HeartbeatResult::Ok => {
                        info!(session = %session_id, chars = response.len(), "Heartbeat OK (suppressed)");
                        self.event_bus.publish(AgentEvent::HeartbeatOk {
                            session_id,
                            response_chars: response.len(),
                        });
                    }
                    HeartbeatResult::Alert => {
                        warn!(session = %session_id, "Heartbeat alert");
                        self.event_bus.publish(AgentEvent::HeartbeatAlert {
                            session_id,
                            message: response,
                            target_channel: self.config.target_channel.clone(),
                        });
                    }
                },
                Err(e) => {
                    error!(session = %session_id, error = %e, "Heartbeat run failed");
                }
            }
        }
    }

    /// Build the prompt by reading HEARTBEAT.md (if it exists) and appending
    /// the configured or default heartbeat prompt.
    fn build_prompt(&self) -> String {
        let heartbeat_path = self.workspace.join(&self.config.heartbeat_file);
        let mut prompt = String::new();

        match std::fs::read_to_string(&heartbeat_path) {
            Ok(content) if !content.trim().is_empty() => {
                prompt.push_str("## Workspace Context (HEARTBEAT.md)\n\n");
                prompt.push_str(&content);
                prompt.push_str("\n\n---\n\n");
            }
            _ => {}
        }

        prompt.push_str(self.config.prompt.as_deref().unwrap_or(DEFAULT_PROMPT));

        prompt
    }

    /// Check whether the current time is within the configured active hours.
    fn is_within_active_hours(&self) -> bool {
        let active = match self.config.active_hours {
            Some(ref ah) => ah,
            None => return true, // No restriction
        };

        is_within_window(
            Utc::now(),
            active.start_hour,
            active.end_hour,
            active.utc_offset_hours,
        )
    }
}

/// Check whether `now` (UTC) falls within the `start_hour..end_hour` window
/// after applying `utc_offset_hours`.
///
/// Supports wrapping windows (e.g., 22..06 for overnight).
fn is_within_window(
    now: chrono::DateTime<Utc>,
    start_hour: u8,
    end_hour: u8,
    utc_offset_hours: i32,
) -> bool {
    let local_hour = {
        let h = now.format("%H").to_string().parse::<i32>().unwrap_or(0);
        ((h + utc_offset_hours).rem_euclid(24)) as u8
    };

    if start_hour <= end_hour {
        // Normal window: e.g., 9..22
        local_hour >= start_hour && local_hour < end_hour
    } else {
        // Wrapping window: e.g., 22..06 means 22-23 + 0-5
        local_hour >= start_hour || local_hour < end_hour
    }
}

#[derive(Debug, PartialEq)]
enum HeartbeatResult {
    Ok,
    Alert,
}

/// Evaluate the LLM response: short + contains ack pattern → Ok, else Alert.
fn evaluate_response(response: &str, ack_max_chars: usize) -> HeartbeatResult {
    if response.len() > ack_max_chars {
        return HeartbeatResult::Alert;
    }

    let lower = response.to_lowercase();
    for pattern in ACK_PATTERNS {
        if lower.contains(&pattern.to_lowercase()) {
            return HeartbeatResult::Ok;
        }
    }

    HeartbeatResult::Alert
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_active_hours_normal_window() {
        // 09:00–22:00 UTC+0, current time 14:00 UTC → inside
        let now = Utc.with_ymd_and_hms(2026, 2, 26, 14, 0, 0).unwrap();
        assert!(is_within_window(now, 9, 22, 0));

        // 08:00 UTC → outside
        let now = Utc.with_ymd_and_hms(2026, 2, 26, 8, 0, 0).unwrap();
        assert!(!is_within_window(now, 9, 22, 0));

        // 22:00 UTC → outside (end is exclusive)
        let now = Utc.with_ymd_and_hms(2026, 2, 26, 22, 0, 0).unwrap();
        assert!(!is_within_window(now, 9, 22, 0));
    }

    #[test]
    fn test_active_hours_wrapping_window() {
        // 22:00–06:00 (overnight), current time 23:00 → inside
        let now = Utc.with_ymd_and_hms(2026, 2, 26, 23, 0, 0).unwrap();
        assert!(is_within_window(now, 22, 6, 0));

        // 03:00 → inside
        let now = Utc.with_ymd_and_hms(2026, 2, 26, 3, 0, 0).unwrap();
        assert!(is_within_window(now, 22, 6, 0));

        // 10:00 → outside
        let now = Utc.with_ymd_and_hms(2026, 2, 26, 10, 0, 0).unwrap();
        assert!(!is_within_window(now, 22, 6, 0));
    }

    #[test]
    fn test_active_hours_with_offset() {
        // 09:00–22:00 UTC+2, current time 08:00 UTC → local time 10:00 → inside
        let now = Utc.with_ymd_and_hms(2026, 2, 26, 8, 0, 0).unwrap();
        assert!(is_within_window(now, 9, 22, 2));

        // 06:00 UTC → local time 08:00 → outside
        let now = Utc.with_ymd_and_hms(2026, 2, 26, 6, 0, 0).unwrap();
        assert!(!is_within_window(now, 9, 22, 2));
    }

    #[test]
    fn test_ack_detection() {
        assert_eq!(evaluate_response("HEARTBEAT_OK", 300), HeartbeatResult::Ok);
        assert_eq!(
            evaluate_response("All good, nothing to report.", 300),
            HeartbeatResult::Ok
        );
        assert_eq!(
            evaluate_response("No issues found. Everything is running normally.", 300),
            HeartbeatResult::Ok
        );
    }

    #[test]
    fn test_actionable_detection() {
        assert_eq!(
            evaluate_response("Disk usage is at 95%. Consider cleaning up /tmp.", 300),
            HeartbeatResult::Alert
        );
        // Short but no ack pattern → still alert
        assert_eq!(
            evaluate_response("Check the logs.", 300),
            HeartbeatResult::Alert
        );
        // Long response even with ack pattern → alert (over char limit)
        let long = format!("HEARTBEAT_OK {}", "x".repeat(300));
        assert_eq!(evaluate_response(&long, 300), HeartbeatResult::Alert);
    }
}
