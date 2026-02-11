use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use shell_sync_core::db::SyncDatabase;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

/// A connected WebSocket client.
struct WsClient {
    tx: mpsc::UnboundedSender<String>,
}

/// Hub managing all WebSocket connections, keyed by machine_id.
pub struct WsHub {
    clients: RwLock<HashMap<String, WsClient>>,
}

impl WsHub {
    pub fn new() -> Self {
        Self {
            clients: RwLock::new(HashMap::new()),
        }
    }

    /// Register an authenticated client.
    async fn add_client(&self, machine_id: String, tx: mpsc::UnboundedSender<String>) {
        self.clients
            .write()
            .await
            .insert(machine_id, WsClient { tx });
    }

    /// Remove a client on disconnect.
    async fn remove_client(&self, machine_id: &str) {
        self.clients.write().await.remove(machine_id);
    }

    /// Number of connected clients.
    pub async fn client_count(&self) -> usize {
        self.clients.read().await.len()
    }

    /// Broadcast an event to all machines in the given groups, excluding one machine.
    pub async fn broadcast_to_groups(
        &self,
        db: &SyncDatabase,
        groups: &[String],
        event: &str,
        data: serde_json::Value,
        exclude_machine_id: Option<&str>,
    ) {
        let mut target_ids = std::collections::HashSet::new();

        for group in groups {
            if let Ok(machines) = db.get_machines_by_group(group) {
                for m in machines {
                    if exclude_machine_id.is_some_and(|id| id == m.machine_id) {
                        continue;
                    }
                    target_ids.insert(m.machine_id);
                }
            }
        }

        let msg = serde_json::json!({ "event": event, "data": data }).to_string();
        let clients = self.clients.read().await;
        let mut sent = 0;

        for machine_id in &target_ids {
            if let Some(client) = clients.get(machine_id) {
                if client.tx.send(msg.clone()).is_ok() {
                    sent += 1;
                }
            }
        }

        info!(
            event,
            sent,
            groups = ?groups,
            "Broadcast to clients"
        );
    }

    /// Send a message to a specific machine by its machine_id.
    pub async fn send_to_machine(&self, machine_id: &str, msg: &str) -> bool {
        let clients = self.clients.read().await;
        if let Some(client) = clients.get(machine_id) {
            client.tx.send(msg.to_string()).is_ok()
        } else {
            false
        }
    }
}

/// Handle a single WebSocket connection through the auth flow and message loop.
pub async fn handle_ws(socket: WebSocket, db: Arc<SyncDatabase>, hub: Arc<WsHub>) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut machine_id: Option<String> = None;
    let mut machine_groups: Vec<String> = Vec::new();

    // Create a channel for outbound messages
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Spawn a task to forward channel messages to the WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Process incoming messages
    while let Some(Ok(msg)) = ws_rx.next().await {
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };

        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&text);
        let data = match parsed {
            Ok(d) => d,
            Err(_) => continue,
        };

        let msg_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match msg_type {
            "auth" => {
                let token = data.get("token").and_then(|v| v.as_str()).unwrap_or("");
                match db.get_machine_by_token(token) {
                    Ok(Some(m)) => {
                        let mid = m.machine_id.clone();
                        let _ = db.update_machine_last_seen(&mid);
                        hub.add_client(mid.clone(), tx.clone()).await;
                        machine_id = Some(mid.clone());
                        machine_groups = m.groups.clone();

                        let resp = serde_json::json!({
                            "event": "auth_success",
                            "data": { "machine_id": mid, "groups": m.groups }
                        });
                        let _ = tx.send(resp.to_string());
                        info!(machine_id = %mid, hostname = %m.hostname, "WS authenticated");
                    }
                    _ => {
                        let resp = serde_json::json!({
                            "event": "auth_failed",
                            "data": { "error": "Invalid token" }
                        });
                        let _ = tx.send(resp.to_string());
                        break;
                    }
                }
            }
            "ping" => {
                let resp = serde_json::json!({
                    "event": "pong",
                    "data": { "timestamp": chrono::Utc::now().timestamp_millis() }
                });
                let _ = tx.send(resp.to_string());
            }
            "key_request" => {
                if let Some(ref mid) = machine_id {
                    let group_name = data
                        .get("group_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let public_key = data
                        .get("public_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    if !group_name.is_empty() && !public_key.is_empty() {
                        // Look up the sender's groups to verify membership
                        if let Ok(Some(sender)) = db.get_machine_by_id(mid) {
                            if sender.groups.contains(&group_name.to_string()) {
                                let event = serde_json::json!({
                                    "event": "key_request",
                                    "data": {
                                        "group_name": group_name,
                                        "requester_machine_id": mid,
                                        "public_key": public_key,
                                    }
                                });
                                let event_str = event.to_string();

                                // Broadcast to other group members
                                if let Ok(machines) = db.get_machines_by_group(group_name) {
                                    for m in machines {
                                        if m.machine_id != *mid {
                                            hub.send_to_machine(&m.machine_id, &event_str).await;
                                        }
                                    }
                                }
                                info!(machine_id = %mid, group = %group_name, "Key request broadcast");
                            }
                        }
                    }
                }
            }
            "key_response" => {
                if let Some(ref mid) = machine_id {
                    let group_name = data
                        .get("group_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let target_machine_id = data
                        .get("target_machine_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let wrapped_key = data
                        .get("wrapped_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    if !target_machine_id.is_empty() && !wrapped_key.is_empty() {
                        // Get the sender's public key to include in the response
                        let sender_public_key = if let Ok(Some(sender)) = db.get_machine_by_id(mid)
                        {
                            sender.public_key.unwrap_or_default()
                        } else {
                            String::new()
                        };

                        let event = serde_json::json!({
                            "event": "key_response",
                            "data": {
                                "group_name": group_name,
                                "wrapped_key": wrapped_key,
                                "sender_public_key": sender_public_key,
                            }
                        });
                        let sent = hub
                            .send_to_machine(target_machine_id, &event.to_string())
                            .await;
                        info!(
                            from = %mid,
                            to = %target_machine_id,
                            group = %group_name,
                            delivered = sent,
                            "Key response relayed"
                        );
                    }
                }
            }
            "history_batch" => {
                if let Some(ref mid) = machine_id {
                    let entries: Vec<shell_sync_core::models::HistoryEntry> =
                        serde_json::from_value(data["entries"].clone()).unwrap_or_default();
                    if !entries.is_empty() {
                        let count = db.insert_history_batch(&entries);
                        info!(machine_id = %mid, count, "History batch received");

                        // Broadcast to group members
                        if !machine_groups.is_empty() {
                            hub.broadcast_to_groups(
                                &db,
                                &machine_groups,
                                "history_sync",
                                serde_json::json!({
                                    "entries": entries,
                                    "source_machine_id": mid,
                                }),
                                Some(mid),
                            )
                            .await;
                        }
                    }
                }
            }
            "history_query" => {
                if let Some(ref _mid) = machine_id {
                    let after_timestamp = data
                        .get("after_timestamp")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let group_name = data
                        .get("group_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("default");
                    let limit = data
                        .get("limit")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(100)
                        .min(1000);

                    match db.get_history_after_timestamp(after_timestamp, group_name, limit) {
                        Ok(entries) => {
                            let has_more = entries.len() as i64 == limit;
                            let resp = serde_json::json!({
                                "event": "history_page",
                                "data": {
                                    "entries": entries,
                                    "has_more": has_more,
                                }
                            });
                            let _ = tx.send(resp.to_string());
                        }
                        Err(e) => {
                            warn!("History query error: {e}");
                        }
                    }
                }
            }
            _ => {
                warn!(msg_type, "Unknown WS message type");
            }
        }
    }

    // Cleanup
    if let Some(mid) = &machine_id {
        hub.remove_client(mid).await;
        info!(machine_id = %mid, "WS disconnected");
    }

    send_task.abort();
}
