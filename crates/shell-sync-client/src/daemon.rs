use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use shell_sync_core::config::{history_db_path, keys_dir_path, load_client_config, pid_file_path, ClientConfig};
use shell_sync_core::db::SyncDatabase;
use shell_sync_core::encryption::{self, KeyManager};
use shell_sync_core::models::HistoryEntry;
use tokio::sync::{mpsc, Mutex, Notify};
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

    // Open history database
    let db_path = history_db_path();
    let db = Arc::new(SyncDatabase::open(db_path.to_str().unwrap_or("history.db"))?);
    info!(path = %db_path.display(), "History database opened");

    // Init encryption key manager
    let keys_dir = keys_dir_path();
    let key_mgr = match KeyManager::new(keys_dir.clone()) {
        Ok(mgr) => {
            info!(path = %keys_dir.display(), "Encryption key manager initialized");
            Arc::new(Mutex::new(mgr))
        }
        Err(e) => {
            warn!("Failed to init encryption: {e} — running without encryption");
            Arc::new(Mutex::new(KeyManager::new(keys_dir)?))
        }
    };

    // Spawn socket listener for shell hooks
    let listener_db = db.clone();
    let listener_config = config.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::socket_listener::start_socket_listener(listener_db, &listener_config).await {
            error!("Socket listener error: {e}");
        }
    });

    // Spawn local stats proxy (127.0.0.1:18888)
    let proxy_db = db.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::stats_proxy::start_stats_proxy(proxy_db).await {
            error!("Stats proxy error: {e}");
        }
    });

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
            result = connect_and_run(&config, &db, &key_mgr) => {
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
    let sock = shell_sync_core::config::socket_path();
    let _ = std::fs::remove_file(&sock);
    info!("Daemon stopped");

    Ok(())
}

async fn connect_and_run(
    config: &ClientConfig,
    db: &Arc<SyncDatabase>,
    key_mgr: &Arc<Mutex<KeyManager>>,
) -> anyhow::Result<()> {
    let ws_url = config
        .server_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    let ws_url = format!("{}/ws", ws_url);

    info!(url = %ws_url, "Connecting...");

    let (ws_stream, _) = connect_async(&ws_url).await?;
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    info!("Connected to sync service");

    // Create outbound channel so multiple tasks can send messages
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<String>();

    // Send auth
    let auth_msg = serde_json::json!({
        "type": "auth",
        "token": config.auth_token
    });
    outbound_tx.send(auth_msg.to_string())?;

    // Spawn task to forward outbound channel to WebSocket
    let forward_task = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Spawn history push loop
    let push_db = db.clone();
    let push_tx = outbound_tx.clone();
    let push_km = key_mgr.clone();
    let push_task = tokio::spawn(async move {
        history_push_loop(&push_db, &push_tx, &push_km, 5).await;
    });

    // Ping interval
    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
    ping_interval.tick().await; // Skip first immediate tick

    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        handle_message(config, db, key_mgr, &outbound_tx, &text).await;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("WebSocket closed");
                        break;
                    }
                    Some(Err(e)) => {
                        push_task.abort();
                        forward_task.abort();
                        return Err(e.into());
                    }
                    _ => {}
                }
            }
            _ = ping_interval.tick() => {
                let ping = serde_json::json!({ "type": "ping" });
                if outbound_tx.send(ping.to_string()).is_err() {
                    break;
                }
            }
        }
    }

    push_task.abort();
    forward_task.abort();
    Ok(())
}

/// Periodically push pending history entries to the server.
/// If a group key is available, entries are encrypted before sending.
async fn history_push_loop(
    db: &SyncDatabase,
    tx: &mpsc::UnboundedSender<String>,
    key_mgr: &Arc<Mutex<KeyManager>>,
    interval_secs: u64,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    interval.tick().await; // Skip first immediate tick

    loop {
        interval.tick().await;

        let entries = match db.get_pending_history(50) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entries.is_empty() {
            continue;
        }

        let ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();

        // Try to encrypt entries if group keys are available
        let km = key_mgr.lock().await;
        let mut encrypted_entries = Vec::new();
        let mut plaintext_entries = Vec::new();

        for entry in &entries {
            if let Some(key) = km.get_group_key(&entry.group_name) {
                match encryption::encrypt_history_entry(key, entry) {
                    Ok(enc) => encrypted_entries.push(serde_json::to_value(&enc).unwrap()),
                    Err(e) => {
                        warn!(group = %entry.group_name, "Encrypt failed, sending plaintext: {e}");
                        plaintext_entries.push(serde_json::to_value(entry).unwrap());
                    }
                }
            } else {
                plaintext_entries.push(serde_json::to_value(entry).unwrap());
            }
        }
        drop(km);

        // Send encrypted entries
        if !encrypted_entries.is_empty() {
            let msg = serde_json::json!({
                "type": "history_batch",
                "entries": encrypted_entries,
                "encrypted": true,
            });
            let _ = tx.send(msg.to_string());
        }

        // Send plaintext entries (for groups without keys)
        if !plaintext_entries.is_empty() {
            let msg = serde_json::json!({
                "type": "history_batch",
                "entries": plaintext_entries,
            });
            let _ = tx.send(msg.to_string());
        }

        if let Err(e) = db.remove_pending_history(&ids) {
            error!("Failed to remove pending history: {e}");
        } else {
            info!(count = ids.len(), "Pushed history batch");
        }
    }
}

async fn handle_message(
    config: &ClientConfig,
    db: &SyncDatabase,
    key_mgr: &Arc<Mutex<KeyManager>>,
    outbound_tx: &mpsc::UnboundedSender<String>,
    text: &str,
) {
    let parsed: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    let event = parsed.get("event").and_then(|v| v.as_str()).unwrap_or("");

    match event {
        "auth_success" => {
            info!(machine_id = %config.machine_id, "Authenticated");

            // Request missing group keys on connect
            request_missing_keys(config, key_mgr, outbound_tx).await;

            sync_aliases(config, key_mgr).await;
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
            sync_aliases(config, key_mgr).await;
        }
        "history_sync" => {
            if let Some(data) = parsed.get("data") {
                let is_encrypted = data.get("encrypted").and_then(|v| v.as_bool()).unwrap_or(false);

                if is_encrypted {
                    // Decrypt entries before storing
                    let enc_entries: Vec<shell_sync_core::models::EncryptedHistoryEntry> =
                        serde_json::from_value(data["entries"].clone()).unwrap_or_default();
                    if !enc_entries.is_empty() {
                        let km = key_mgr.lock().await;
                        let mut decrypted = Vec::new();
                        for enc in &enc_entries {
                            if let Some(key) = km.get_group_key(&enc.group_name) {
                                match encryption::decrypt_history_entry(key, enc) {
                                    Ok(entry) => decrypted.push(entry),
                                    Err(e) => warn!("Failed to decrypt history entry: {e}"),
                                }
                            } else {
                                warn!(group = %enc.group_name, "No key to decrypt history entry");
                            }
                        }
                        drop(km);

                        if !decrypted.is_empty() {
                            let count = db.insert_history_batch(&decrypted);
                            let source = data["source_machine_id"].as_str().unwrap_or("unknown");
                            info!(count, source, "Received encrypted history sync");
                        }
                    }
                } else {
                    // Plaintext entries (legacy/unencrypted groups)
                    let entries: Vec<HistoryEntry> =
                        serde_json::from_value(data["entries"].clone()).unwrap_or_default();
                    if !entries.is_empty() {
                        let count = db.insert_history_batch(&entries);
                        let source = data["source_machine_id"].as_str().unwrap_or("unknown");
                        info!(count, source, "Received history sync");
                    }
                }
            }
        }
        "key_request" => {
            // Another machine is requesting a group key
            if let Some(data) = parsed.get("data") {
                let group = data["group_name"].as_str().unwrap_or("");
                let requester_id = data["requester_machine_id"].as_str().unwrap_or("");
                let requester_pubkey = data["requester_public_key"].as_str().unwrap_or("");

                if group.is_empty() || requester_pubkey.is_empty() {
                    return;
                }

                let km = key_mgr.lock().await;
                if km.has_group_key(group) {
                    match km.wrap_group_key(group, requester_pubkey) {
                        Ok(wrapped) => {
                            let resp = serde_json::json!({
                                "type": "key_response",
                                "target_machine_id": requester_id,
                                "group_name": group,
                                "wrapped_key": wrapped,
                                "sender_public_key": km.public_key_b64(),
                            });
                            let _ = outbound_tx.send(resp.to_string());
                            info!(group, requester = requester_id, "Sent group key");
                        }
                        Err(e) => warn!("Failed to wrap group key: {e}"),
                    }
                }
            }
        }
        "key_response" => {
            // Received a group key from another machine
            if let Some(data) = parsed.get("data") {
                let group = data["group_name"].as_str().unwrap_or("");
                let wrapped = data["wrapped_key"].as_str().unwrap_or("");
                let sender_pubkey = data["sender_public_key"].as_str().unwrap_or("");

                if group.is_empty() || wrapped.is_empty() || sender_pubkey.is_empty() {
                    return;
                }

                let mut km = key_mgr.lock().await;
                match km.unwrap_group_key(group, wrapped, sender_pubkey) {
                    Ok(()) => info!(group, "Received and stored group key"),
                    Err(e) => warn!(group, "Failed to unwrap group key: {e}"),
                }
            }
        }
        "pong" => {}
        _ => {
            warn!(event, "Unknown event");
        }
    }
}

/// Request group keys for any groups we're missing keys for.
async fn request_missing_keys(
    config: &ClientConfig,
    key_mgr: &Arc<Mutex<KeyManager>>,
    outbound_tx: &mpsc::UnboundedSender<String>,
) {
    let km = key_mgr.lock().await;
    for group in &config.groups {
        if !km.has_group_key(group) {
            let msg = serde_json::json!({
                "type": "key_request",
                "group_name": group,
                "requester_machine_id": config.machine_id,
                "requester_public_key": km.public_key_b64(),
            });
            let _ = outbound_tx.send(msg.to_string());
            info!(group, "Requested group key");
        }
    }
}

async fn sync_aliases(config: &ClientConfig, key_mgr: &Arc<Mutex<KeyManager>>) {
    match fetch_and_apply_aliases(config, key_mgr).await {
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

async fn fetch_and_apply_aliases(
    config: &ClientConfig,
    key_mgr: &Arc<Mutex<KeyManager>>,
) -> anyhow::Result<usize> {
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
    let is_encrypted = data.get("encrypted").and_then(|v| v.as_bool()).unwrap_or(false);

    let aliases: Vec<shell_sync_core::models::Alias> = if is_encrypted {
        // Server returned encrypted aliases — decrypt them
        let enc_aliases: Vec<shell_sync_core::models::EncryptedAlias> =
            serde_json::from_value(data["aliases"].clone()).unwrap_or_default();
        let km = key_mgr.lock().await;
        let mut decrypted = Vec::new();
        for enc in &enc_aliases {
            if let Some(key) = km.get_group_key(&enc.group_name) {
                match encryption::decrypt_alias(key, enc) {
                    Ok(alias) => decrypted.push(alias),
                    Err(e) => warn!(name = %enc.name, "Failed to decrypt alias: {e}"),
                }
            } else {
                warn!(group = %enc.group_name, "No key to decrypt alias '{}'", enc.name);
            }
        }
        decrypted
    } else {
        serde_json::from_value(data["aliases"].clone()).unwrap_or_default()
    };

    let count = aliases.len();
    crate::shell_writer::apply_aliases(&aliases)?;

    Ok(count)
}
