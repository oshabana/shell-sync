use crate::app::App;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

/// Poll for a crossterm event and handle it, updating app state.
///
/// Returns `true` if a search refresh is needed after this event.
pub fn handle_event(app: &mut App) -> anyhow::Result<bool> {
    if !event::poll(Duration::from_millis(50))? {
        return Ok(false);
    }

    let ev = event::read()?;
    match ev {
        Event::Key(key) => Ok(handle_key(app, key)),
        _ => Ok(false),
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match (key.code, ctrl) {
        // Ctrl+C / Escape: cancel
        (KeyCode::Char('c'), true) | (KeyCode::Esc, _) => {
            app.cancel();
            false
        }

        // Ctrl+R: cycle search mode
        (KeyCode::Char('r'), true) => {
            app.search_mode = app.search_mode.next();
            true
        }

        // Ctrl+S: cycle filter mode
        (KeyCode::Char('s'), true) => {
            app.filter_mode = app.filter_mode.next();
            true
        }

        // Enter: accept selected
        (KeyCode::Enter, _) => {
            app.accept_selected();
            false
        }

        // Tab: same as Enter (paste selected for inline mode)
        (KeyCode::Tab, _) => {
            app.accept_selected();
            false
        }

        // Up arrow: move selection up
        (KeyCode::Up, _) => {
            app.select_previous();
            false
        }

        // Down arrow: move selection down
        (KeyCode::Down, _) => {
            app.select_next();
            false
        }

        // Left arrow: move cursor left
        (KeyCode::Left, _) => {
            app.move_cursor_left();
            false
        }

        // Right arrow: move cursor right
        (KeyCode::Right, _) => {
            app.move_cursor_right();
            false
        }

        // Backspace: delete char before cursor
        (KeyCode::Backspace, _) => {
            app.delete_char();
            true
        }

        // Ctrl+U: clear input
        (KeyCode::Char('u'), true) => {
            app.input.clear();
            app.cursor = 0;
            true
        }

        // Ctrl+A: move to start
        (KeyCode::Char('a'), true) => {
            app.cursor = 0;
            false
        }

        // Ctrl+E: move to end
        (KeyCode::Char('e'), true) => {
            app.cursor = app.input.len();
            false
        }

        // Regular character input
        (KeyCode::Char(c), false) => {
            app.insert_char(c);
            true
        }

        _ => false,
    }
}
