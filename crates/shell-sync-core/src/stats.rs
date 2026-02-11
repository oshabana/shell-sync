use crate::db::SyncDatabase;
use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResult {
    pub total_commands: i64,
    pub unique_commands: i64,
    pub success_rate: f64,
    pub top_commands: Vec<(String, i64)>,
    pub top_prefixes: Vec<(String, i64)>,
    pub avg_duration_ms: f64,
    pub median_duration_ms: i64,
    pub p95_duration_ms: i64,
    pub hourly_distribution: Vec<i64>,
    pub daily_distribution: Vec<i64>,
    pub per_directory: Vec<(String, i64)>,
    pub per_machine: Vec<(String, i64)>,
    pub streak_days: i64,
}

#[derive(Debug, Clone)]
pub struct StatsFilter {
    pub after_timestamp: Option<i64>,
    pub machine_id: Option<String>,
    pub group_name: Option<String>,
    pub directory: Option<String>,
}

/// Compute shell usage statistics from the local history database.
pub fn compute_stats(db: &SyncDatabase, filter: &StatsFilter) -> anyhow::Result<StatsResult> {
    let conn = db.raw_connection();
    let conn = conn.lock().unwrap();

    // Build WHERE clause
    let mut conditions = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(after) = filter.after_timestamp {
        conditions.push(format!("timestamp >= ?{idx}"));
        param_values.push(Box::new(after));
        idx += 1;
    }
    if let Some(ref mid) = filter.machine_id {
        conditions.push(format!("machine_id = ?{idx}"));
        param_values.push(Box::new(mid.clone()));
        idx += 1;
    }
    if let Some(ref group) = filter.group_name {
        conditions.push(format!("group_name = ?{idx}"));
        param_values.push(Box::new(group.clone()));
        idx += 1;
    }
    if let Some(ref dir) = filter.directory {
        conditions.push(format!("cwd = ?{idx}"));
        param_values.push(Box::new(dir.clone()));
        // idx not needed after last use
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    // Total commands
    let total_commands: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM history {where_clause}"),
            params_ref.as_slice(),
            |row| row.get(0),
        )
        .unwrap_or(0);

    if total_commands == 0 {
        return Ok(StatsResult {
            total_commands: 0,
            unique_commands: 0,
            success_rate: 0.0,
            top_commands: vec![],
            top_prefixes: vec![],
            avg_duration_ms: 0.0,
            median_duration_ms: 0,
            p95_duration_ms: 0,
            hourly_distribution: vec![0; 24],
            daily_distribution: vec![0; 7],
            per_directory: vec![],
            per_machine: vec![],
            streak_days: 0,
        });
    }

    // Unique commands
    let unique_commands: i64 = conn
        .query_row(
            &format!("SELECT COUNT(DISTINCT command) FROM history {where_clause}"),
            params_ref.as_slice(),
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Success rate
    let success_count: i64 = conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM history {where_clause} {} exit_code = 0",
                if conditions.is_empty() {
                    "WHERE"
                } else {
                    "AND"
                }
            ),
            params_ref.as_slice(),
            |row| row.get(0),
        )
        .unwrap_or(0);
    let success_rate = if total_commands > 0 {
        (success_count as f64 / total_commands as f64) * 100.0
    } else {
        0.0
    };

    // Top 10 commands (full command string)
    let top_commands = {
        let sql = format!(
            "SELECT command, COUNT(*) as cnt FROM history {where_clause} GROUP BY command ORDER BY cnt DESC LIMIT 10"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    // Top 10 prefixes (first word of command)
    let top_prefixes = {
        let sql = format!("SELECT command FROM history {where_clause}");
        let mut stmt = conn.prepare(&sql)?;
        let mut prefix_counts: HashMap<String, i64> = HashMap::new();
        let mut rows = stmt.query(params_ref.as_slice())?;
        while let Some(row) = rows.next()? {
            let cmd: String = row.get(0)?;
            let prefix = cmd.split_whitespace().next().unwrap_or("").to_string();
            if !prefix.is_empty() {
                *prefix_counts.entry(prefix).or_insert(0) += 1;
            }
        }
        let mut sorted: Vec<(String, i64)> = prefix_counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(10);
        sorted
    };

    // Duration stats
    let avg_duration_ms: f64 = conn
        .query_row(
            &format!("SELECT AVG(duration_ms) FROM history {where_clause}"),
            params_ref.as_slice(),
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    // Collect all durations for median and p95
    let (median_duration_ms, p95_duration_ms) = {
        let sql =
            format!("SELECT duration_ms FROM history {where_clause} ORDER BY duration_ms ASC");
        let mut stmt = conn.prepare(&sql)?;
        let durations: Vec<i64> = stmt
            .query_map(params_ref.as_slice(), |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        if durations.is_empty() {
            (0i64, 0i64)
        } else {
            let median = durations[durations.len() / 2];
            let p95_idx = ((durations.len() as f64) * 0.95).ceil() as usize;
            let p95 = durations[p95_idx.min(durations.len() - 1)];
            (median, p95)
        }
    };

    // Hourly distribution (24 buckets)
    let hourly_distribution = {
        let sql = format!("SELECT timestamp FROM history {where_clause}");
        let mut stmt = conn.prepare(&sql)?;
        let mut hours = vec![0i64; 24];
        let mut rows = stmt.query(params_ref.as_slice())?;
        while let Some(row) = rows.next()? {
            let ts: i64 = row.get(0)?;
            if let Some(dt) = chrono::DateTime::from_timestamp_millis(ts) {
                let hour = dt.time().hour() as usize;
                hours[hour] += 1;
            }
        }
        hours
    };

    // Daily distribution (7 buckets, Mon=0 .. Sun=6)
    let daily_distribution = {
        let sql = format!("SELECT timestamp FROM history {where_clause}");
        let mut stmt = conn.prepare(&sql)?;
        let mut days = vec![0i64; 7];
        let mut rows = stmt.query(params_ref.as_slice())?;
        while let Some(row) = rows.next()? {
            let ts: i64 = row.get(0)?;
            if let Some(dt) = chrono::DateTime::from_timestamp_millis(ts) {
                let day = dt.weekday().num_days_from_monday() as usize;
                days[day] += 1;
            }
        }
        days
    };

    // Per directory (top 10)
    let per_directory = {
        let sql = format!(
            "SELECT cwd, COUNT(*) as cnt FROM history {where_clause} GROUP BY cwd ORDER BY cnt DESC LIMIT 10"
        );
        let mut stmt = conn.prepare(&sql)?;
        let result = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        result
    };

    // Per machine
    let per_machine = {
        let sql = format!(
            "SELECT hostname, COUNT(*) as cnt FROM history {where_clause} GROUP BY hostname ORDER BY cnt DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let result = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        result
    };

    // Streak days — consecutive days with at least one command (counting back from today)
    let streak_days = {
        let sql = format!(
            "SELECT DISTINCT date(timestamp / 1000, 'unixepoch') as d FROM history {where_clause} ORDER BY d DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let dates: Vec<String> = stmt
            .query_map(params_ref.as_slice(), |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        if dates.is_empty() {
            0
        } else {
            let mut streak = 1i64;
            for i in 1..dates.len() {
                let prev = chrono::NaiveDate::parse_from_str(&dates[i - 1], "%Y-%m-%d");
                let curr = chrono::NaiveDate::parse_from_str(&dates[i], "%Y-%m-%d");
                if let (Ok(p), Ok(c)) = (prev, curr) {
                    if p - c == chrono::Duration::days(1) {
                        streak += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            streak
        }
    };

    Ok(StatsResult {
        total_commands,
        unique_commands,
        success_rate,
        top_commands,
        top_prefixes,
        avg_duration_ms,
        median_duration_ms,
        p95_duration_ms,
        hourly_distribution,
        daily_distribution,
        per_directory,
        per_machine,
        streak_days,
    })
}

/// Parse a human-readable duration string into a Unix timestamp threshold (in ms).
/// Supports: "7d", "30d", "1y", "all"
pub fn parse_last_filter(last: &str) -> Option<i64> {
    let last = last.trim().to_lowercase();
    if last == "all" {
        return None;
    }

    let (num, unit) = last.split_at(last.len().saturating_sub(1));
    let num: i64 = num.parse().ok()?;

    let seconds = match unit {
        "d" => num * 86400,
        "w" => num * 7 * 86400,
        "m" => num * 30 * 86400,
        "y" => num * 365 * 86400,
        _ => return None,
    };

    let now = chrono::Utc::now().timestamp_millis();
    Some(now - seconds * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_last_7d() {
        let ts = parse_last_filter("7d").unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let diff = now - ts;
        // Should be approximately 7 days in ms
        assert!((diff - 7 * 86400 * 1000).abs() < 1000);
    }

    #[test]
    fn parse_last_30d() {
        let ts = parse_last_filter("30d").unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let diff = now - ts;
        assert!((diff - 30 * 86400 * 1000).abs() < 1000);
    }

    #[test]
    fn parse_last_1y() {
        let ts = parse_last_filter("1y").unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let diff = now - ts;
        assert!((diff - 365 * 86400 * 1000).abs() < 1000);
    }

    #[test]
    fn parse_last_all() {
        assert!(parse_last_filter("all").is_none());
    }

    #[test]
    fn parse_last_invalid() {
        assert!(parse_last_filter("foo").is_none());
    }
}
