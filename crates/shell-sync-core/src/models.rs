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

fn default_group() -> String {
    "default".to_string()
}
