pub mod app;
pub mod input;
pub mod search;
pub mod ui;

use app::App;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use shell_sync_core::config::history_db_path;
use shell_sync_core::db::SyncDatabase;
use std::io;

const SEARCH_LIMIT: i64 = 200;

/// Main entry point for the TUI search.
///
/// Opens the history database, runs the interactive search loop, and
/// prints the selected command to stdout (if any) when the user presses Enter.
pub fn run_search(query: &str, inline: bool) -> anyhow::Result<()> {
    let db_path = history_db_path();
    let db = SyncDatabase::open(db_path.to_str().unwrap_or("history.db"))?;

    let mut app = App::new(query, inline);

    // Initial search
    app.results = search::search(
        &db,
        &app.input,
        app.search_mode,
        app.filter_mode,
        app.filter_value(),
        SEARCH_LIMIT,
    );
    app.total_count = app.results.len() as i64;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stderr();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Main loop
    let result = run_loop(&mut terminal, &mut app, &db);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result?;

    // Print chosen command to stdout
    if let Some(cmd) = app.chosen {
        print!("{}", cmd);
    }

    Ok(())
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stderr>>,
    app: &mut App,
    db: &SyncDatabase,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if app.should_quit {
            break;
        }

        let needs_search = input::handle_event(app)?;

        if app.should_quit {
            break;
        }

        if needs_search {
            app.results = search::search(
                db,
                &app.input,
                app.search_mode,
                app.filter_mode,
                app.filter_value(),
                SEARCH_LIMIT,
            );
            app.total_count = app.results.len() as i64;
            // Reset selection to top when results change
            app.selected = 0;
        }
    }

    Ok(())
}
