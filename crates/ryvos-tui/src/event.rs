use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent};
use ryvos_core::types::AgentEvent;
use tokio::sync::broadcast;

/// Events that drive the TUI loop.
pub enum TuiEvent {
    /// A crossterm key/mouse/resize event.
    Key(crossterm::event::KeyEvent),
    #[allow(dead_code)]
    Resize(u16, u16),
    /// An agent event from the EventBus.
    Agent(AgentEvent),
    /// Tick timer for animations (spinners, etc.).
    Tick,
}

/// Merged event loop: crossterm + EventBus + tick timer.
pub struct EventLoop {
    agent_rx: broadcast::Receiver<AgentEvent>,
    tick_interval: Duration,
}

impl EventLoop {
    pub fn new(agent_rx: broadcast::Receiver<AgentEvent>) -> Self {
        Self {
            agent_rx,
            tick_interval: Duration::from_millis(100),
        }
    }

    /// Wait for the next event from any source.
    pub async fn next(&mut self) -> Option<TuiEvent> {
        let tick_sleep = tokio::time::sleep(self.tick_interval);

        // Poll crossterm in a blocking thread
        let crossterm_poll = tokio::task::spawn_blocking(|| {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                event::read().ok()
            } else {
                None
            }
        });

        tokio::select! {
            // Agent events
            result = self.agent_rx.recv() => {
                match result {
                    Ok(evt) => Some(TuiEvent::Agent(evt)),
                    Err(broadcast::error::RecvError::Lagged(_)) => Some(TuiEvent::Tick),
                    Err(_) => None,
                }
            }
            // Crossterm events
            result = crossterm_poll => {
                match result {
                    Ok(Some(CrosstermEvent::Key(key))) => Some(TuiEvent::Key(key)),
                    Ok(Some(CrosstermEvent::Resize(w, h))) => Some(TuiEvent::Resize(w, h)),
                    _ => Some(TuiEvent::Tick),
                }
            }
            // Tick timer
            _ = tick_sleep => {
                Some(TuiEvent::Tick)
            }
        }
    }
}
