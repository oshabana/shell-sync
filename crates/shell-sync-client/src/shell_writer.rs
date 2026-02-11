use shell_sync_core::config::client_alias_path;
use shell_sync_core::models::Alias;
use shell_sync_core::shell::{detect_shell, ShellType};
use std::path::PathBuf;
use tracing::info;

/// Write aliases to the shell-sync alias file and ensure it's sourced from the RC file.
pub fn apply_aliases(aliases: &[Alias]) -> anyhow::Result<()> {
    let shell = detect_shell();
    let ext = shell.alias_extension();
    let alias_path = client_alias_path(ext);

    // Ensure directory exists
    if let Some(parent) = alias_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Generate alias file content
    let content = generate_alias_content(shell, aliases);
    std::fs::write(&alias_path, &content)?;

    info!(count = aliases.len(), path = %alias_path.display(), "Applied aliases");

    // Ensure the RC file sources our alias file
    ensure_source_line(shell, &alias_path)?;

    Ok(())
}

fn generate_alias_content(shell: ShellType, aliases: &[Alias]) -> String {
    let header = match shell {
        ShellType::Fish => format!(
            "# Shell Sync - auto-generated aliases\n# Last updated: {}\n# Total: {} aliases\n\n",
            chrono::Utc::now().to_rfc3339(),
            aliases.len()
        ),
        _ => format!(
            "#!/bin/bash\n# Shell Sync - auto-generated aliases\n# Last updated: {}\n# Total: {} aliases\n\n",
            chrono::Utc::now().to_rfc3339(),
            aliases.len()
        ),
    };

    let lines: Vec<String> = aliases
        .iter()
        .map(|a| shell.format_alias(&a.name, &a.command))
        .collect();

    format!("{}{}\n", header, lines.join("\n"))
}

fn ensure_source_line(shell: ShellType, alias_path: &PathBuf) -> anyhow::Result<()> {
    let rc_path = shell.rc_file();
    let alias_str = alias_path.to_string_lossy();
    let source_line = shell.source_line(&alias_str);

    // If the RC file doesn't exist, don't create it (fish conf.d might need special handling)
    if !rc_path.exists() {
        if shell == ShellType::Fish {
            // Create fish conf.d directory and file
            if let Some(parent) = rc_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&rc_path, format!("{}\n", source_line))?;
            info!(path = %rc_path.display(), "Created fish config");
        }
        return Ok(());
    }

    let content = std::fs::read_to_string(&rc_path)?;

    // Check if the source line already exists
    if content.contains(&alias_str.to_string()) {
        return Ok(());
    }

    // Append the source line
    let mut new_content = content;
    if !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&format!("\n# Shell Sync aliases\n{}\n", source_line));

    std::fs::write(&rc_path, new_content)?;
    info!(path = %rc_path.display(), "Added source line to shell config");

    Ok(())
}
