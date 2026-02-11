use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use shell_sync_core::config::{load_client_config, pid_file_path, ClientConfig};
use tokio::sync::Notify;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

/// Run the client sync daemon.
pub async fn run(server_override: Option<String>, foreground: bool) -> anyhow::Result<()> {
    let config = load_client_config()?;

    let config = if let Some(url) = server_override {
        ClientConfig {
            server_url: url,
            ..config
        }
    } else {
        config
    };

    if !foreground {
        // TODO: daemonize (fork + detach). For now, always run in foreground.
        info!("Running in foreground mode");
    }

    // Write PID file
    let pid_path = pid_file_path();
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pid_path, std::process::id().to_string())?;

    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = shutdown.clone();

    // Handle SIGINT/SIGTERM
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Received shutdown signal");
        shutdown_clone.notify_one();
    });

    println!("Shell Sync daemon started");
    println!("Server: {}", config.server_url);
    println!("Groups: {}", config.groups.join(", "));

    // Main reconnect loop
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        tokio::select! {
            _ = shutdown.notified() => {
                break;
            }
            result = connect_and_run(&config) => {
                match result {
                    Ok(()) => {
                        info!("Connection closed cleanly");
                        backoff = Duration::from_secs(1);
                    }
                    Err(e) => {
                        warn!("Connection error: {e}");
                    }
                }

                // Check if shutdown was requested during connection
                if Arc::strong_count(&shutdown) <= 1 {
                    break;
                }

                info!(backoff_secs = backoff.as_secs(), "Reconnecting...");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
        }
    }

    // Cleanup
    let _ = std::fs::remove_file(&pid_path);
    info!("Daemon stopped");

    Ok(())
}

async fn connect_and_run(config: &ClientConfig) -> anyhow::Result<()> {
    let ws_url = config
        .server_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    let ws_url = format!("{}/ws", ws_url);

    info!(url = %ws_url, "Connecting...");

    let (ws_stream, _) = connect_async(&ws_url).await?;
    let (mut tx, mut rx) = ws_stream.split();

    info!("Connected to sync service");

    // Send auth
    let auth_msg = serde_json::json!({
        "type": "auth",
        "token": config.auth_token
    });
    tx.send(Message::Text(auth_msg.to_string().into())).await?;

    // Ping interval
    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
    ping_interval.tick().await; // Skip first immediate tick

    loop {
        tokio::select! {
            msg = rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_message(config, &text).await;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("WebSocket closed");
                        return Ok(());
                    }
                    Some(Err(e)) => {
                        return Err(e.into());
                    }
                    _ => {}
                }
            }
            _ = ping_interval.tick() => {
                let ping = serde_json::json!({ "type": "ping" });
                if tx.send(Message::Text(ping.to_string().into())).await.is_err() {
                    return Ok(());
                }
            }
        }
    }
}

async fn handle_message(config: &ClientConfig, text: &str) {
    let parsed: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    let event = parsed.get("event").and_then(|v| v.as_str()).unwrap_or("");

    match event {
        "auth_success" => {
            info!(machine_id = %config.machine_id, "Authenticated");
            sync_aliases(config).await;
        }
        "auth_failed" => {
            error!("Authentication failed — check your config");
        }
        "alias_added" | "alias_updated" | "alias_deleted" | "sync_required" => {
            let name = parsed
                .get("data")
                .and_then(|d| d.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("(unknown)");
            info!(event, name, "Sync event received");
            sync_aliases(config).await;
        }
        "pong" => {}
        _ => {
            warn!(event, "Unknown event");
        }
    }
}

async fn sync_aliases(config: &ClientConfig) {
    match fetch_and_apply_aliases(config).await {
        Ok(count) => info!(count, "Aliases synced"),
        Err(e) => {
            error!("Failed to sync aliases: {e}");
            // Queue for offline sync
            if let Err(qe) = crate::offline::queue_sync_request() {
                error!("Failed to queue offline sync: {qe}");
            }
        }
    }
}

async fn fetch_and_apply_aliases(config: &ClientConfig) -> anyhow::Result<usize> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/aliases", config.server_url))
        .header("Authorization", format!("Bearer {}", config.auth_token))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }

    let data: serde_json::Value = resp.json().await?;
    let aliases: Vec<shell_sync_core::models::Alias> =
        serde_json::from_value(data["aliases"].clone()).unwrap_or_default();

    let count = aliases.len();
    crate::shell_writer::apply_aliases(&aliases)?;

    Ok(count)
}
