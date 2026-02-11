use clap::Parser;
use tracing_subscriber::EnvFilter;

mod cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = cli::Cli::parse();

    match cli.command {
        cli::Commands::Serve {
            port,
            no_mdns,
            no_web_ui,
            foreground: _,
        } => {
            let config = shell_sync_core::config::ServerConfig {
                port,
                mdns_enabled: !no_mdns,
                web_ui_enabled: !no_web_ui,
                ..Default::default()
            };
            shell_sync_server::server::run(config).await?;
        }

        cli::Commands::Register { server, groups } => {
            let groups: Vec<String> = groups.split(',').map(|s| s.trim().to_string()).collect();
            shell_sync_client::registration::register(server, groups).await?;
        }

        cli::Commands::Connect { server, foreground } => {
            shell_sync_client::daemon::run(server, foreground).await?;
        }

        cli::Commands::Add { name, command, group } => {
            shell_sync_client::commands::add_alias(&name, &command, &group).await?;
        }

        cli::Commands::Rm { name, group } => {
            shell_sync_client::commands::remove_alias(&name, &group).await?;
        }

        cli::Commands::Ls { group, format } => {
            shell_sync_client::commands::list_aliases(group.as_deref(), matches!(format, cli::OutputFormat::Json)).await?;
        }

        cli::Commands::Update { name, command, group } => {
            shell_sync_client::commands::update_alias(&name, &command, &group).await?;
        }

        cli::Commands::Import { file, group, dry_run } => {
            shell_sync_client::commands::import_aliases(file.as_deref(), &group, dry_run).await?;
        }

        cli::Commands::Export => {
            shell_sync_client::commands::export_aliases().await?;
        }

        cli::Commands::Sync => {
            shell_sync_client::commands::force_sync().await?;
        }

        cli::Commands::Status => {
            shell_sync_client::commands::status()?;
        }

        cli::Commands::Stop => {
            shell_sync_client::commands::stop_daemon()?;
        }

        cli::Commands::Conflicts => {
            shell_sync_client::commands::list_conflicts().await?;
        }

        cli::Commands::History { limit } => {
            shell_sync_client::commands::show_history(limit).await?;
        }

        cli::Commands::Machines => {
            shell_sync_client::commands::list_machines().await?;
        }

        cli::Commands::GitBackup => {
            shell_sync_client::commands::git_backup().await?;
        }

        cli::Commands::Completions { shell } => {
            use clap::CommandFactory;
            clap_complete::generate(shell, &mut cli::Cli::command(), "shell-sync", &mut std::io::stdout());
        }

        cli::Commands::Migrate { old_db_path } => {
            shell_sync_client::commands::migrate(&old_db_path)?;
        }
    }

    Ok(())
}
