use shell_sync_core::config::{client_config_dir, save_client_config, ClientConfig};
use shell_sync_core::encryption::KeyManager;
use shell_sync_core::models::RegisterResponse;

/// Register this machine with a sync server.
/// If `server_url` is None, attempts mDNS discovery first.
pub async fn register(server_url: Option<String>, groups: Vec<String>) -> anyhow::Result<()> {
    let url = match server_url {
        Some(u) => u,
        None => {
            // Try mDNS discovery
            match crate::discovery::discover_server(std::time::Duration::from_secs(5)).await {
                Some(u) => {
                    println!("Auto-discovered server via mDNS: {}", u);
                    u
                }
                None => {
                    anyhow::bail!(
                        "No server found via mDNS. Specify --server URL or ensure the server is running with mDNS enabled."
                    );
                }
            }
        }
    };

    let hostname = gethostname::gethostname()
        .to_string_lossy()
        .into_owned();

    // Generate encryption keypair
    let keys_dir = client_config_dir().join("keys");
    let key_manager = KeyManager::new(keys_dir)
        .map_err(|e| anyhow::anyhow!("Failed to initialize encryption keys: {e}"))?;
    let public_key = key_manager.public_key_b64();

    println!("Registering with {}...", url);
    println!("Groups: {}", groups.join(", "));

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/register", url))
        .json(&serde_json::json!({
            "hostname": hostname,
            "groups": groups,
            "os_type": std::env::consts::OS,
            "public_key": public_key
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Registration failed (HTTP {}): {}", status, body);
    }

    let data: RegisterResponse = resp.json().await?;

    let config = ClientConfig {
        server_url: url,
        machine_id: data.machine_id.clone(),
        auth_token: data.auth_token,
        groups,
        hostname,
    };

    save_client_config(&config)?;

    println!("Registration successful!");
    println!("Machine ID: {}", data.machine_id);
    println!();
    println!("Next steps:");
    println!("  1. shell-sync connect    # Start the daemon");
    println!("  2. shell-sync status     # Check connection");

    Ok(())
}
