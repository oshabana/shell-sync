use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use git2::{Repository, Signature};
use shell_sync_core::db::SyncDatabase;
use tracing::{error, info};

/// Manages periodic git backups of all aliases.
pub struct GitBackup {
    db: Arc<SyncDatabase>,
    repo_path: PathBuf,
    pending_changes: AtomicBool,
}

impl GitBackup {
    pub fn new(db: Arc<SyncDatabase>, repo_path: &str) -> Self {
        Self {
            db,
            repo_path: PathBuf::from(repo_path),
            pending_changes: AtomicBool::new(false),
        }
    }

    /// Initialize the git repository and aliases directory.
    pub fn initialize(&self) -> anyhow::Result<()> {
        let aliases_dir = self.repo_path.join("aliases");
        std::fs::create_dir_all(&aliases_dir)?;

        if !self.repo_path.join(".git").exists() {
            let repo = Repository::init(&self.repo_path)?;
            // Set config
            let mut config = repo.config()?;
            config.set_str("user.name", "Shell Sync Service")?;
            config.set_str("user.email", "shell-sync@localhost")?;
            info!(path = %self.repo_path.display(), "Initialized new git repository");
        }

        Ok(())
    }

    /// Mark that there are pending changes to commit.
    pub fn mark_dirty(&self) {
        self.pending_changes.store(true, Ordering::Relaxed);
    }

    /// Returns true if there are pending changes.
    pub fn has_pending_changes(&self) -> bool {
        self.pending_changes.load(Ordering::Relaxed)
    }

    /// Force a sync: write alias files and commit.
    pub fn force_sync(&self) -> anyhow::Result<()> {
        self.mark_dirty();
        self.sync_to_git()
    }

    /// Write alias files and commit if there are pending changes.
    pub fn sync_to_git(&self) -> anyhow::Result<()> {
        if !self.pending_changes.load(Ordering::Relaxed) {
            return Ok(());
        }

        info!("Starting sync to git...");

        let aliases = self.db.get_all_aliases()?;

        // Group aliases by group_name
        let mut grouped: std::collections::HashMap<String, Vec<shell_sync_core::models::Alias>> =
            std::collections::HashMap::new();
        for alias in &aliases {
            grouped
                .entry(alias.group_name.clone())
                .or_default()
                .push(alias.clone());
        }

        let aliases_dir = self.repo_path.join("aliases");
        std::fs::create_dir_all(&aliases_dir)?;

        // Write each group to its own file
        for (group_name, group_aliases) in &grouped {
            let filename = aliases_dir.join(format!("{}.sh", group_name));
            let content = generate_alias_file(group_name, group_aliases);
            std::fs::write(&filename, content)?;
        }

        // Remove files for groups that no longer exist
        if let Ok(entries) = std::fs::read_dir(&aliases_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "sh") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if !grouped.contains_key(stem) {
                            let _ = std::fs::remove_file(&path);
                            info!(file = %path.display(), "Removed obsolete alias file");
                        }
                    }
                }
            }
        }

        // Write summary
        let summary = generate_summary(&grouped);
        std::fs::write(self.repo_path.join("SUMMARY.md"), summary)?;

        // Git add + commit
        self.git_commit(&aliases, &grouped)?;
        self.pending_changes.store(false, Ordering::Relaxed);

        Ok(())
    }

    fn git_commit(
        &self,
        aliases: &[shell_sync_core::models::Alias],
        grouped: &std::collections::HashMap<String, Vec<shell_sync_core::models::Alias>>,
    ) -> anyhow::Result<()> {
        let repo = Repository::open(&self.repo_path)?;

        // Add all files to index
        let mut index = repo.index()?;
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        index.write()?;

        // Check if there are actual changes
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;

        let has_head = repo.head().is_ok();
        if has_head {
            let head = repo.head()?;
            let head_commit = head.peel_to_commit()?;
            let head_tree = head_commit.tree()?;
            let diff = repo.diff_tree_to_tree(Some(&head_tree), Some(&tree), None)?;
            if diff.deltas().count() == 0 {
                info!("No changes to commit");
                return Ok(());
            }
        }

        let sig = Signature::now("Shell Sync Service", "shell-sync@localhost")?;
        let message = format!(
            "Auto-sync shell aliases\n\nTotal: {} aliases across {} groups\nTimestamp: {}\n\nSynced by Shell Sync Service",
            aliases.len(),
            grouped.len(),
            chrono::Utc::now().to_rfc3339()
        );

        if has_head {
            let head = repo.head()?;
            let parent = head.peel_to_commit()?;
            repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&parent])?;
        } else {
            repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[])?;
        }

        info!(aliases = aliases.len(), groups = grouped.len(), "Committed changes");
        Ok(())
    }

    /// Spawn a background task that periodically syncs.
    pub fn spawn_periodic_sync(self: &Arc<Self>, interval_secs: u64) -> tokio::task::JoinHandle<()> {
        let backup = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                if backup.has_pending_changes() {
                    if let Err(e) = backup.sync_to_git() {
                        error!("Periodic git sync error: {e}");
                    }
                }
            }
        })
    }
}

fn generate_alias_file(group_name: &str, aliases: &[shell_sync_core::models::Alias]) -> String {
    let mut out = format!(
        "#!/bin/bash\n# Shell Sync - {} group\n# Auto-generated on {}\n# Total aliases: {}\n\n",
        group_name,
        chrono::Utc::now().to_rfc3339(),
        aliases.len()
    );

    for alias in aliases {
        let escaped = alias.command.replace('\'', "'\\''");
        out.push_str(&format!("alias {}='{}'\n", alias.name, escaped));
    }

    out
}

fn generate_summary(
    grouped: &std::collections::HashMap<String, Vec<shell_sync_core::models::Alias>>,
) -> String {
    let total: usize = grouped.values().map(|v| v.len()).sum();
    let mut out = format!(
        "# Shell Sync - Alias Summary\n\nLast updated: {}\n\n## Statistics\n\n- Total groups: {}\n- Total aliases: {}\n\n## Groups\n\n",
        chrono::Utc::now().to_rfc3339(),
        grouped.len(),
        total
    );

    for (name, aliases) in grouped {
        out.push_str(&format!(
            "### {} ({} aliases)\n\nFile: `aliases/{}.sh`\n\n",
            name,
            aliases.len(),
            name
        ));
        for alias in aliases {
            out.push_str(&format!("- **{}**: `{}`\n", alias.name, alias.command));
        }
        out.push('\n');
    }

    out
}
