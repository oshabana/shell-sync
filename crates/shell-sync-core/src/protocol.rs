use crate::models::HistoryEntry;
use serde::{Deserialize, Serialize};

/// Messages sent from client to server over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    #[serde(rename = "auth")]
    Auth { token: String },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "history_batch")]
    HistoryBatch { entries: Vec<HistoryEntry> },
    #[serde(rename = "history_query")]
    HistoryQuery {
        after_timestamp: i64,
        group_name: String,
        limit: i64,
    },
    #[serde(rename = "key_request")]
    KeyRequest {
        group_name: String,
        public_key: String,
    },
    #[serde(rename = "key_response")]
    KeyResponse {
        group_name: String,
        target_machine_id: String,
        wrapped_key: String,
    },
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
    #[serde(rename = "history_sync")]
    HistorySync { data: HistorySyncData },
    #[serde(rename = "history_page")]
    HistoryPage { data: HistoryPageData },
    #[serde(rename = "key_request")]
    KeyRequestEvent { data: KeyRequestData },
    #[serde(rename = "key_response")]
    KeyResponseEvent { data: KeyResponseData },
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySyncData {
    pub entries: Vec<HistoryEntry>,
    pub source_machine_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryPageData {
    pub entries: Vec<HistoryEntry>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRequestData {
    pub group_name: String,
    pub requester_machine_id: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyResponseData {
    pub group_name: String,
    pub wrapped_key: String,
    pub sender_public_key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_auth_serializes() {
        let msg = ClientMessage::Auth {
            token: "abc".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"auth","token":"abc"}"#);
    }

    #[test]
    fn client_auth_roundtrip() {
        let msg = ClientMessage::Auth {
            token: "tok-123".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ClientMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            ClientMessage::Auth { token } => assert_eq!(token, "tok-123"),
            _ => panic!("Expected Auth variant"),
        }
    }

    #[test]
    fn client_ping_serializes() {
        let msg = ClientMessage::Ping;
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"ping"}"#);
    }

    #[test]
    fn server_auth_success_roundtrip() {
        let event = ServerEvent::AuthSuccess {
            data: AuthSuccessData {
                machine_id: "m1".into(),
                groups: vec!["default".into(), "work".into()],
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"auth_success""#));
        let parsed: ServerEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ServerEvent::AuthSuccess { data } => {
                assert_eq!(data.machine_id, "m1");
                assert_eq!(data.groups, vec!["default", "work"]);
            }
            _ => panic!("Expected AuthSuccess"),
        }
    }

    #[test]
    fn server_auth_failed_roundtrip() {
        let event = ServerEvent::AuthFailed {
            data: AuthFailedData {
                error: "bad token".into(),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: ServerEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ServerEvent::AuthFailed { data } => assert_eq!(data.error, "bad token"),
            _ => panic!("Expected AuthFailed"),
        }
    }

    #[test]
    fn server_pong_roundtrip() {
        let event = ServerEvent::Pong {
            data: PongData {
                timestamp: 1234567890,
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: ServerEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ServerEvent::Pong { data } => assert_eq!(data.timestamp, 1234567890),
            _ => panic!("Expected Pong"),
        }
    }

    #[test]
    fn server_alias_added_roundtrip() {
        let event = ServerEvent::AliasAdded {
            data: serde_json::json!({"id": 1, "name": "gs"}),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: ServerEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            ServerEvent::AliasAdded { data } => {
                assert_eq!(data["name"], "gs");
            }
            _ => panic!("Expected AliasAdded"),
        }
    }

    #[test]
    fn unknown_type_fails() {
        let result = serde_json::from_str::<ClientMessage>(r#"{"type":"bogus"}"#);
        assert!(result.is_err());
    }
}
