# API Import Tests Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add comprehensive tests for the `/api/import` endpoint, including tests that expose two missing validations (secret detection and group membership), then fix them.

**Architecture:** TDD approach — write failing tests that document expected behavior first, then fix the import handler to match `add_alias` validation parity. Tests use the existing `test_app()` / `do_register()` / `post_json_auth()` / `body_json()` helpers in the `api.rs` test module.

**Tech Stack:** Rust, axum, tower::ServiceExt (oneshot), tokio::test, serde_json

---

### Task 1: Basic import success test

**Files:**
- Modify: `shell-sync/crates/shell-sync-server/src/api.rs` (append to `mod tests`)

**Step 1: Write the test**

Add this test at the end of the `mod tests` block (before the closing `}`):

```rust
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
    let resp = app.clone().oneshot(post_json_auth("/api/import", &token, &body)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["added"], 3);
    assert_eq!(json["failed"], 0);
    assert_eq!(json["results"]["added"].as_array().unwrap().len(), 3);
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test --package shell-sync-server import_aliases_success -- --nocapture`
Expected: PASS

**Step 3: Commit**

```bash
git add shell-sync/crates/shell-sync-server/src/api.rs
git commit -m "test: add basic import success test"
```

---

### Task 2: Import with duplicates (mixed success/failure) test

**Files:**
- Modify: `shell-sync/crates/shell-sync-server/src/api.rs` (append to `mod tests`)

**Step 1: Write the test**

```rust
#[tokio::test]
async fn import_aliases_partial_duplicates() {
    let (app, _dir) = test_app().await;
    let token = do_register(&app, "test-host", &["default"]).await;

    // Pre-add one alias so it becomes a duplicate during import
    let pre = serde_json::json!({ "name": "gs", "command": "git status", "group": "default" });
    app.clone().oneshot(post_json_auth("/api/aliases", &token, &pre)).await.unwrap();

    // Import 3 aliases: gs (dup), gl (new), gp (new)
    let body = serde_json::json!({
        "aliases": [
            { "name": "gs", "command": "git status" },
            { "name": "gl", "command": "git log --oneline" },
            { "name": "gp", "command": "git push" },
        ],
        "group": "default",
    });
    let resp = app.clone().oneshot(post_json_auth("/api/import", &token, &body)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["added"], 2);
    assert_eq!(json["failed"], 1);

    // The failed entry should name the duplicate
    let failed = json["results"]["failed"].as_array().unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["name"], "gs");
    assert!(failed[0]["error"].as_str().unwrap().contains("already exists"));
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test --package shell-sync-server import_aliases_partial -- --nocapture`
Expected: PASS (the current implementation already handles this correctly by catching DB errors)

**Step 3: Commit**

```bash
git add shell-sync/crates/shell-sync-server/src/api.rs
git commit -m "test: add import with partial duplicates test"
```

---

### Task 3: Import empty list test

**Files:**
- Modify: `shell-sync/crates/shell-sync-server/src/api.rs` (append to `mod tests`)

**Step 1: Write the test**

```rust
#[tokio::test]
async fn import_aliases_empty_list() {
    let (app, _dir) = test_app().await;
    let token = do_register(&app, "test-host", &["default"]).await;
    let body = serde_json::json!({ "aliases": [], "group": "default" });
    let resp = app.clone().oneshot(post_json_auth("/api/import", &token, &body)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["added"], 0);
    assert_eq!(json["failed"], 0);
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test --package shell-sync-server import_aliases_empty -- --nocapture`
Expected: PASS

**Step 3: Commit**

```bash
git add shell-sync/crates/shell-sync-server/src/api.rs
git commit -m "test: add empty import list test"
```

---

### Task 4: Import requires authentication test

**Files:**
- Modify: `shell-sync/crates/shell-sync-server/src/api.rs` (append to `mod tests`)

**Step 1: Write the test**

```rust
#[tokio::test]
async fn import_aliases_requires_auth() {
    let (app, _dir) = test_app().await;
    let body = serde_json::json!({
        "aliases": [{ "name": "gs", "command": "git status" }],
        "group": "default",
    });
    let resp = app.clone().oneshot(post_json("/api/import", &body)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test --package shell-sync-server import_aliases_requires -- --nocapture`
Expected: PASS

**Step 3: Commit**

```bash
git add shell-sync/crates/shell-sync-server/src/api.rs
git commit -m "test: add import auth requirement test"
```

---

### Task 5: Import rejects secrets (write failing test, then fix handler)

This is where it gets interesting. The current `import_aliases` handler does NOT check for secrets — but `add_alias` does. This test documents that parity should exist.

**Files:**
- Modify: `shell-sync/crates/shell-sync-server/src/api.rs` (handler + test)

**Step 1: Write the failing test**

```rust
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
    let resp = app.clone().oneshot(post_json_auth("/api/import", &token, &body)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    // Secret alias should be rejected, other two succeed
    assert_eq!(json["added"], 2);
    assert_eq!(json["failed"], 1);
    let failed = json["results"]["failed"].as_array().unwrap();
    assert_eq!(failed[0]["name"], "db_password");
    assert!(failed[0]["error"].as_str().unwrap().to_lowercase().contains("secret"));
}
```

**Step 2: Run test to verify it FAILS**

Run: `cargo test --package shell-sync-server import_aliases_rejects_secrets -- --nocapture`
Expected: FAIL — currently `db_password` will be added successfully (3 added, 0 failed)

**Step 3: Fix the import handler to check for secrets**

In `import_aliases` (api.rs), replace the loop body:

**Current code (lines ~357-364):**
```rust
for import_alias in &body.aliases {
    match state
        .db
        .add_alias(&import_alias.name, &import_alias.command, &body.group, &machine.machine_id)
    {
        Ok(alias) => added.push(alias),
        Err(e) => failed.push(serde_json::json!({ "name": import_alias.name, "error": e.to_string() })),
    }
}
```

**Replace with:**
```rust
for import_alias in &body.aliases {
    if check_for_secrets(&import_alias.name, &import_alias.command) {
        failed.push(serde_json::json!({
            "name": import_alias.name,
            "error": "Potential secret detected in alias. Secrets should not be synced."
        }));
        continue;
    }
    match state
        .db
        .add_alias(&import_alias.name, &import_alias.command, &body.group, &machine.machine_id)
    {
        Ok(alias) => added.push(alias),
        Err(e) => failed.push(serde_json::json!({ "name": import_alias.name, "error": e.to_string() })),
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --package shell-sync-server import_aliases_rejects_secrets -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add shell-sync/crates/shell-sync-server/src/api.rs
git commit -m "fix: add secret detection to bulk import endpoint"
```

---

### Task 6: Import rejects wrong group (write failing test, then fix handler)

Same gap as secrets — `add_alias` checks group membership but `import_aliases` doesn't.

**Files:**
- Modify: `shell-sync/crates/shell-sync-server/src/api.rs` (handler + test)

**Step 1: Write the failing test**

```rust
#[tokio::test]
async fn import_aliases_wrong_group_403() {
    let (app, _dir) = test_app().await;
    let token = do_register(&app, "test-host", &["default"]).await;
    let body = serde_json::json!({
        "aliases": [{ "name": "gs", "command": "git status" }],
        "group": "admin",
    });
    let resp = app.clone().oneshot(post_json_auth("/api/import", &token, &body)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
```

**Step 2: Run test to verify it FAILS**

Run: `cargo test --package shell-sync-server import_aliases_wrong_group -- --nocapture`
Expected: FAIL — currently returns 200 OK (aliases get inserted into "admin" group without check)

**Step 3: Fix the import handler to check group membership**

In `import_aliases` (api.rs), add this check right after `let machine = authenticate(...)`:

```rust
if !machine.groups.contains(&body.group) {
    return Err(err(
        StatusCode::FORBIDDEN,
        &format!("Machine does not belong to group '{}'", body.group),
    ));
}
```

The full handler top should now read:
```rust
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
    // ... rest unchanged
```

**Step 4: Run test to verify it passes**

Run: `cargo test --package shell-sync-server import_aliases_wrong_group -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add shell-sync/crates/shell-sync-server/src/api.rs
git commit -m "fix: add group membership check to bulk import endpoint"
```

---

### Task 7: Import with all duplicates test

Verifies the edge case where every alias in the batch already exists.

**Files:**
- Modify: `shell-sync/crates/shell-sync-server/src/api.rs` (append to `mod tests`)

**Step 1: Write the test**

```rust
#[tokio::test]
async fn import_aliases_all_duplicates() {
    let (app, _dir) = test_app().await;
    let token = do_register(&app, "test-host", &["default"]).await;

    // Pre-add all aliases
    for (name, cmd) in [("gs", "git status"), ("gl", "git log")] {
        let body = serde_json::json!({ "name": name, "command": cmd, "group": "default" });
        app.clone().oneshot(post_json_auth("/api/aliases", &token, &body)).await.unwrap();
    }

    let body = serde_json::json!({
        "aliases": [
            { "name": "gs", "command": "git status" },
            { "name": "gl", "command": "git log" },
        ],
        "group": "default",
    });
    let resp = app.clone().oneshot(post_json_auth("/api/import", &token, &body)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["added"], 0);
    assert_eq!(json["failed"], 2);
}
```

**Step 2: Run test to verify it passes**

Run: `cargo test --package shell-sync-server import_aliases_all_dup -- --nocapture`
Expected: PASS

**Step 3: Commit**

```bash
git add shell-sync/crates/shell-sync-server/src/api.rs
git commit -m "test: add import all-duplicates edge case test"
```

---

### Task 8: Run full test suite and verify no regressions

**Step 1: Run all server tests**

Run: `cargo test --package shell-sync-server -- --nocapture`
Expected: All tests pass (existing + new)

**Step 2: Run all workspace tests**

Run: `cargo test --workspace`
Expected: All tests pass across all crates

**Step 3: Commit (if any formatting or cleanup needed)**

```bash
cargo fmt --package shell-sync-server
git add -A
git commit -m "style: format after import test additions"
```
