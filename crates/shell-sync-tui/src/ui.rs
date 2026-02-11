use crate::app::App;
use chrono::{TimeZone, Utc};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

/// Render the entire TUI to the given frame.
pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // input bar
            Constraint::Min(3),   // results list
            Constraint::Length(3), // footer
        ])
        .split(frame.area());

    draw_input_bar(frame, app, chunks[0]);
    draw_results(frame, app, chunks[1]);
    draw_footer(frame, app, chunks[2]);
}

fn draw_input_bar(frame: &mut Frame, app: &App, area: Rect) {
    let search_label = format!("[{}]", app.search_mode.label());
    let filter_label = format!("[{}]", app.filter_mode.label());

    let input_line = Line::from(vec![
        Span::styled(
            &search_label,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            &filter_label,
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" > "),
        Span::raw(&app.input),
    ]);

    let input_widget = Paragraph::new(input_line).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" shell-sync search "),
    );
    frame.render_widget(input_widget, area);

    // Place cursor
    let cursor_x = area.x
        + 1 // border
        + search_label.len() as u16
        + 1 // space
        + filter_label.len() as u16
        + 3 // " > "
        + app.input[..app.cursor].len() as u16;
    let cursor_y = area.y + 1;
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn draw_results(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .results
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_selected = i == app.selected;

            let exit_style = if entry.exit_code != 0 {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            };

            let duration = format_duration(entry.duration_ms);
            let time = format_timestamp(entry.timestamp);

            let line = Line::from(vec![
                Span::styled(
                    &entry.command,
                    if is_selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
                Span::raw("  "),
                Span::styled(format!("E{}", entry.exit_code), exit_style),
                Span::raw("  "),
                Span::styled(duration, Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled(time, Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled(
                    truncate_cwd(&entry.cwd, 30),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);

            if is_selected {
                ListItem::new(line).style(Style::default().bg(Color::DarkGray))
            } else {
                ListItem::new(line)
            }
        })
        .collect();

    let title = format!(" Results ({}) ", app.results.len());
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));

    frame.render_widget(list, area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let help = if app.inline {
        "Enter/Tab: paste | Esc: cancel | Ctrl+R: mode | Ctrl+S: filter | Up/Down: navigate"
    } else {
        "Enter: select | Esc: cancel | Ctrl+R: mode | Ctrl+S: filter | Up/Down: navigate"
    };

    let filter_info = match app.filter_mode {
        crate::app::FilterMode::Global => String::new(),
        _ => format!(" | filter: {}", app.filter_value()),
    };

    let footer_line = Line::from(vec![
        Span::styled(help, Style::default().fg(Color::DarkGray)),
        Span::styled(filter_info, Style::default().fg(Color::Yellow)),
    ]);

    let footer = Paragraph::new(footer_line).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, area);
}

fn format_duration(ms: i64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("{}m{}s", mins, secs)
    }
}

fn format_timestamp(ts: i64) -> String {
    // Timestamp is in milliseconds
    let secs = ts / 1000;
    match Utc.timestamp_opt(secs, 0) {
        chrono::LocalResult::Single(dt) => dt.format("%m-%d %H:%M").to_string(),
        _ => "?".to_string(),
    }
}

fn truncate_cwd(cwd: &str, max_len: usize) -> String {
    if cwd.len() <= max_len {
        cwd.to_string()
    } else {
        // Show the last part of the path
        let truncated = &cwd[cwd.len() - max_len + 3..];
        format!("...{}", truncated)
    }
}
