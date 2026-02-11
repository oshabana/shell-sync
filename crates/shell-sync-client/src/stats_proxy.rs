use std::sync::Arc;

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use shell_sync_core::db::SyncDatabase;
use shell_sync_core::stats::{compute_stats, parse_last_filter, StatsFilter, StatsResult};
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

#[derive(Deserialize)]
struct StatsQuery {
    last: Option<String>,
    machine: Option<String>,
    group: Option<String>,
    directory: Option<String>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
    limit: Option<i64>,
}

/// Start the local stats HTTP proxy on 127.0.0.1:18888.
/// This is spawned as a background task in the daemon.
pub async fn start_stats_proxy(db: Arc<SyncDatabase>) -> anyhow::Result<()> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/local/stats", get(handle_stats))
        .route("/api/local/search", get(handle_search))
        .layer(cors)
        .with_state(db);

    let listener = TcpListener::bind("127.0.0.1:18888").await?;
    info!("Stats proxy listening on http://127.0.0.1:18888");

    axum::serve(listener, app).await?;

    Ok(())
}

async fn handle_stats(
    axum::extract::State(db): axum::extract::State<Arc<SyncDatabase>>,
    Query(params): Query<StatsQuery>,
) -> Result<Json<StatsResult>, (StatusCode, String)> {
    let last = params.last.as_deref().unwrap_or("30d");
    let after_timestamp = parse_last_filter(last);

    let filter = StatsFilter {
        after_timestamp,
        machine_id: params.machine,
        group_name: params.group,
        directory: params.directory,
    };

    match compute_stats(&db, &filter) {
        Ok(stats) => Ok(Json(stats)),
        Err(e) => {
            error!("Stats computation failed: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

async fn handle_search(
    axum::extract::State(db): axum::extract::State<Arc<SyncDatabase>>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<Vec<shell_sync_core::models::HistoryEntry>>, (StatusCode, String)> {
    let query = params.q.as_deref().unwrap_or("");
    let limit = params.limit.unwrap_or(50).min(500);

    match db.search_history(query, None, None, None, limit, 0) {
        Ok(entries) => Ok(Json(entries)),
        Err(e) => {
            error!("Search failed: {e}");
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}
