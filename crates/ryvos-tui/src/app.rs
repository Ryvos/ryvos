use std::sync::Arc;

use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use ryvos_agent::AgentRuntime;
use ryvos_core::event::EventBus;
use ryvos_core::types::{AgentEvent, SessionId};

use crate::event::{EventLoop, TuiEvent};
use crate::input::{InputAction, InputHandler};
use crate::ui;

/// Role for display messages.
#[derive(Debug, Clone)]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    Error,
    System,
}

/// A message displayed in the TUI.
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub role: MessageRole,
    pub text: String,
}

/// Application state.
pub struct App {
    pub messages: Vec<DisplayMessage>,
    pub streaming_text: String,
    pub input: InputHandler,
    pub session_id: SessionId,
    pub is_running: bool,
    pub active_tool: Option<String>,
    pub scroll_offset: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub tick_count: usize,
}

impl App {
    pub fn new(session_id: SessionId) -> Self {
        Self {
            messages: vec![DisplayMessage {
                role: MessageRole::System,
                text: format!("Ryvos TUI — session {}", &session_id.to_string()[..8]),
            }],
            streaming_text: String::new(),
            input: InputHandler::new(),
            session_id,
            is_running: false,
            active_tool: None,
            scroll_offset: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            tick_count: 0,
        }
    }

    /// Handle an agent event.
    pub fn handle_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::RunStarted { .. } => {
                self.is_running = true;
                self.streaming_text.clear();
            }
            AgentEvent::TextDelta(text) => {
                self.streaming_text.push_str(&text);
            }
            AgentEvent::ToolStart { name, .. } => {
                self.active_tool = Some(name.clone());
                // Flush streaming text before tool output
                if !self.streaming_text.is_empty() {
                    let text = std::mem::take(&mut self.streaming_text);
                    self.messages.push(DisplayMessage {
                        role: MessageRole::Assistant,
                        text,
                    });
                }
                self.messages.push(DisplayMessage {
                    role: MessageRole::Tool,
                    text: format!("Running: {}", name),
                });
            }
            AgentEvent::ToolEnd { name, result } => {
                self.active_tool = None;
                let status = if result.is_error { "ERROR" } else { "ok" };
                let content = if result.content.len() > 200 {
                    format!("{}...", &result.content[..200])
                } else {
                    result.content.clone()
                };
                self.messages.push(DisplayMessage {
                    role: MessageRole::Tool,
                    text: format!("[{}: {}] {}", name, status, content),
                });
            }
            AgentEvent::RunComplete { input_tokens, output_tokens, .. } => {
                self.is_running = false;
                self.active_tool = None;
                self.total_input_tokens += input_tokens;
                self.total_output_tokens += output_tokens;

                // Flush remaining streaming text
                if !self.streaming_text.is_empty() {
                    let text = std::mem::take(&mut self.streaming_text);
                    self.messages.push(DisplayMessage {
                        role: MessageRole::Assistant,
                        text,
                    });
                }
                self.scroll_offset = 0;
            }
            AgentEvent::RunError { error } => {
                self.is_running = false;
                self.active_tool = None;
                self.streaming_text.clear();
                self.messages.push(DisplayMessage {
                    role: MessageRole::Error,
                    text: error,
                });
            }
            AgentEvent::ApprovalRequested { request } => {
                self.messages.push(DisplayMessage {
                    role: MessageRole::System,
                    text: format!(
                        "[APPROVAL REQUIRED] {} ({}): \"{}\" -- /approve {} or /deny {}",
                        request.tool_name,
                        request.tier,
                        request.input_summary,
                        &request.id[..8.min(request.id.len())],
                        &request.id[..8.min(request.id.len())],
                    ),
                });
            }
            AgentEvent::ApprovalResolved { .. } => {}
            AgentEvent::ToolBlocked { name, tier, reason } => {
                self.messages.push(DisplayMessage {
                    role: MessageRole::Error,
                    text: format!("[BLOCKED] {} ({}): {}", name, tier, reason),
                });
            }
            AgentEvent::TurnComplete { .. } => {}
            AgentEvent::CronFired { .. } => {}
            AgentEvent::GuardianStall { elapsed_secs, turn, .. } => {
                self.messages.push(DisplayMessage {
                    role: MessageRole::System,
                    text: format!("[GUARDIAN] Stall detected: {}s at turn {}", elapsed_secs, turn),
                });
            }
            AgentEvent::GuardianDoomLoop { tool_name, consecutive_calls, .. } => {
                self.messages.push(DisplayMessage {
                    role: MessageRole::System,
                    text: format!("[GUARDIAN] Doom loop: {} x{}", tool_name, consecutive_calls),
                });
            }
            AgentEvent::GuardianBudgetAlert { used_tokens, budget_tokens, is_hard_stop, .. } => {
                let kind = if is_hard_stop { "HARD STOP" } else { "warning" };
                self.messages.push(DisplayMessage {
                    role: MessageRole::System,
                    text: format!("[GUARDIAN] Budget {}: {}/{} tokens", kind, used_tokens, budget_tokens),
                });
            }
            AgentEvent::GoalEvaluated { evaluation, .. } => {
                let status = if evaluation.passed { "PASSED" } else { "FAILED" };
                self.messages.push(DisplayMessage {
                    role: MessageRole::System,
                    text: format!("[GOAL {}] score: {:.0}%", status, evaluation.overall_score * 100.0),
                });
            }
            AgentEvent::GuardianHint { .. }
            | AgentEvent::UsageUpdate { .. }
            | AgentEvent::DecisionMade { .. } => {}
        }
    }
}

/// Main app loop.
pub async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    runtime: Arc<AgentRuntime>,
    event_bus: Arc<EventBus>,
    session_id: SessionId,
) -> anyhow::Result<()> {
    let mut app = App::new(session_id.clone());
    let agent_rx = event_bus.subscribe();
    let mut events = EventLoop::new(agent_rx);

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        if let Some(event) = events.next().await {
            match event {
                TuiEvent::Key(key) => {
                    let action = app.input.handle_key(key);
                    match action {
                        InputAction::Quit => break,
                        InputAction::Clear => {
                            app.messages.clear();
                            app.messages.push(DisplayMessage {
                                role: MessageRole::System,
                                text: "Cleared.".to_string(),
                            });
                        }
                        InputAction::Submit(text) => {
                            if app.is_running {
                                continue;
                            }
                            app.messages.push(DisplayMessage {
                                role: MessageRole::User,
                                text: text.clone(),
                            });
                            app.scroll_offset = 0;

                            // Spawn agent run
                            let rt = runtime.clone();
                            let sid = session_id.clone();
                            let eb = event_bus.clone();
                            tokio::spawn(async move {
                                if let Err(e) = rt.run(&sid, &text).await {
                                    eb.publish(AgentEvent::RunError {
                                        error: e.to_string(),
                                    });
                                }
                            });
                        }
                        InputAction::ScrollUp => {
                            app.scroll_offset = app.scroll_offset.saturating_add(3);
                        }
                        InputAction::ScrollDown => {
                            app.scroll_offset = app.scroll_offset.saturating_sub(3);
                        }
                        InputAction::Newline | InputAction::None => {}
                    }
                }
                TuiEvent::Agent(event) => {
                    app.handle_agent_event(event);
                }
                TuiEvent::Tick => {
                    app.tick_count += 1;
                }
                TuiEvent::Resize(_, _) => {}
            }
        }
    }

    Ok(())
}
