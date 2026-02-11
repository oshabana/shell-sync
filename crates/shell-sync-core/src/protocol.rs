use serde::{Deserialize, Serialize};

/// Messages sent from client to server over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "auth")]
    Auth { token: String },
    #[serde(rename = "ping")]
    Ping,
}

/// Events sent from server to client over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum ServerEvent {
    #[serde(rename = "auth_success")]
    AuthSuccess { data: AuthSuccessData },
    #[serde(rename = "auth_failed")]
    AuthFailed { data: AuthFailedData },
    #[serde(rename = "alias_added")]
    AliasAdded { data: serde_json::Value },
    #[serde(rename = "alias_updated")]
    AliasUpdated { data: serde_json::Value },
    #[serde(rename = "alias_deleted")]
    AliasDeleted { data: serde_json::Value },
    #[serde(rename = "sync_required")]
    SyncRequired { data: serde_json::Value },
    #[serde(rename = "pong")]
    Pong { data: PongData },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSuccessData {
    pub machine_id: String,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthFailedData {
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PongData {
    pub timestamp: i64,
}
