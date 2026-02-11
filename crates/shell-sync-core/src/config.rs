use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Server configuration stored in config.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_db_path")]
    pub db_path: String,
    #[serde(default = "default_git_repo_path")]
    pub git_repo_path: String,
    #[serde(default = "default_true")]
    pub mdns_enabled: bool,
    #[serde(default = "default_true")]
    pub web_ui_enabled: bool,
    #[serde(default = "default_git_sync_interval")]
    pub git_sync_interval_secs: u64,
}

/// Client configuration stored in ~/.shell-sync/config.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server_url: String,
    pub machine_id: String,
    pub auth_token: String,
    pub groups: Vec<String>,
    pub hostname: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            db_path: default_db_path(),
            git_repo_path: default_git_repo_path(),
            mdns_enabled: true,
            web_ui_enabled: true,
            git_sync_interval_secs: default_git_sync_interval(),
        }
    }
}

fn default_port() -> u16 {
    8888
}

fn default_db_path() -> String {
    "./data/sync.db".to_string()
}

fn default_git_repo_path() -> String {
    "./git-repo".to_string()
}

fn default_true() -> bool {
    true
}

fn default_git_sync_interval() -> u64 {
    300
}

/// Returns the path to the client config directory (~/.shell-sync/).
pub fn client_config_dir() -> PathBuf {
    let home = directories::BaseDirs::new()
        .expect("Could not determine home directory")
        .home_dir()
        .to_path_buf();
    home.join(".shell-sync")
}

/// Returns the path to the client config file.
pub fn client_config_path() -> PathBuf {
    client_config_dir().join("config.toml")
}

/// Returns the path to the client alias output file.
pub fn client_alias_path(extension: &str) -> PathBuf {
    client_config_dir().join(format!("aliases.{}", extension))
}

/// Returns the path to the PID file for the daemon.
pub fn pid_file_path() -> PathBuf {
    client_config_dir().join("daemon.pid")
}

/// Returns the path to the offline queue database.
pub fn offline_queue_db_path() -> PathBuf {
    client_config_dir().join("offline-queue.db")
}

/// Load client config from disk.
pub fn load_client_config() -> anyhow::Result<ClientConfig> {
    let path = client_config_path();
    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Failed to read config at {}: {}", path.display(), e))?;
    let config: ClientConfig = toml::from_str(&content)?;
    Ok(config)
}

/// Save client config to disk.
pub fn save_client_config(config: &ClientConfig) -> anyhow::Result<()> {
    let dir = client_config_dir();
    std::fs::create_dir_all(&dir)?;
    let path = client_config_path();
    let content = toml::to_string_pretty(config)?;
    std::fs::write(&path, content)?;
    Ok(())
}
