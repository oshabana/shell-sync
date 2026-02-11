use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use shell_sync_core::db::SyncDatabase;
use shell_sync_core::models::*;
use shell_sync_core::secrets::check_for_secrets;
use tracing::error;

use crate::git_backup::GitBackup;
use crate::ws::WsHub;

/// Shared application state passed to all route handlers.
pub struct AppState {
    pub db: Arc<SyncDatabase>,
    pub hub: Arc<WsHub>,
    pub git_backup: Arc<GitBackup>,
}

// ---------- helpers ----------

fn err(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

/// Extract and validate the Bearer token, returning the authenticated Machine.
fn authenticate(headers: &HeaderMap, db: &SyncDatabase) -> Result<Machine, (StatusCode, Json<serde_json::Value>)> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !auth.starts_with("Bearer ") {
        return Err(err(StatusCode::UNAUTHORIZED, "Missing or invalid authorization header"));
    }

    let token = &auth[7..];
    let machine = db
        .get_machine_by_token(token)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Invalid authentication token"))?;

    let _ = db.update_machine_last_seen(&machine.machine_id);
    Ok(machine)
}

// ---------- routes ----------

/// GET /api/health
pub async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let active = state.hub.client_count().await;
    Json(serde_json::json!({
        "status": "healthy",
        "active_machines": active,
        "timestamp": chrono::Utc::now().timestamp_millis()
    }))
}

/// POST /api/register
pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RegisterRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if body.hostname.is_empty() || body.groups.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Missing required fields: hostname, groups (array)"));
    }

    let machine_id = uuid::Uuid::new_v4().to_string();
    let auth_token = uuid::Uuid::new_v4().to_string();
    let os_type = body.os_type.as_deref().unwrap_or(std::env::consts::OS);

    state
        .db
        .register_machine(&machine_id, &body.hostname, &body.groups, os_type, &auth_token)
        .map_err(|e| {
            error!("Register error: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
        })?;

    tracing::info!(
        machine_id = %machine_id,
        hostname = %body.hostname,
        groups = ?body.groups,
        "Registered new machine"
    );

    Ok(Json(RegisterResponse {
        machine_id,
        auth_token,
        message: "Machine registered successfully".into(),
    }))
}

/// GET /api/aliases
pub async fn get_aliases(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let machine = authenticate(&headers, &state.db)?;
    let aliases = state
        .db
        .get_aliases_by_groups(&machine.groups)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let count = aliases.len();
    Ok(Json(serde_json::json!({
        "aliases": aliases,
        "groups": machine.groups,
        "count": count
    })))
}

/// POST /api/aliases
pub async fn add_alias(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<AddAliasRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let machine = authenticate(&headers, &state.db)?;

    if body.name.is_empty() || body.command.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Missing required fields: name, command"));
    }

    // Validate alias name
    let valid_name = body.name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-');
    if !valid_name {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Invalid alias name. Use only letters, numbers, underscore, dot, and dash.",
        ));
    }

    if check_for_secrets(&body.name, &body.command) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Potential secret detected in alias. Secrets should not be synced.",
        ));
    }

    if !machine.groups.contains(&body.group) {
        return Err(err(
            StatusCode::FORBIDDEN,
            &format!("Machine does not belong to group '{}'", body.group),
        ));
    }

    let alias = state
        .db
        .add_alias(&body.name, &body.command, &body.group, &machine.machine_id)
        .map_err(|e| {
            if e.to_string().contains("already exists") {
                err(StatusCode::CONFLICT, &e.to_string())
            } else {
                err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
            }
        })?;

    state.git_backup.mark_dirty();

    state
        .hub
        .broadcast_to_groups(
            &state.db,
            &[body.group.clone()],
            "alias_added",
            serde_json::to_value(&alias).unwrap_or_default(),
            Some(&machine.machine_id),
        )
        .await;

    Ok(Json(serde_json::json!({ "message": "Alias added successfully", "alias": alias })))
}

/// PUT /api/aliases/:id
pub async fn update_alias(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<UpdateAliasRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let machine = authenticate(&headers, &state.db)?;

    if body.command.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "Missing required field: command"));
    }

    let existing = state
        .db
        .get_alias_by_id(id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "Alias not found"))?;

    if check_for_secrets(&existing.name, &body.command) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Potential secret detected in alias. Secrets should not be synced.",
        ));
    }

    let updated = state
        .db
        .update_alias(id, &body.command, &machine.machine_id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "Alias not found"))?;

    state.git_backup.mark_dirty();

    state
        .hub
        .broadcast_to_groups(
            &state.db,
            &[updated.group_name.clone()],
            "alias_updated",
            serde_json::to_value(&updated).unwrap_or_default(),
            Some(&machine.machine_id),
        )
        .await;

    Ok(Json(serde_json::json!({ "message": "Alias updated successfully", "alias": updated })))
}

/// DELETE /api/aliases/:id
pub async fn delete_alias(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let machine = authenticate(&headers, &state.db)?;

    let alias = state
        .db
        .get_alias_by_id(id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "Alias not found"))?;

    let deleted = state
        .db
        .delete_alias(id, &machine.machine_id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if !deleted {
        return Err(err(StatusCode::NOT_FOUND, "Alias not found"));
    }

    state.git_backup.mark_dirty();

    state
        .hub
        .broadcast_to_groups(
            &state.db,
            &[alias.group_name.clone()],
            "alias_deleted",
            serde_json::json!({ "id": id, "name": alias.name }),
            Some(&machine.machine_id),
        )
        .await;

    Ok(Json(serde_json::json!({ "message": "Alias deleted successfully" })))
}

#[derive(Deserialize)]
pub struct DeleteByNameQuery {
    pub group: Option<String>,
}

/// DELETE /api/aliases/name/:name
pub async fn delete_alias_by_name(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(query): Query<DeleteByNameQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let machine = authenticate(&headers, &state.db)?;
    let group = query.group.as_deref().unwrap_or("default");

    state
        .db
        .get_alias_by_name(&name, group)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "Alias not found"))?;

    let deleted = state
        .db
        .delete_alias_by_name(&name, group, &machine.machine_id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if !deleted {
        return Err(err(StatusCode::NOT_FOUND, "Alias not found"));
    }

    state.git_backup.mark_dirty();

    state
        .hub
        .broadcast_to_groups(
            &state.db,
            &[group.to_string()],
            "alias_deleted",
            serde_json::json!({ "name": name, "group": group }),
            Some(&machine.machine_id),
        )
        .await;

    Ok(Json(serde_json::json!({ "message": "Alias deleted successfully" })))
}

/// GET /api/conflicts
pub async fn get_conflicts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let machine = authenticate(&headers, &state.db)?;
    let conflicts = state
        .db
        .get_conflicts_by_machine(&machine.machine_id)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let count = conflicts.len();
    Ok(Json(serde_json::json!({ "conflicts": conflicts, "count": count })))
}

/// POST /api/conflicts/resolve
pub async fn resolve_conflict(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ResolveConflictRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let _machine = authenticate(&headers, &state.db)?;

    let resolved = state
        .db
        .resolve_conflict(body.conflict_id, &body.resolution)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    if resolved {
        Ok(Json(serde_json::json!({ "message": "Conflict resolved successfully" })))
    } else {
        Err(err(StatusCode::NOT_FOUND, "Conflict not found"))
    }
}

/// POST /api/import
pub async fn import_aliases(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ImportRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let machine = authenticate(&headers, &state.db)?;

    let mut added = Vec::new();
    let mut failed = Vec::new();

    for import_alias in &body.aliases {
        match state
            .db
            .add_alias(&import_alias.name, &import_alias.command, &body.group, &machine.machine_id)
        {
            Ok(alias) => added.push(alias),
            Err(e) => failed.push(serde_json::json!({ "name": import_alias.name, "error": e.to_string() })),
        }
    }

    if !added.is_empty() {
        state.git_backup.mark_dirty();
        state
            .hub
            .broadcast_to_groups(
                &state.db,
                &[body.group.clone()],
                "sync_required",
                serde_json::json!({ "message": "Bulk import completed", "count": added.len() }),
                Some(&machine.machine_id),
            )
            .await;
    }

    Ok(Json(serde_json::json!({
        "message": "Import completed",
        "added": added.len(),
        "failed": failed.len(),
        "results": { "added": added, "failed": failed }
    })))
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
}

/// GET /api/history
pub async fn get_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let _machine = authenticate(&headers, &state.db)?;
    let limit = query.limit.unwrap_or(100);
    let history = state
        .db
        .get_history(limit)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let count = history.len();
    Ok(Json(serde_json::json!({ "history": history, "count": count })))
}

/// GET /api/machines
pub async fn get_machines(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let _machine = authenticate(&headers, &state.db)?;
    let machines = state
        .db
        .get_all_machines()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    // Hide auth tokens
    let sanitized: Vec<serde_json::Value> = machines
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "machine_id": m.machine_id,
                "hostname": m.hostname,
                "groups": m.groups,
                "os_type": m.os_type,
                "auth_token": "***",
                "last_seen": m.last_seen,
                "created_at": m.created_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "machines": sanitized, "count": sanitized.len() })))
}

/// POST /api/git/sync
pub async fn force_git_sync(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let _machine = authenticate(&headers, &state.db)?;
    state
        .git_backup
        .force_sync()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    Ok(Json(serde_json::json!({ "message": "Git sync completed" })))
}
