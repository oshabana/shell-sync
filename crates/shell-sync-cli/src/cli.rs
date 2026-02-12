use clap::{Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser)]
#[command(
    name = "shell-sync",
    about = "Real-time shell alias synchronization across machines",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the sync server
    Serve {
        /// Port to listen on
        #[arg(long, default_value_t = 8888)]
        port: u16,
        /// Disable mDNS broadcasting
        #[arg(long)]
        no_mdns: bool,
        /// Disable embedded web UI
        #[arg(long)]
        no_web_ui: bool,
        /// Run in foreground (don't daemonize)
        #[arg(long)]
        foreground: bool,
    },

    /// Register this machine with a sync server
    Register {
        /// Server URL (falls back to SHELL_SYNC_SERVER env, then mDNS)
        #[arg(long, env = "SHELL_SYNC_SERVER")]
        server: Option<String>,
        /// Comma-separated list of groups
        #[arg(long, default_value = "default")]
        groups: String,
    },

    /// Start the client sync daemon
    Connect {
        /// Server URL (falls back to SHELL_SYNC_SERVER env, then config)
        #[arg(long, env = "SHELL_SYNC_SERVER")]
        server: Option<String>,
        /// Run in foreground
        #[arg(long)]
        foreground: bool,
    },

    /// Add a new alias
    Add {
        /// Alias name
        name: String,
        /// Alias command
        command: String,
        /// Target group
        #[arg(long, default_value = "default")]
        group: String,
    },

    /// Remove an alias
    Rm {
        /// Alias name
        name: String,
        /// Target group
        #[arg(long, default_value = "default")]
        group: String,
    },

    /// List aliases
    Ls {
        /// Filter by group
        #[arg(long)]
        group: Option<String>,
        /// Output format
        #[arg(long, default_value = "table")]
        format: OutputFormat,
    },

    /// Update an existing alias
    Update {
        /// Alias name
        name: String,
        /// New command
        command: String,
        /// Target group
        #[arg(long, default_value = "default")]
        group: String,
    },

    /// Import aliases from file or stdin
    Import {
        /// Path to file with aliases
        #[arg(long)]
        file: Option<String>,
        /// Target group
        #[arg(long, default_value = "default")]
        group: String,
        /// Show what would be imported without doing it
        #[arg(long)]
        dry_run: bool,
    },

    /// Export all aliases
    Export,

    /// Force a full sync
    Sync,

    /// Show daemon and connection status
    Status,

    /// Stop the daemon
    Stop,

    /// List and resolve conflicts
    Conflicts,

    /// Show sync history
    History {
        /// Maximum entries to show
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },

    /// List registered machines (server admin)
    Machines,

    /// Force a git backup commit
    GitBackup,

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },

    /// Migrate data from the Node.js version
    Migrate {
        /// Path to the old Node.js sync.db database
        old_db_path: String,
    },

    /// Interactive history search (Ctrl+R replacement)
    Search {
        /// Initial search query
        #[arg(default_value = "")]
        query: String,
        /// Output selected command to stdout (for shell integration)
        #[arg(long)]
        inline: bool,
    },

    /// Encrypt existing plaintext data and re-upload
    EncryptMigrate,

    /// Generate and install shell hooks for history capture
    InitHooks {
        /// Overwrite existing hook files
        #[arg(long)]
        force: bool,
    },

    /// Show shell usage statistics and analytics
    Stats {
        /// Time period (e.g., "7d", "30d", "1y", "all")
        #[arg(long, default_value = "30d")]
        last: String,
        /// Filter by machine
        #[arg(long)]
        machine: Option<String>,
        /// Filter by group
        #[arg(long)]
        group: Option<String>,
        /// Filter by directory
        #[arg(long)]
        directory: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Debug, clap::ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
}
