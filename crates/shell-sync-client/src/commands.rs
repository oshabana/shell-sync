use shell_sync_core::config::{load_client_config, pid_file_path, ClientConfig};
use shell_sync_core::models::Alias;

fn client_and_config() -> anyhow::Result<(reqwest::Client, ClientConfig)> {
    let config = load_client_config()?;
    Ok((reqwest::Client::new(), config))
}

fn auth_header(config: &ClientConfig) -> String {
    format!("Bearer {}", config.auth_token)
}

/// `shell-sync add <name> <command> --group <group>`
pub async fn add_alias(name: &str, command: &str, group: &str) -> anyhow::Result<()> {
    let (client, config) = client_and_config()?;

    let resp = client
        .post(format!("{}/api/aliases", config.server_url))
        .header("Authorization", auth_header(&config))
        .json(&serde_json::json!({ "name": name, "command": command, "group": group }))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            println!("Alias '{}' synced successfully", name);
        }
        Ok(r) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            let msg = body["error"].as_str().unwrap_or("Unknown error");
            anyhow::bail!("Failed: {}", msg);
        }
        Err(_) => {
            // Offline — queue it
            crate::offline::queue_operation(
                "add",
                &serde_json::json!({ "name": name, "command": command, "group": group }),
            )?;
            println!("Server unreachable — queued for offline sync");
        }
    }

    Ok(())
}

/// `shell-sync rm <name> --group <group>`
pub async fn remove_alias(name: &str, group: &str) -> anyhow::Result<()> {
    let (client, config) = client_and_config()?;

    let resp = client
        .delete(format!(
            "{}/api/aliases/name/{}?group={}",
            config.server_url, name, group
        ))
        .header("Authorization", auth_header(&config))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            println!("Alias '{}' deleted successfully", name);
        }
        Ok(r) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            let msg = body["error"].as_str().unwrap_or("Unknown error");
            anyhow::bail!("Failed: {}", msg);
        }
        Err(_) => {
            crate::offline::queue_operation(
                "delete",
                &serde_json::json!({ "name": name, "group": group }),
            )?;
            println!("Server unreachable — queued for offline sync");
        }
    }

    Ok(())
}

/// `shell-sync ls [--group X] [--format table|json]`
pub async fn list_aliases(group: Option<&str>, json_format: bool) -> anyhow::Result<()> {
    let (client, config) = client_and_config()?;

    let resp = client
        .get(format!("{}/api/aliases", config.server_url))
        .header("Authorization", auth_header(&config))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Failed to fetch aliases (HTTP {})", resp.status());
    }

    let data: serde_json::Value = resp.json().await?;
    let aliases: Vec<Alias> = serde_json::from_value(data["aliases"].clone()).unwrap_or_default();

    let filtered: Vec<&Alias> = if let Some(g) = group {
        aliases.iter().filter(|a| a.group_name == g).collect()
    } else {
        aliases.iter().collect()
    };

    if json_format {
        println!("{}", serde_json::to_string_pretty(&filtered)?);
    } else {
        if filtered.is_empty() {
            println!("No aliases found");
            return Ok(());
        }

        let mut table = comfy_table::Table::new();
        table.set_header(vec!["Name", "Command", "Group", "Version"]);
        for a in &filtered {
            table.add_row(vec![
                &a.name,
                &a.command,
                &a.group_name,
                &a.version.to_string(),
            ]);
        }
        println!("{table}");
    }

    Ok(())
}

/// `shell-sync update <name> <command> --group <group>`
pub async fn update_alias(name: &str, command: &str, group: &str) -> anyhow::Result<()> {
    let (client, config) = client_and_config()?;

    // First find the alias by name to get its ID
    let resp = client
        .get(format!("{}/api/aliases", config.server_url))
        .header("Authorization", auth_header(&config))
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    let aliases: Vec<Alias> = serde_json::from_value(data["aliases"].clone()).unwrap_or_default();

    let alias = aliases
        .iter()
        .find(|a| a.name == name && a.group_name == group)
        .ok_or_else(|| anyhow::anyhow!("Alias '{}' not found in group '{}'", name, group))?;

    let resp = client
        .put(format!("{}/api/aliases/{}", config.server_url, alias.id))
        .header("Authorization", auth_header(&config))
        .json(&serde_json::json!({ "command": command }))
        .send()
        .await?;

    if resp.status().is_success() {
        println!("Alias '{}' updated successfully", name);
    } else {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        anyhow::bail!("Failed: {}", body["error"].as_str().unwrap_or("Unknown error"));
    }

    Ok(())
}

/// `shell-sync import [--file path] --group <group> [--dry-run]`
pub async fn import_aliases(file: Option<&str>, group: &str, dry_run: bool) -> anyhow::Result<()> {
    let content = match file {
        Some(path) => std::fs::read_to_string(path)?,
        None => {
            // Read from stdin
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };

    // Parse alias lines: `alias name='command'` or `name=command` or `name command`
    let mut aliases = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.strip_prefix("alias ").unwrap_or(line);

        if let Some((name, cmd)) = line.split_once('=') {
            let name = name.trim();
            let cmd = cmd.trim().trim_matches('\'').trim_matches('"');
            aliases.push(serde_json::json!({ "name": name, "command": cmd }));
        }
    }

    if dry_run {
        println!("Would import {} aliases to group '{}':", aliases.len(), group);
        for a in &aliases {
            println!("  {} = {}", a["name"].as_str().unwrap_or(""), a["command"].as_str().unwrap_or(""));
        }
        return Ok(());
    }

    let (client, config) = client_and_config()?;

    let resp = client
        .post(format!("{}/api/import", config.server_url))
        .header("Authorization", auth_header(&config))
        .json(&serde_json::json!({ "aliases": aliases, "group": group }))
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    println!(
        "Import completed: {} added, {} failed",
        data["added"].as_i64().unwrap_or(0),
        data["failed"].as_i64().unwrap_or(0)
    );

    Ok(())
}

/// `shell-sync export`
pub async fn export_aliases() -> anyhow::Result<()> {
    let (client, config) = client_and_config()?;

    let resp = client
        .get(format!("{}/api/aliases", config.server_url))
        .header("Authorization", auth_header(&config))
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    let aliases: Vec<Alias> = serde_json::from_value(data["aliases"].clone()).unwrap_or_default();

    for a in &aliases {
        let escaped = a.command.replace('\'', "'\\''");
        println!("alias {}='{}'", a.name, escaped);
    }

    Ok(())
}

/// `shell-sync sync`
pub async fn force_sync() -> anyhow::Result<()> {
    let (client, config) = client_and_config()?;

    // First, flush offline queue
    let flushed = crate::offline::flush_queue(&config.server_url, &config.auth_token).await?;
    if flushed > 0 {
        println!("Flushed {} offline operations", flushed);
    }

    let resp = client
        .get(format!("{}/api/aliases", config.server_url))
        .header("Authorization", auth_header(&config))
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    let aliases: Vec<Alias> = serde_json::from_value(data["aliases"].clone()).unwrap_or_default();

    crate::shell_writer::apply_aliases(&aliases)?;
    println!("Synced {} aliases", aliases.len());

    Ok(())
}

/// `shell-sync status`
pub fn status() -> anyhow::Result<()> {
    let config = match load_client_config() {
        Ok(c) => c,
        Err(_) => {
            println!("Status: Not configured");
            println!("Run: shell-sync register");
            return Ok(());
        }
    };

    let running = is_daemon_running();
    println!("Status: {}", if running { "Running" } else { "Not running" });
    println!("Server: {}", config.server_url);
    println!("Groups: {}", config.groups.join(", "));
    println!("Machine: {}", config.machine_id);

    let pending = crate::offline::pending_count().unwrap_or(0);
    if pending > 0 {
        println!("Offline queue: {} pending operations", pending);
    }

    Ok(())
}

/// `shell-sync stop`
pub fn stop_daemon() -> anyhow::Result<()> {
    let pid_path = pid_file_path();
    if !pid_path.exists() {
        println!("Daemon is not running");
        return Ok(());
    }

    let pid_str = std::fs::read_to_string(&pid_path)?;
    let pid: i32 = pid_str.trim().parse()?;

    // Send SIGTERM
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }

    let _ = std::fs::remove_file(&pid_path);
    println!("Daemon stopped");

    Ok(())
}

/// `shell-sync conflicts`
pub async fn list_conflicts() -> anyhow::Result<()> {
    let (client, config) = client_and_config()?;

    let resp = client
        .get(format!("{}/api/conflicts", config.server_url))
        .header("Authorization", auth_header(&config))
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    let conflicts = data["conflicts"].as_array();

    match conflicts {
        Some(c) if !c.is_empty() => {
            println!("{} conflicts found:\n", c.len());
            for (i, conflict) in c.iter().enumerate() {
                println!("{}. {}", i + 1, conflict["alias_name"].as_str().unwrap_or(""));
                println!("   Local:  {}", conflict["local_command"].as_str().unwrap_or(""));
                println!("   Remote: {}", conflict["remote_command"].as_str().unwrap_or(""));
                println!();
            }
        }
        _ => println!("No conflicts"),
    }

    Ok(())
}

/// `shell-sync history [--limit N]`
pub async fn show_history(limit: i64) -> anyhow::Result<()> {
    let (client, config) = client_and_config()?;

    let resp = client
        .get(format!("{}/api/history?limit={}", config.server_url, limit))
        .header("Authorization", auth_header(&config))
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    let history = data["history"].as_array();

    match history {
        Some(h) if !h.is_empty() => {
            let mut table = comfy_table::Table::new();
            table.set_header(vec!["Time", "Action", "Alias", "Group"]);
            for entry in h {
                let ts = entry["timestamp"].as_i64().unwrap_or(0);
                let time = chrono::DateTime::from_timestamp_millis(ts)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                table.add_row(vec![
                    &time,
                    entry["action"].as_str().unwrap_or(""),
                    entry["alias_name"].as_str().unwrap_or(""),
                    entry["group_name"].as_str().unwrap_or(""),
                ]);
            }
            println!("{table}");
        }
        _ => println!("No history"),
    }

    Ok(())
}

/// `shell-sync machines`
pub async fn list_machines() -> anyhow::Result<()> {
    let (client, config) = client_and_config()?;

    let resp = client
        .get(format!("{}/api/machines", config.server_url))
        .header("Authorization", auth_header(&config))
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    let machines = data["machines"].as_array();

    match machines {
        Some(m) if !m.is_empty() => {
            let mut table = comfy_table::Table::new();
            table.set_header(vec!["Hostname", "OS", "Groups", "Last Seen"]);
            for machine in m {
                let last_seen = machine["last_seen"].as_i64().unwrap_or(0);
                let time = chrono::DateTime::from_timestamp_millis(last_seen)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                let groups = machine["groups"]
                    .as_array()
                    .map(|g| {
                        g.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                table.add_row(vec![
                    machine["hostname"].as_str().unwrap_or(""),
                    machine["os_type"].as_str().unwrap_or(""),
                    &groups,
                    &time,
                ]);
            }
            println!("{table}");
        }
        _ => println!("No machines registered"),
    }

    Ok(())
}

/// `shell-sync git-backup`
pub async fn git_backup() -> anyhow::Result<()> {
    let (client, config) = client_and_config()?;

    let resp = client
        .post(format!("{}/api/git/sync", config.server_url))
        .header("Authorization", auth_header(&config))
        .send()
        .await?;

    if resp.status().is_success() {
        println!("Git backup completed");
    } else {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        anyhow::bail!("Failed: {}", body["error"].as_str().unwrap_or("Unknown error"));
    }

    Ok(())
}

/// `shell-sync migrate <old-db-path>`
pub fn migrate(old_db_path: &str) -> anyhow::Result<()> {
    use rusqlite::Connection;

    println!("Migrating from Node.js database: {}", old_db_path);

    let old_conn = Connection::open(old_db_path)?;

    // Read machines
    let mut stmt = old_conn.prepare("SELECT machine_id, hostname, groups, os_type, auth_token, last_seen, created_at FROM machines")?;
    let machines: Vec<(String, String, String, Option<String>, String, i64, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                row.get(4)?, row.get(5)?, row.get(6)?,
            ))
        })?
        .collect::<Result<_, _>>()?;

    // Read aliases
    let mut stmt = old_conn.prepare("SELECT name, command, group_name, created_by_machine, created_at, updated_at, version FROM aliases")?;
    let aliases: Vec<(String, String, String, String, i64, i64, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                row.get(4)?, row.get(5)?, row.get(6)?,
            ))
        })?
        .collect::<Result<_, _>>()?;

    println!("Found {} machines and {} aliases", machines.len(), aliases.len());

    // Create new database
    let new_db = shell_sync_core::db::SyncDatabase::open("./data/sync.db")?;

    // Migrate machines (preserving UUIDs and tokens)
    for (mid, host, groups, os, token, _, _) in &machines {
        let groups: Vec<String> = serde_json::from_str(groups).unwrap_or_default();
        new_db.register_machine(mid, host, &groups, os.as_deref().unwrap_or("unknown"), token)?;
    }

    // Migrate aliases
    let mut added = 0;
    let mut skipped = 0;
    for (name, command, group, machine, _, _, _) in &aliases {
        match new_db.add_alias(name, command, group, machine) {
            Ok(_) => added += 1,
            Err(_) => skipped += 1,
        }
    }

    println!("Migration complete: {} aliases migrated, {} skipped (duplicates)", added, skipped);

    Ok(())
}

fn is_daemon_running() -> bool {
    let pid_path = pid_file_path();
    if !pid_path.exists() {
        return false;
    }

    match std::fs::read_to_string(&pid_path) {
        Ok(pid_str) => {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                // Check if process exists
                unsafe { libc::kill(pid, 0) == 0 }
            } else {
                false
            }
        }
        Err(_) => false,
    }
}
