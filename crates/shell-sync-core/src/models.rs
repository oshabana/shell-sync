use serde::{Deserialize, Serialize};

/// A shell alias that can be synced across machines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alias {
    pub id: i64,
    pub name: String,
    pub command: String,
    pub group_name: String,
    pub created_by_machine: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i64,
}

/// A registered machine in the sync network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Machine {
    pub id: i64,
    pub machine_id: String,
    pub hostname: String,
    pub groups: Vec<String>,
    pub os_type: Option<String>,
    pub auth_token: String,
    pub last_seen: i64,
    pub created_at: i64,
    #[serde(default)]
    pub public_key: Option<String>,
}

/// A conflict between local and remote alias versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub id: i64,
    pub alias_name: String,
    pub group_name: String,
    pub local_command: String,
    pub remote_command: String,
    pub machine_id: String,
    pub created_at: i64,
    pub resolved: bool,
    pub resolution: Option<String>,
}

/// A record of a sync action in history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncHistoryEntry {
    pub id: i64,
    pub timestamp: i64,
    pub machine_id: String,
    pub action: String,
    pub alias_name: String,
    pub alias_command: Option<String>,
    pub group_name: Option<String>,
}

/// Response returned when registering a new machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub machine_id: String,
    pub auth_token: String,
    pub message: String,
}

/// Request body for machine registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub hostname: String,
    pub groups: Vec<String>,
    #[serde(default)]
    pub os_type: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
}

/// Request body for adding an alias.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddAliasRequest {
    pub name: String,
    pub command: String,
    #[serde(default = "default_group")]
    pub group: String,
}

/// Request body for updating an alias.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAliasRequest {
    pub command: String,
}

/// Request body for resolving a conflict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveConflictRequest {
    pub conflict_id: i64,
    pub resolution: String,
}

/// Request body for bulk import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRequest {
    pub aliases: Vec<ImportAlias>,
    #[serde(default = "default_group")]
    pub group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportAlias {
    pub name: String,
    pub command: String,
}

/// A shell history entry that can be synced across machines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub command: String,
    pub cwd: String,
    pub exit_code: i32,
    pub duration_ms: i64,
    pub session_id: String,
    pub machine_id: String,
    pub hostname: String,
    pub timestamp: i64,
    pub shell: String,
    pub group_name: String,
}

/// Payload sent from shell hooks via Unix socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryHookPayload {
    pub command: String,
    pub cwd: String,
    pub exit_code: i32,
    pub duration_ms: i64,
    pub session_id: String,
    pub shell: String,
}

/// Encrypted version of HistoryEntry for wire transmission.
/// Sensitive fields are encrypted; routing metadata stays plaintext.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedHistoryEntry {
    pub id: String,
    pub command: String,     // base64 ciphertext
    pub cwd: String,         // base64 ciphertext
    pub exit_code: String,   // base64 ciphertext
    pub duration_ms: String, // base64 ciphertext
    pub session_id: String,  // plaintext (routing)
    pub machine_id: String,  // plaintext (routing)
    pub hostname: String,    // base64 ciphertext
    pub timestamp: i64,      // plaintext (for ordering/pagination)
    pub shell: String,       // plaintext
    pub group_name: String,  // plaintext (routing)
    pub nonces: String,      // JSON array of base64 nonces for each encrypted field
}

/// Encrypted version of Alias for wire transmission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedAlias {
    pub id: i64,
    pub name: String,       // plaintext (needed for shell file)
    pub command: String,    // base64 ciphertext
    pub group_name: String, // plaintext
    pub created_by_machine: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: i64,
    pub nonce: String, // base64 nonce for command field
}

fn default_group() -> String {
    "default".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_json_roundtrip() {
        let alias = Alias {
            id: 1,
            name: "gs".into(),
            command: "git status".into(),
            group_name: "default".into(),
            created_by_machine: "m1".into(),
            created_at: 1000,
            updated_at: 2000,
            version: 3,
        };
        let json = serde_json::to_string(&alias).unwrap();
        let parsed: Alias = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.name, "gs");
        assert_eq!(parsed.command, "git status");
        assert_eq!(parsed.group_name, "default");
        assert_eq!(parsed.version, 3);
    }

    #[test]
    fn add_alias_request_default_group() {
        let req: AddAliasRequest =
            serde_json::from_str(r#"{"name":"gs","command":"git status"}"#).unwrap();
        assert_eq!(req.group, "default");
    }

    #[test]
    fn add_alias_request_explicit_group() {
        let req: AddAliasRequest =
            serde_json::from_str(r#"{"name":"gs","command":"git status","group":"work"}"#).unwrap();
        assert_eq!(req.group, "work");
    }

    #[test]
    fn import_request_default_group() {
        let req: ImportRequest =
            serde_json::from_str(r#"{"aliases":[{"name":"gs","command":"git status"}]}"#).unwrap();
        assert_eq!(req.group, "default");
    }

    #[test]
    fn register_request_optional_os() {
        let with: RegisterRequest =
            serde_json::from_str(r#"{"hostname":"mac","groups":["default"],"os_type":"macos"}"#)
                .unwrap();
        assert_eq!(with.os_type, Some("macos".into()));

        let without: RegisterRequest =
            serde_json::from_str(r#"{"hostname":"mac","groups":["default"]}"#).unwrap();
        assert!(without.os_type.is_none());
    }
}
