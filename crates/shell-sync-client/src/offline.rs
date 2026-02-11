use rusqlite::{params, Connection};
use shell_sync_core::config::offline_queue_db_path;
use tracing::info;

/// Initialize the offline queue database.
fn open_queue_db() -> anyhow::Result<Connection> {
    let path = offline_queue_db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            action TEXT NOT NULL,
            payload TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )"
    )?;
    Ok(conn)
}

/// Queue an operation for later sync.
pub fn queue_operation(action: &str, payload: &serde_json::Value) -> anyhow::Result<()> {
    let conn = open_queue_db()?;
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "INSERT INTO queue (action, payload, created_at) VALUES (?1, ?2, ?3)",
        params![action, payload.to_string(), now],
    )?;
    info!(action, "Queued offline operation");
    Ok(())
}

/// Queue a full sync request.
pub fn queue_sync_request() -> anyhow::Result<()> {
    queue_operation("sync", &serde_json::json!({}))
}

/// Flush the offline queue by replaying operations against the server.
pub async fn flush_queue(server_url: &str, auth_token: &str) -> anyhow::Result<usize> {
    let conn = open_queue_db()?;
    let mut stmt = conn.prepare("SELECT id, action, payload FROM queue ORDER BY id")?;
    let rows: Vec<(i64, String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;

    if rows.is_empty() {
        return Ok(0);
    }

    let client = reqwest::Client::new();
    let mut flushed = 0;

    for (id, action, payload) in &rows {
        let result = match action.as_str() {
            "add" => {
                let payload: serde_json::Value = serde_json::from_str(payload)?;
                client
                    .post(format!("{}/api/aliases", server_url))
                    .header("Authorization", format!("Bearer {}", auth_token))
                    .json(&payload)
                    .send()
                    .await
            }
            "delete" => {
                let payload: serde_json::Value = serde_json::from_str(payload)?;
                let name = payload["name"].as_str().unwrap_or("");
                let group = payload["group"].as_str().unwrap_or("default");
                client
                    .delete(format!("{}/api/aliases/name/{}?group={}", server_url, name, group))
                    .header("Authorization", format!("Bearer {}", auth_token))
                    .send()
                    .await
            }
            "sync" => {
                // Full sync is handled by the daemon on reconnect
                Ok(reqwest::Response::from(
                    http::Response::builder()
                        .status(200)
                        .body("")
                        .unwrap(),
                ))
            }
            _ => continue,
        };

        match result {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 409 => {
                conn.execute("DELETE FROM queue WHERE id = ?1", params![id])?;
                flushed += 1;
            }
            Ok(resp) => {
                tracing::warn!(
                    action,
                    status = resp.status().as_u16(),
                    "Failed to flush queued operation, will retry"
                );
                break; // Stop on first failure to preserve order
            }
            Err(e) => {
                tracing::warn!(action, error = %e, "Failed to flush queued operation");
                break;
            }
        }
    }

    if flushed > 0 {
        info!(flushed, "Flushed offline queue");
    }

    Ok(flushed)
}

/// Get the number of pending operations in the queue.
pub fn pending_count() -> anyhow::Result<usize> {
    let conn = open_queue_db()?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM queue", [], |row| row.get(0))?;
    Ok(count as usize)
}
