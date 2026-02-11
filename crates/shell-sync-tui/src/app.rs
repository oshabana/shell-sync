use shell_sync_core::models::HistoryEntry;

/// How the search query is matched against commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Fuzzy,
    Prefix,
    Fulltext,
    Regex,
}

impl SearchMode {
    /// Cycle to the next search mode.
    pub fn next(self) -> Self {
        match self {
            Self::Fuzzy => Self::Prefix,
            Self::Prefix => Self::Fulltext,
            Self::Fulltext => Self::Regex,
            Self::Regex => Self::Fuzzy,
        }
    }

    /// Short label for the mode indicator.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Fuzzy => "FUZZY",
            Self::Prefix => "PREFIX",
            Self::Fulltext => "FULL",
            Self::Regex => "REGEX",
        }
    }
}

/// Which subset of history entries to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    Global,
    Host,
    Session,
    Directory,
}

impl FilterMode {
    /// Cycle to the next filter mode.
    pub fn next(self) -> Self {
        match self {
            Self::Global => Self::Host,
            Self::Host => Self::Session,
            Self::Session => Self::Directory,
            Self::Directory => Self::Global,
        }
    }

    /// Short label for the filter indicator.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Global => "GLOBAL",
            Self::Host => "HOST",
            Self::Session => "SESSION",
            Self::Directory => "DIR",
        }
    }
}

/// Application state for the TUI search.
pub struct App {
    /// Current search mode.
    pub search_mode: SearchMode,
    /// Current filter mode.
    pub filter_mode: FilterMode,
    /// Text typed by the user in the search bar.
    pub input: String,
    /// Cursor position within `input`.
    pub cursor: usize,
    /// Search results currently displayed.
    pub results: Vec<HistoryEntry>,
    /// Index of the selected result (0-based).
    pub selected: usize,
    /// Total number of results available.
    pub total_count: i64,
    /// Whether running in inline mode (for shell integration).
    pub inline: bool,
    /// The selected command to return on Enter (None if cancelled).
    pub chosen: Option<String>,
    /// Whether the app should quit.
    pub should_quit: bool,
    /// Current hostname for host-filter.
    pub current_hostname: String,
    /// Current session id for session-filter.
    pub current_session_id: String,
    /// Current working directory for dir-filter.
    pub current_cwd: String,
}

impl App {
    pub fn new(initial_query: &str, inline: bool) -> Self {
        let hostname = hostname();
        let session_id = std::env::var("SHELL_SYNC_SESSION_ID").unwrap_or_default();
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        Self {
            search_mode: SearchMode::Fuzzy,
            filter_mode: FilterMode::Global,
            input: initial_query.to_string(),
            cursor: initial_query.len(),
            results: Vec::new(),
            selected: 0,
            total_count: 0,
            inline,
            chosen: None,
            should_quit: false,
            current_hostname: hostname,
            current_session_id: session_id,
            current_cwd: cwd,
        }
    }

    /// Returns the filter value string for the current filter mode.
    pub fn filter_value(&self) -> &str {
        match self.filter_mode {
            FilterMode::Global => "",
            FilterMode::Host => &self.current_hostname,
            FilterMode::Session => &self.current_session_id,
            FilterMode::Directory => &self.current_cwd,
        }
    }

    /// Move selection up.
    pub fn select_previous(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if !self.results.is_empty() && self.selected < self.results.len() - 1 {
            self.selected += 1;
        }
    }

    /// Accept the currently selected item.
    pub fn accept_selected(&mut self) {
        if let Some(entry) = self.results.get(self.selected) {
            self.chosen = Some(entry.command.clone());
        }
        self.should_quit = true;
    }

    /// Cancel without selecting anything.
    pub fn cancel(&mut self) {
        self.chosen = None;
        self.should_quit = true;
    }

    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Delete the character before the cursor.
    pub fn delete_char(&mut self) {
        if self.cursor > 0 {
            let prev = self.input[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    /// Move cursor left by one character.
    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.input[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right by one character.
    pub fn move_cursor_right(&mut self) {
        if self.cursor < self.input.len() {
            self.cursor = self.input[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.input.len());
        }
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        })
}
