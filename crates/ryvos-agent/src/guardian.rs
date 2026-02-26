use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use ryvos_core::config::GuardianConfig;
use ryvos_core::event::EventBus;
use ryvos_core::types::{AgentEvent, SessionId};

/// Action the Guardian sends to the agent loop.
#[derive(Debug, Clone)]
pub enum GuardianAction {
    /// Inject a corrective hint as a user message.
    InjectHint(String),
    /// Cancel the run with a reason.
    CancelRun(String),
}

/// Record of a recent tool call for doom loop detection.
#[derive(Debug, Clone)]
struct ToolCallRecord {
    name: String,
    input_fingerprint: String,
}

/// Guardian Watchdog — event-driven background monitor that detects anomalies
/// and injects corrective actions into the agent loop.
pub struct Guardian {
    config: GuardianConfig,
    event_bus: Arc<EventBus>,
    cancel: CancellationToken,
    hint_tx: mpsc::Sender<GuardianAction>,
}

impl Guardian {
    /// Create a new Guardian and its action receiver.
    /// The receiver should be passed to `AgentRuntime::set_guardian_hints()`.
    pub fn new(
        config: GuardianConfig,
        event_bus: Arc<EventBus>,
        cancel: CancellationToken,
    ) -> (Self, mpsc::Receiver<GuardianAction>) {
        let (hint_tx, hint_rx) = mpsc::channel(32);
        let guardian = Self {
            config,
            event_bus,
            cancel,
            hint_tx,
        };
        (guardian, hint_rx)
    }

    /// Run the Guardian event loop. Spawned as a tokio task.
    pub async fn run(self, session_id: SessionId) {
        let mut rx = self.event_bus.subscribe();
        let threshold = self.config.doom_loop_threshold;
        let stall_timeout = std::time::Duration::from_secs(self.config.stall_timeout_secs);
        let token_budget = self.config.token_budget;
        let warn_pct = self.config.token_warn_pct as u64;

        // Doom loop tracking
        let deque_capacity = threshold * 2;
        let mut recent_tools: VecDeque<ToolCallRecord> = VecDeque::with_capacity(deque_capacity);

        // Stall detection
        let mut last_progress = Instant::now();

        // Budget monitoring
        let mut total_tokens: u64 = 0;
        let mut warned = false;
        let mut hard_stopped = false;

        info!("Guardian watchdog started");

        loop {
            let stall_remaining = stall_timeout.saturating_sub(last_progress.elapsed());

            tokio::select! {
                event = rx.recv() => {
                    let Ok(event) = event else { break };
                    match event {
                        AgentEvent::ToolStart { ref name, ref input } => {
                            let fingerprint = {
                                let s = serde_json::to_string(input).unwrap_or_default();
                                s.chars().take(200).collect::<String>()
                            };
                            recent_tools.push_back(ToolCallRecord {
                                name: name.clone(),
                                input_fingerprint: fingerprint,
                            });
                            while recent_tools.len() > deque_capacity {
                                recent_tools.pop_front();
                            }

                            // Check for doom loop: last N calls have same name + fingerprint
                            if recent_tools.len() >= threshold {
                                let tail: Vec<_> = recent_tools.iter().rev().take(threshold).collect();
                                let first = &tail[0];
                                let is_doom_loop = tail.iter().all(|r| {
                                    r.name == first.name && r.input_fingerprint == first.input_fingerprint
                                });

                                if is_doom_loop {
                                    let tool_name = first.name.clone();
                                    let count = threshold;
                                    warn!(
                                        tool = %tool_name,
                                        consecutive = count,
                                        "Guardian: doom loop detected"
                                    );

                                    self.event_bus.publish(AgentEvent::GuardianDoomLoop {
                                        session_id: session_id.clone(),
                                        tool_name: tool_name.clone(),
                                        consecutive_calls: count,
                                    });

                                    let hint = format!(
                                        "[Guardian] You have called '{}' {} times with identical input. \
                                         This looks like an infinite loop. Stop repeating this call and \
                                         try a different approach.",
                                        tool_name, count
                                    );
                                    self.event_bus.publish(AgentEvent::GuardianHint {
                                        session_id: session_id.clone(),
                                        message: hint.clone(),
                                    });
                                    let _ = self.hint_tx.send(GuardianAction::InjectHint(hint)).await;
                                    recent_tools.clear();
                                }
                            }
                        }
                        AgentEvent::ToolEnd { .. } | AgentEvent::TurnComplete { .. } => {
                            last_progress = Instant::now();
                        }
                        AgentEvent::UsageUpdate { input_tokens, output_tokens } => {
                            total_tokens += input_tokens + output_tokens;

                            if token_budget > 0 && !hard_stopped {
                                let warn_threshold = token_budget * warn_pct / 100;

                                if !warned && total_tokens >= warn_threshold {
                                    warned = true;
                                    warn!(
                                        used = total_tokens,
                                        budget = token_budget,
                                        "Guardian: token budget warning"
                                    );
                                    self.event_bus.publish(AgentEvent::GuardianBudgetAlert {
                                        session_id: session_id.clone(),
                                        used_tokens: total_tokens,
                                        budget_tokens: token_budget,
                                        is_hard_stop: false,
                                    });
                                    let hint = format!(
                                        "[Guardian] Token budget warning: {}/{} tokens used ({}%). \
                                         Please wrap up your current task efficiently.",
                                        total_tokens, token_budget, total_tokens * 100 / token_budget
                                    );
                                    self.event_bus.publish(AgentEvent::GuardianHint {
                                        session_id: session_id.clone(),
                                        message: hint.clone(),
                                    });
                                    let _ = self.hint_tx.send(GuardianAction::InjectHint(hint)).await;
                                }

                                if total_tokens >= token_budget {
                                    hard_stopped = true;
                                    warn!(
                                        used = total_tokens,
                                        budget = token_budget,
                                        "Guardian: token budget exceeded — cancelling run"
                                    );
                                    self.event_bus.publish(AgentEvent::GuardianBudgetAlert {
                                        session_id: session_id.clone(),
                                        used_tokens: total_tokens,
                                        budget_tokens: token_budget,
                                        is_hard_stop: true,
                                    });
                                    let reason = format!(
                                        "Token budget exceeded: {}/{}",
                                        total_tokens, token_budget
                                    );
                                    let _ = self.hint_tx.send(GuardianAction::CancelRun(reason)).await;
                                    self.cancel.cancel();
                                }
                            }
                        }
                        AgentEvent::RunComplete { .. } | AgentEvent::RunError { .. } => {
                            // Reset state for next run
                            recent_tools.clear();
                            last_progress = Instant::now();
                            total_tokens = 0;
                            warned = false;
                            hard_stopped = false;
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep(stall_remaining) => {
                    if last_progress.elapsed() >= stall_timeout {
                        let elapsed = last_progress.elapsed().as_secs();
                        warn!(
                            elapsed_secs = elapsed,
                            "Guardian: stall detected"
                        );
                        self.event_bus.publish(AgentEvent::GuardianStall {
                            session_id: session_id.clone(),
                            turn: 0,
                            elapsed_secs: elapsed,
                        });
                        let hint = format!(
                            "[Guardian] No progress detected for {}s. \
                             If you are stuck, try a different approach or ask the user for help.",
                            elapsed
                        );
                        self.event_bus.publish(AgentEvent::GuardianHint {
                            session_id: session_id.clone(),
                            message: hint.clone(),
                        });
                        let _ = self.hint_tx.send(GuardianAction::InjectHint(hint)).await;
                        last_progress = Instant::now(); // Reset to avoid spam
                    }
                }
                _ = self.cancel.cancelled() => {
                    info!("Guardian watchdog stopped");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn doom_loop_detection() {
        let event_bus = Arc::new(EventBus::default());
        let cancel = CancellationToken::new();
        let config = GuardianConfig {
            enabled: true,
            doom_loop_threshold: 3,
            stall_timeout_secs: 300, // long timeout to avoid interference
            token_budget: 0,
            token_warn_pct: 80,
        };

        let (guardian, mut hint_rx) = Guardian::new(config, event_bus.clone(), cancel.clone());
        let session_id = SessionId::new();
        let handle = tokio::spawn(guardian.run(session_id));

        // Let the Guardian's event loop start and subscribe
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Send 3 identical ToolStart events
        let input = serde_json::json!({"command": "echo hello"});
        for _ in 0..3 {
            event_bus.publish(AgentEvent::ToolStart {
                name: "bash".to_string(),
                input: input.clone(),
            });
        }

        // Should receive an InjectHint
        let action = tokio::time::timeout(std::time::Duration::from_secs(2), hint_rx.recv())
            .await
            .expect("timeout waiting for hint")
            .expect("channel closed");

        match action {
            GuardianAction::InjectHint(msg) => {
                assert!(msg.contains("bash"), "hint should mention tool name");
                assert!(msg.contains("3 times"), "hint should mention count");
            }
            GuardianAction::CancelRun(_) => panic!("expected InjectHint, got CancelRun"),
        }

        cancel.cancel();
        handle.await.ok();
    }

    #[tokio::test]
    async fn no_doom_loop_on_different_tools() {
        let event_bus = Arc::new(EventBus::default());
        let cancel = CancellationToken::new();
        let config = GuardianConfig {
            enabled: true,
            doom_loop_threshold: 3,
            stall_timeout_secs: 300,
            token_budget: 0,
            token_warn_pct: 80,
        };

        let (guardian, mut hint_rx) = Guardian::new(config, event_bus.clone(), cancel.clone());
        let session_id = SessionId::new();
        let handle = tokio::spawn(guardian.run(session_id));

        // Let the Guardian's event loop start and subscribe
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Send different tool calls
        event_bus.publish(AgentEvent::ToolStart {
            name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        });
        event_bus.publish(AgentEvent::ToolStart {
            name: "read".to_string(),
            input: serde_json::json!({"path": "/tmp"}),
        });
        event_bus.publish(AgentEvent::ToolStart {
            name: "write".to_string(),
            input: serde_json::json!({"path": "/tmp/out"}),
        });

        // Should NOT receive any hint
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(200), hint_rx.recv()).await;

        assert!(
            result.is_err(),
            "should not receive hint for different tools"
        );

        cancel.cancel();
        handle.await.ok();
    }

    #[tokio::test]
    async fn stall_detection() {
        let event_bus = Arc::new(EventBus::default());
        let cancel = CancellationToken::new();
        let config = GuardianConfig {
            enabled: true,
            doom_loop_threshold: 3,
            stall_timeout_secs: 1, // 1 second for fast test
            token_budget: 0,
            token_warn_pct: 80,
        };

        let (guardian, mut hint_rx) = Guardian::new(config, event_bus.clone(), cancel.clone());
        let session_id = SessionId::new();
        let handle = tokio::spawn(guardian.run(session_id));

        // Wait for stall to trigger
        let action = tokio::time::timeout(std::time::Duration::from_secs(3), hint_rx.recv())
            .await
            .expect("timeout waiting for stall hint")
            .expect("channel closed");

        match action {
            GuardianAction::InjectHint(msg) => {
                assert!(msg.contains("No progress"), "hint should mention stall");
            }
            GuardianAction::CancelRun(_) => panic!("expected InjectHint, got CancelRun"),
        }

        cancel.cancel();
        handle.await.ok();
    }

    #[test]
    fn guardian_config_defaults() {
        let config = GuardianConfig::default();
        assert!(config.enabled);
        assert_eq!(config.doom_loop_threshold, 3);
        assert_eq!(config.stall_timeout_secs, 120);
        assert_eq!(config.token_budget, 0);
        assert_eq!(config.token_warn_pct, 80);
    }
}
