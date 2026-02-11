use std::sync::Arc;

use axum::{
    extract::{ws::WebSocketUpgrade, State},
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post, put},
    Router,
};
use rust_embed::Embed;
use shell_sync_core::config::ServerConfig;
use shell_sync_core::db::SyncDatabase;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::api::{self, AppState};
use crate::git_backup::GitBackup;
use crate::ws::{self, WsHub};

#[derive(Embed)]
#[folder = "../../web-ui/dist"]
struct WebAssets;

/// Build the Axum router with all API routes and WebSocket handler.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        // REST API
        .route("/api/health", get(api::health))
        .route("/api/register", post(api::register))
        .route("/api/aliases", get(api::get_aliases).post(api::add_alias))
        .route(
            "/api/aliases/:id",
            put(api::update_alias).delete(api::delete_alias),
        )
        .route("/api/aliases/name/:name", delete(api::delete_alias_by_name))
        .route("/api/conflicts", get(api::get_conflicts))
        .route("/api/conflicts/resolve", post(api::resolve_conflict))
        .route("/api/import", post(api::import_aliases))
        .route("/api/history", get(api::get_history))
        .route("/api/machines", get(api::get_machines))
        .route("/api/git/sync", post(api::force_git_sync))
        .route("/api/shell-history", get(api::get_shell_history))
        // WebSocket
        .route("/ws", get(ws_upgrade))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Build and start the shell-sync server.
pub async fn run(config: ServerConfig) -> anyhow::Result<()> {
    let db = Arc::new(SyncDatabase::open(&config.db_path)?);
    let hub = Arc::new(WsHub::new());
    let git_backup = Arc::new(GitBackup::new(Arc::clone(&db), &config.git_repo_path));

    git_backup.initialize()?;

    // Spawn periodic git sync
    let _sync_handle = git_backup.spawn_periodic_sync(config.git_sync_interval_secs);

    // Start mDNS broadcast
    let _mdns = if config.mdns_enabled {
        match crate::mdns::start_broadcast(config.port) {
            Ok(mdns) => Some(mdns),
            Err(e) => {
                tracing::warn!("Failed to start mDNS broadcast: {e}");
                None
            }
        }
    } else {
        None
    };

    let state = Arc::new(AppState {
        db: Arc::clone(&db),
        hub: Arc::clone(&hub),
        git_backup: Arc::clone(&git_backup),
    });

    let mut app = build_router(state);

    // Embed web UI if enabled
    if config.web_ui_enabled {
        app = app.fallback(serve_embedded);
    }

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!(
        port = config.port,
        db = %config.db_path,
        git = %config.git_repo_path,
        "Shell Sync server started"
    );

    println!();
    println!("=================================");
    println!("  Shell Sync Service Started");
    println!("=================================");
    println!("  REST API: http://localhost:{}", config.port);
    println!("  WebSocket: ws://localhost:{}/ws", config.port);
    println!("  Web UI: http://localhost:{}/", config.port);
    println!("  Database: {}", config.db_path);
    println!("  Git Repo: {}", config.git_repo_path);
    println!(
        "  mDNS: {}",
        if config.mdns_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!("=================================");
    println!();

    axum::serve(listener, app).await?;

    Ok(())
}

/// WebSocket upgrade handler at GET /ws.
async fn ws_upgrade(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        ws::handle_ws(socket, Arc::clone(&state.db), Arc::clone(&state.hub))
    })
}

/// Serve embedded web UI assets with SPA fallback.
async fn serve_embedded(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try to serve the exact file
    if let Some(file) = WebAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            file.data.to_vec(),
        )
            .into_response();
    }

    // SPA fallback: serve index.html for any unknown route
    if let Some(index) = WebAssets::get("index.html") {
        return Html(String::from_utf8_lossy(&index.data).to_string()).into_response();
    }

    (StatusCode::NOT_FOUND, "Not Found").into_response()
}
