use crate::app::{FilterMode, SearchMode};
use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::Matcher;
use shell_sync_core::db::SyncDatabase;
use shell_sync_core::models::HistoryEntry;

/// Execute a search against the local history database.
///
/// Returns matching entries (up to `limit`) for the given query, mode, and filter.
pub fn search(
    db: &SyncDatabase,
    query: &str,
    mode: SearchMode,
    filter: FilterMode,
    filter_value: &str,
    limit: i64,
) -> Vec<HistoryEntry> {
    // Build filter args from filter mode
    let (machine_id, session_id, cwd) = match filter {
        FilterMode::Global => (None, None, None),
        FilterMode::Host => {
            if filter_value.is_empty() {
                (None, None, None)
            } else {
                // Host filter: we match on hostname, but DB filters on machine_id.
                // We'll do a broad SQL search then filter on hostname in post.
                (None, None, None)
            }
        }
        FilterMode::Session => {
            if filter_value.is_empty() {
                (None, None, None)
            } else {
                (None, Some(filter_value), None)
            }
        }
        FilterMode::Directory => {
            if filter_value.is_empty() {
                (None, None, None)
            } else {
                (None, None, Some(filter_value))
            }
        }
    };

    match mode {
        SearchMode::Fuzzy => search_fuzzy(db, query, machine_id, session_id, cwd, filter, filter_value, limit),
        SearchMode::Prefix => search_prefix(db, query, machine_id, session_id, cwd, filter, filter_value, limit),
        SearchMode::Fulltext => search_fulltext(db, query, machine_id, session_id, cwd, filter, filter_value, limit),
        SearchMode::Regex => search_regex(db, query, machine_id, session_id, cwd, filter, filter_value, limit),
    }
}

fn search_fuzzy(
    db: &SyncDatabase,
    query: &str,
    _machine_id: Option<&str>,
    session_id: Option<&str>,
    cwd: Option<&str>,
    filter: FilterMode,
    filter_value: &str,
    limit: i64,
) -> Vec<HistoryEntry> {
    if query.is_empty() {
        // No query: return most recent entries
        return db
            .search_history("", None, session_id, cwd, limit, 0)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| apply_host_filter(e, filter, filter_value))
            .collect();
    }

    // Fetch a broad set and rank with nucleo
    let broad_limit = limit * 10;
    let candidates = db
        .search_history("", None, session_id, cwd, broad_limit, 0)
        .unwrap_or_default();

    let mut matcher = Matcher::new(nucleo::Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);

    let mut scored: Vec<(i64, HistoryEntry)> = candidates
        .into_iter()
        .filter(|e| apply_host_filter(e, filter, filter_value))
        .filter_map(|entry| {
            let mut buf = Vec::new();
            let haystack = nucleo::Utf32Str::new(&entry.command, &mut buf);
            let score = pattern.score(haystack, &mut matcher)?;
            Some((score as i64, entry))
        })
        .collect();

    // Sort by score descending, then by timestamp descending for ties
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.timestamp.cmp(&a.1.timestamp)));

    scored
        .into_iter()
        .take(limit as usize)
        .map(|(_, e)| e)
        .collect()
}

fn search_prefix(
    db: &SyncDatabase,
    query: &str,
    _machine_id: Option<&str>,
    session_id: Option<&str>,
    cwd: Option<&str>,
    filter: FilterMode,
    filter_value: &str,
    limit: i64,
) -> Vec<HistoryEntry> {
    if query.is_empty() {
        return db
            .search_history("", None, session_id, cwd, limit, 0)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| apply_host_filter(e, filter, filter_value))
            .collect();
    }

    // search_history uses LIKE '%query%', but for prefix we want LIKE 'query%'
    // We'll fetch broadly and filter in post for now, since we can't change the DB method.
    let broad_limit = limit * 10;
    let results = db
        .search_history("", None, session_id, cwd, broad_limit, 0)
        .unwrap_or_default();

    results
        .into_iter()
        .filter(|e| apply_host_filter(e, filter, filter_value))
        .filter(|e| e.command.starts_with(query))
        .take(limit as usize)
        .collect()
}

fn search_fulltext(
    db: &SyncDatabase,
    query: &str,
    _machine_id: Option<&str>,
    session_id: Option<&str>,
    cwd: Option<&str>,
    filter: FilterMode,
    filter_value: &str,
    limit: i64,
) -> Vec<HistoryEntry> {
    // search_history already does LIKE '%query%' which is fulltext
    db.search_history(query, None, session_id, cwd, limit, 0)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| apply_host_filter(e, filter, filter_value))
        .collect()
}

fn search_regex(
    db: &SyncDatabase,
    query: &str,
    _machine_id: Option<&str>,
    session_id: Option<&str>,
    cwd: Option<&str>,
    filter: FilterMode,
    filter_value: &str,
    limit: i64,
) -> Vec<HistoryEntry> {
    let re = match regex::Regex::new(query) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let broad_limit = limit * 10;
    let results = db
        .search_history("", None, session_id, cwd, broad_limit, 0)
        .unwrap_or_default();

    results
        .into_iter()
        .filter(|e| apply_host_filter(e, filter, filter_value))
        .filter(|e| re.is_match(&e.command))
        .take(limit as usize)
        .collect()
}

fn apply_host_filter(entry: &HistoryEntry, filter: FilterMode, filter_value: &str) -> bool {
    match filter {
        FilterMode::Host if !filter_value.is_empty() => entry.hostname == filter_value,
        _ => true,
    }
}
