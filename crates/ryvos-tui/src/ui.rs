use std::sync::OnceLock;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use tui_banner::Banner;

use crate::app::{App, MessageRole};

/// Cached banner — rendered once since it never changes.
struct BannerCache {
    lines: Vec<String>,
    /// Widest line in the banner (character count).
    width: u16,
    /// Number of lines.
    height: u16,
}

fn cached_banner() -> &'static BannerCache {
    static CACHE: OnceLock<BannerCache> = OnceLock::new();
    CACHE.get_or_init(|| {
        let text = Banner::new("RYVOS")
            .map(|b| b.style(tui_banner::Style::NeonCyber).render())
            .unwrap_or_else(|_| String::from("RYVOS"));
        let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        let width = lines
            .iter()
            .map(|l| l.chars().count() as u16)
            .max()
            .unwrap_or(5);
        let height = lines.len() as u16;
        BannerCache {
            lines,
            width,
            height,
        }
    })
}

/// Draw the TUI layout.
pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let term_w = area.width;
    let term_h = area.height;

    let banner = cached_banner();

    // Decide banner mode based on terminal size:
    // - Full ASCII art: need enough width for the art AND enough height so messages still have room
    // - Compact 1-line: when terminal is too small for art but still has some room
    // - Hidden: when terminal is very small (< 15 rows)
    let min_messages_rows: u16 = 8; // messages + status + input = at least this many rows
    let banner_height = if term_h < min_messages_rows + 3 {
        // Too small — no banner at all
        0
    } else if term_w >= banner.width + 2 && term_h >= banner.height + 1 + min_messages_rows {
        // Full banner fits: art height + 1 (bottom border)
        banner.height + 1
    } else {
        // Compact: version line + bottom border
        2
    };

    let constraints = if banner_height > 0 {
        vec![
            Constraint::Length(banner_height),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(3),
        ]
    } else {
        vec![
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(3),
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    if banner_height > 0 {
        draw_banner(f, chunks[0], banner_height);
        draw_messages(f, app, chunks[1]);
        draw_status_bar(f, app, chunks[2]);
        draw_input(f, app, chunks[3]);
    } else {
        draw_messages(f, app, chunks[0]);
        draw_status_bar(f, app, chunks[1]);
        draw_input(f, app, chunks[2]);
    }
}

fn draw_banner(f: &mut Frame, area: Rect, banner_height: u16) {
    let banner = cached_banner();

    let lines: Vec<Line> = if banner_height > 2 && area.width >= banner.width {
        // Full ASCII art
        banner
            .lines
            .iter()
            .map(|l| Line::from(Span::styled(l.clone(), Style::default().fg(Color::Cyan))))
            .collect()
    } else {
        // Compact: styled title
        vec![Line::from(vec![
            Span::styled(
                " RYVOS",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  Blazingly fast AI agent runtime",
                Style::default().fg(Color::DarkGray),
            ),
        ])]
    };

    let widget = Paragraph::new(lines)
        .block(Block::default().borders(Borders::BOTTOM))
        .style(Style::default());

    f.render_widget(widget, area);
}

fn draw_messages(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    for msg in &app.messages {
        let (prefix, style) = match msg.role {
            MessageRole::User => (
                "> ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            MessageRole::Assistant => ("", Style::default().fg(Color::White)),
            MessageRole::Tool => ("[tool] ", Style::default().fg(Color::Yellow)),
            MessageRole::Error => (
                "[error] ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            MessageRole::System => ("[system] ", Style::default().fg(Color::DarkGray)),
        };

        for text_line in msg.text.lines() {
            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(text_line.to_string(), style),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Add streaming text if any
    if !app.streaming_text.is_empty() {
        for text_line in app.streaming_text.lines() {
            lines.push(Line::from(Span::styled(
                text_line.to_string(),
                Style::default().fg(Color::White),
            )));
        }
    }

    // Calculate scroll
    let visible_height = area.height as usize;
    let total_lines = lines.len();
    let scroll = if app.scroll_offset > 0 {
        total_lines
            .saturating_sub(visible_height)
            .saturating_sub(app.scroll_offset)
    } else {
        total_lines.saturating_sub(visible_height)
    };

    let messages = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Messages "))
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0));

    f.render_widget(messages, area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let status_text = if app.is_running {
        let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let idx = (app.tick_count / 2) % spinner.len();
        let tool_info = if let Some(ref tool) = app.active_tool {
            format!(" [{}]", tool)
        } else {
            String::new()
        };
        format!(" {} Thinking...{}", spinner[idx], tool_info)
    } else {
        format!(
            " Session: {} | Tokens: {}in/{}out | /quit to exit",
            &app.session_id.to_string()[..8],
            app.total_input_tokens,
            app.total_output_tokens
        )
    };

    let status =
        Paragraph::new(status_text).style(Style::default().bg(Color::DarkGray).fg(Color::White));

    f.render_widget(status, area);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let input = Paragraph::new(app.input.buffer.as_str())
        .block(Block::default().borders(Borders::ALL).title(" Input "))
        .style(Style::default().fg(Color::White));

    f.render_widget(input, area);

    // Position cursor
    let cursor_x = area.x + 1 + app.input.cursor as u16;
    let cursor_y = area.y + 1;
    f.set_cursor_position((cursor_x.min(area.x + area.width - 2), cursor_y));
}
