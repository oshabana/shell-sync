use std::sync::Arc;

use shell_sync_core::config::{socket_path, ClientConfig};
use shell_sync_core::db::SyncDatabase;
use shell_sync_core::models::{HistoryEntry, HistoryHookPayload};
use tokio::io::AsyncBufReadExt;
use tokio::net::UnixListener;
use tracing::{error, info, warn};

/// Start the Unix domain socket listener that receives history hook payloads.
pub async fn start_socket_listener(
    db: Arc<SyncDatabase>,
    config: &ClientConfig,
) -> anyhow::Result<()> {
    let sock_path = socket_path();

    // Clean up stale socket
    if sock_path.exists() {
        let _ = std::fs::remove_file(&sock_path);
    }

    // Ensure parent directory exists
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(&sock_path)?;

    // Set socket permissions to 0o600 (owner only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&sock_path, perms)?;
    }

    info!(path = %sock_path.display(), "Socket listener started");

    let machine_id = config.machine_id.clone();
    let hostname = config.hostname.clone();
    let group_name = config.groups.first().cloned().unwrap_or_else(|| "default".to_string());

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let db = db.clone();
                let machine_id = machine_id.clone();
                let hostname = hostname.clone();
                let group_name = group_name.clone();

                tokio::spawn(async move {
                    let reader = tokio::io::BufReader::new(stream);
                    let mut lines = reader.lines();

                    while let Ok(Some(line)) = lines.next_line().await {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<HistoryHookPayload>(&line) {
                            Ok(payload) => {
                                let entry = HistoryEntry {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    command: payload.command,
                                    cwd: payload.cwd,
                                    exit_code: payload.exit_code,
                                    duration_ms: payload.duration_ms,
                                    session_id: payload.session_id,
                                    machine_id: machine_id.clone(),
                                    hostname: hostname.clone(),
                                    timestamp: chrono::Utc::now().timestamp_millis(),
                                    shell: payload.shell,
                                    group_name: group_name.clone(),
                                };

                                if let Err(e) = db.insert_history_entry(&entry) {
                                    error!("Failed to insert history entry: {e}");
                                }
                                if let Err(e) = db.add_history_pending(&entry) {
                                    error!("Failed to queue pending history: {e}");
                                }
                            }
                            Err(e) => {
                                warn!("Invalid hook payload: {e}");
                            }
                        }
                    }
                });
            }
            Err(e) => {
                error!("Socket accept error: {e}");
            }
        }
    }
}
