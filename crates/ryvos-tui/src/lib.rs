mod app;
mod event;
mod input;
mod ui;

use std::sync::Arc;

use ryvos_agent::approval::ApprovalBroker;
use ryvos_agent::AgentRuntime;
use ryvos_core::event::EventBus;
use ryvos_core::types::SessionId;

/// Launch the terminal UI.
pub async fn run_tui(
    runtime: Arc<AgentRuntime>,
    event_bus: Arc<EventBus>,
    session_id: SessionId,
    broker: Option<Arc<ApprovalBroker>>,
) -> anyhow::Result<()> {
    // Enter raw mode
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = app::run_app(&mut terminal, runtime, event_bus, session_id, broker).await;

    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}
