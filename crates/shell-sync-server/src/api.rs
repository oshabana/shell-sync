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
fn authenticate(
    headers: &HeaderMap,
    db: &SyncDatabase,
) -> Result<Machine, (StatusCode, Json<serde_json::Value>)> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !auth.starts_with("Bearer ") {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "Missing or invalid authorization header",
        ));
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
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Missing required fields: hostname, groups (array)",
        ));
    }

    let machine_id = uuid::Uuid::new_v4().to_string();
    let auth_token = uuid::Uuid::new_v4().to_string();
    let os_type = body.os_type.as_deref().unwrap_or(std::env::consts::OS);

    state
        .db
        .register_machine(
            &machine_id,
            &body.hostname,
            &body.groups,
            os_type,
            &auth_token,
            body.public_key.as_deref(),
        )
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
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Missing required fields: name, command",
        ));
    }

    // Validate alias name
    let valid_name = body
        .name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-');
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

    Ok(Json(
        serde_json::json!({ "message": "Alias added successfully", "alias": alias }),
    ))
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
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Missing required field: command",
        ));
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

    Ok(Json(
        serde_json::json!({ "message": "Alias updated successfully", "alias": updated }),
    ))
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

    Ok(Json(
        serde_json::json!({ "message": "Alias deleted successfully" }),
    ))
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

    Ok(Json(
        serde_json::json!({ "message": "Alias deleted successfully" }),
    ))
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
    Ok(Json(
        serde_json::json!({ "conflicts": conflicts, "count": count }),
    ))
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
        Ok(Json(
            serde_json::json!({ "message": "Conflict resolved successfully" }),
        ))
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

    if !machine.groups.contains(&body.group) {
        return Err(err(
            StatusCode::FORBIDDEN,
            &format!("Machine does not belong to group '{}'", body.group),
        ));
    }

    let mut added = Vec::new();
    let mut failed = Vec::new();

    for import_alias in &body.aliases {
        if check_for_secrets(&import_alias.name, &import_alias.command) {
            failed.push(serde_json::json!({
                "name": import_alias.name,
                "error": "Potential secret detected in alias. Secrets should not be synced."
            }));
            continue;
        }
        match state.db.add_alias(
            &import_alias.name,
            &import_alias.command,
            &body.group,
            &machine.machine_id,
        ) {
            Ok(alias) => added.push(alias),
            Err(e) => failed
                .push(serde_json::json!({ "name": import_alias.name, "error": e.to_string() })),
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
    Ok(Json(
        serde_json::json!({ "history": history, "count": count }),
    ))
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

    Ok(Json(
        serde_json::json!({ "machines": sanitized, "count": sanitized.len() }),
    ))
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

#[derive(Deserialize)]
pub struct ShellHistoryQuery {
    pub after_timestamp: Option<i64>,
    pub group: Option<String>,
    pub limit: Option<i64>,
}

/// GET /api/shell-history
pub async fn get_shell_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ShellHistoryQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let machine = authenticate(&headers, &state.db)?;
    let after = query.after_timestamp.unwrap_or(0);
    let group = query.group.as_deref().unwrap_or("default");
    let limit = query.limit.unwrap_or(100).min(1000);

    if !machine.groups.contains(&group.to_string()) {
        return Err(err(
            StatusCode::FORBIDDEN,
            &format!("Machine does not belong to group '{}'", group),
        ));
    }

    let entries = state
        .db
        .get_history_after_timestamp(after, group, limit)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    let has_more = entries.len() as i64 == limit;
    let count = entries.len();

    Ok(Json(serde_json::json!({
        "entries": entries,
        "count": count,
        "has_more": has_more,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_backup::GitBackup;
    use crate::server::build_router;
    use crate::ws::WsHub;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn get(uri: &str) -> Request<Body> {
        Request::get(uri).body(Body::empty()).unwrap()
    }

    fn get_auth(uri: &str, token: &str) -> Request<Body> {
        Request::get(uri)
            .header("authorization", auth_header(token))
            .body(Body::empty())
            .unwrap()
    }

    fn post_json(uri: &str, body: &serde_json::Value) -> Request<Body> {
        Request::post(uri)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap()
    }

    fn post_json_auth(uri: &str, token: &str, body: &serde_json::Value) -> Request<Body> {
        Request::post(uri)
            .header("content-type", "application/json")
            .header("authorization", auth_header(token))
            .body(Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap()
    }

    fn put_json_auth(uri: &str, token: &str, body: &serde_json::Value) -> Request<Body> {
        Request::put(uri)
            .header("content-type", "application/json")
            .header("authorization", auth_header(token))
            .body(Body::from(serde_json::to_vec(body).unwrap()))
            .unwrap()
    }

    fn delete_auth(uri: &str, token: &str) -> Request<Body> {
        Request::delete(uri)
            .header("authorization", auth_header(token))
            .body(Body::empty())
            .unwrap()
    }

    async fn body_json(resp: axum::http::Response<Body>) -> serde_json::Value {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn test_app() -> (axum::Router, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            shell_sync_core::db::SyncDatabase::open(dir.path().join("test.db").to_str().unwrap())
                .unwrap(),
        );
        let hub = Arc::new(WsHub::new());
        let git_dir = dir.path().join("git");
        std::fs::create_dir_all(&git_dir).unwrap();
        let git_backup = Arc::new(GitBackup::new(Arc::clone(&db), git_dir.to_str().unwrap()));
        git_backup.initialize().unwrap();
        let state = Arc::new(AppState {
            db,
            hub,
            git_backup,
        });
        (build_router(state), dir)
    }

    /// Register a machine and return its auth token.
    async fn do_register(app: &axum::Router, hostname: &str, groups: &[&str]) -> String {
        let body = serde_json::json!({ "hostname": hostname, "groups": groups });
        let resp = app
            .clone()
            .oneshot(post_json("/api/register", &body))
            .await
            .unwrap();
        let json = body_json(resp).await;
        json["auth_token"].as_str().unwrap().to_string()
    }

    fn auth_header(token: &str) -> String {
        format!("Bearer {token}")
    }

    /// Helper: register + add an alias, returning (token, alias_id)
    async fn setup_with_alias(app: &axum::Router) -> (String, i64) {
        let token = do_register(app, "test-host", &["default"]).await;
        let body = serde_json::json!({
            "name": "gs", "command": "git status", "group": "default",
        });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/aliases", &token, &body))
            .await
            .unwrap();
        let json = body_json(resp).await;
        let id = json["alias"]["id"].as_i64().unwrap();
        (token, id)
    }

    #[tokio::test]
    async fn health_200() {
        let (app, _dir) = test_app().await;
        let resp = app.oneshot(get("/api/health")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["status"], "healthy");
    }

    #[tokio::test]
    async fn register_success() {
        let (app, _dir) = test_app().await;
        let body = serde_json::json!({ "hostname": "test-host", "groups": ["default"] });
        let resp = app
            .oneshot(post_json("/api/register", &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json["machine_id"].as_str().is_some());
        assert!(json["auth_token"].as_str().is_some());
    }

    #[tokio::test]
    async fn register_empty_hostname_400() {
        let (app, _dir) = test_app().await;
        let body = serde_json::json!({ "hostname": "", "groups": ["default"] });
        let resp = app
            .oneshot(post_json("/api/register", &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_aliases_requires_auth() {
        let (app, _dir) = test_app().await;
        let resp = app.oneshot(get("/api/aliases")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_aliases_bad_token_401() {
        let (app, _dir) = test_app().await;
        let resp = app
            .oneshot(get_auth("/api/aliases", "bad-token"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn add_alias_success() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let body = serde_json::json!({
            "name": "gs", "command": "git status", "group": "default",
        });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/aliases", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["alias"]["name"], "gs");
    }

    #[tokio::test]
    async fn add_alias_empty_name_400() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let body = serde_json::json!({
            "name": "", "command": "git status", "group": "default",
        });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/aliases", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn add_alias_invalid_chars_400() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let body = serde_json::json!({
            "name": "my alias", "command": "git status", "group": "default",
        });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/aliases", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn add_alias_secret_rejected() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let body = serde_json::json!({
            "name": "db_password", "command": "echo hunter2", "group": "default",
        });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/aliases", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn add_alias_wrong_group_403() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let body = serde_json::json!({
            "name": "gs", "command": "git status", "group": "admin",
        });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/aliases", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn add_alias_duplicate_409() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let body = serde_json::json!({
            "name": "gs", "command": "git status", "group": "default",
        });
        app.clone()
            .oneshot(post_json_auth("/api/aliases", &token, &body))
            .await
            .unwrap();
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/aliases", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn update_alias_success() {
        let (app, _dir) = test_app().await;
        let (token, alias_id) = setup_with_alias(&app).await;
        let body = serde_json::json!({ "command": "git status -sb" });
        let resp = app
            .clone()
            .oneshot(put_json_auth(
                &format!("/api/aliases/{}", alias_id),
                &token,
                &body,
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["alias"]["command"], "git status -sb");
    }

    #[tokio::test]
    async fn delete_alias_success() {
        let (app, _dir) = test_app().await;
        let (token, alias_id) = setup_with_alias(&app).await;
        let resp = app
            .clone()
            .oneshot(delete_auth(&format!("/api/aliases/{}", alias_id), &token))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn delete_alias_not_found() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let resp = app
            .clone()
            .oneshot(delete_auth("/api/aliases/99999", &token))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn import_aliases_success() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let body = serde_json::json!({
            "aliases": [
                { "name": "gs", "command": "git status" },
                { "name": "gl", "command": "git log --oneline" },
                { "name": "gp", "command": "git push" },
            ],
            "group": "default",
        });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/import", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["added"], 3);
        assert_eq!(json["failed"], 0);
        assert_eq!(json["results"]["added"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn import_aliases_partial_duplicates() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;

        // Pre-add one alias so it becomes a duplicate during import
        let pre = serde_json::json!({ "name": "gs", "command": "git status", "group": "default" });
        app.clone()
            .oneshot(post_json_auth("/api/aliases", &token, &pre))
            .await
            .unwrap();

        // Import 3 aliases: gs (dup), gl (new), gp (new)
        let body = serde_json::json!({
            "aliases": [
                { "name": "gs", "command": "git status" },
                { "name": "gl", "command": "git log --oneline" },
                { "name": "gp", "command": "git push" },
            ],
            "group": "default",
        });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/import", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["added"], 2);
        assert_eq!(json["failed"], 1);

        // The failed entry should name the duplicate
        let failed = json["results"]["failed"].as_array().unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0]["name"], "gs");
        assert!(failed[0]["error"]
            .as_str()
            .unwrap()
            .contains("already exists"));
    }

    #[tokio::test]
    async fn import_aliases_empty_list() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let body = serde_json::json!({ "aliases": [], "group": "default" });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/import", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["added"], 0);
        assert_eq!(json["failed"], 0);
    }

    #[tokio::test]
    async fn import_aliases_requires_auth() {
        let (app, _dir) = test_app().await;
        let body = serde_json::json!({
            "aliases": [{ "name": "gs", "command": "git status" }],
            "group": "default",
        });
        let resp = app
            .clone()
            .oneshot(post_json("/api/import", &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn import_aliases_rejects_secrets() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let body = serde_json::json!({
            "aliases": [
                { "name": "gs", "command": "git status" },
                { "name": "db_password", "command": "echo hunter2" },
                { "name": "gl", "command": "git log" },
            ],
            "group": "default",
        });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/import", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        // Secret alias should be rejected, other two succeed
        assert_eq!(json["added"], 2);
        assert_eq!(json["failed"], 1);
        let failed = json["results"]["failed"].as_array().unwrap();
        assert_eq!(failed[0]["name"], "db_password");
        assert!(failed[0]["error"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("secret"));
    }

    #[tokio::test]
    async fn import_aliases_wrong_group_403() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let body = serde_json::json!({
            "aliases": [{ "name": "gs", "command": "git status" }],
            "group": "admin",
        });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/import", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn import_aliases_all_duplicates() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;

        // Pre-add all aliases
        for (name, cmd) in [("gs", "git status"), ("gl", "git log")] {
            let body = serde_json::json!({ "name": name, "command": cmd, "group": "default" });
            app.clone()
                .oneshot(post_json_auth("/api/aliases", &token, &body))
                .await
                .unwrap();
        }

        let body = serde_json::json!({
            "aliases": [
                { "name": "gs", "command": "git status" },
                { "name": "gl", "command": "git log" },
            ],
            "group": "default",
        });
        let resp = app
            .clone()
            .oneshot(post_json_auth("/api/import", &token, &body))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["added"], 0);
        assert_eq!(json["failed"], 2);
    }

    #[tokio::test]
    async fn get_machines_hides_tokens() {
        let (app, _dir) = test_app().await;
        let token = do_register(&app, "test-host", &["default"]).await;
        let resp = app
            .clone()
            .oneshot(get_auth("/api/machines", &token))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let machines = json["machines"].as_array().unwrap();
        for m in machines {
            assert_eq!(m["auth_token"], "***");
        }
    }
}
