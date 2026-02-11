use crate::models::*;
use rusqlite::{params, Connection, Result as SqlResult};
use std::path::Path;
use std::sync::Mutex;

/// Thread-safe database wrapper for shell-sync.
pub struct SyncDatabase {
    conn: Mutex<Connection>,
}

impl SyncDatabase {
    /// Open (or create) the database at the given path.
    pub fn open(db_path: &str) -> anyhow::Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS aliases (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                command TEXT NOT NULL,
                group_name TEXT NOT NULL DEFAULT 'default',
                created_by_machine TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                UNIQUE(name, group_name)
            );
            CREATE INDEX IF NOT EXISTS idx_aliases_group ON aliases(group_name);
            CREATE INDEX IF NOT EXISTS idx_aliases_name ON aliases(name);

            CREATE TABLE IF NOT EXISTS machines (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                machine_id TEXT NOT NULL UNIQUE,
                hostname TEXT NOT NULL,
                groups TEXT NOT NULL,
                os_type TEXT,
                auth_token TEXT NOT NULL UNIQUE,
                last_seen INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                public_key TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_machines_token ON machines(auth_token);

            CREATE TABLE IF NOT EXISTS conflicts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                alias_name TEXT NOT NULL,
                group_name TEXT NOT NULL,
                local_command TEXT NOT NULL,
                remote_command TEXT NOT NULL,
                machine_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                resolved BOOLEAN DEFAULT 0,
                resolution TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_conflicts_machine ON conflicts(machine_id);
            CREATE INDEX IF NOT EXISTS idx_conflicts_resolved ON conflicts(resolved);

            CREATE TABLE IF NOT EXISTS sync_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                machine_id TEXT NOT NULL,
                action TEXT NOT NULL,
                alias_name TEXT NOT NULL,
                alias_command TEXT,
                group_name TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_history_timestamp ON sync_history(timestamp);
            CREATE INDEX IF NOT EXISTS idx_history_machine ON sync_history(machine_id);

            CREATE TABLE IF NOT EXISTS history (
                id TEXT PRIMARY KEY,
                command TEXT NOT NULL,
                cwd TEXT NOT NULL,
                exit_code INTEGER NOT NULL DEFAULT 0,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                session_id TEXT NOT NULL,
                machine_id TEXT NOT NULL,
                hostname TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                shell TEXT NOT NULL DEFAULT 'bash',
                group_name TEXT NOT NULL DEFAULT 'default'
            );
            CREATE INDEX IF NOT EXISTS idx_hist_timestamp ON history(timestamp);
            CREATE INDEX IF NOT EXISTS idx_hist_machine ON history(machine_id);
            CREATE INDEX IF NOT EXISTS idx_hist_session ON history(session_id);
            CREATE INDEX IF NOT EXISTS idx_hist_cwd ON history(cwd);

            CREATE TABLE IF NOT EXISTS history_pending (
                id TEXT PRIMARY KEY,
                entry_json TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_version (rowid, version) VALUES (1, 2);
            ",
        )?;

        Ok(())
    }

    // ===== MACHINES =====

    pub fn register_machine(
        &self,
        machine_id: &str,
        hostname: &str,
        groups: &[String],
        os_type: &str,
        auth_token: &str,
        public_key: Option<&str>,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let groups_json = serde_json::to_string(groups)?;

        conn.execute(
            "INSERT INTO machines (machine_id, hostname, groups, os_type, auth_token, last_seen, created_at, public_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(machine_id) DO UPDATE SET
                hostname = excluded.hostname,
                groups = excluded.groups,
                os_type = excluded.os_type,
                last_seen = excluded.last_seen,
                public_key = COALESCE(excluded.public_key, machines.public_key)",
            params![machine_id, hostname, groups_json, os_type, auth_token, now, now, public_key],
        )?;

        Ok(())
    }

    pub fn get_machine_by_token(&self, auth_token: &str) -> anyhow::Result<Option<Machine>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM machines WHERE auth_token = ?1")?;
        let machine = stmt
            .query_row(params![auth_token], Self::row_to_machine)
            .optional()?;
        Ok(machine)
    }

    pub fn update_machine_last_seen(&self, machine_id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE machines SET last_seen = ?1 WHERE machine_id = ?2",
            params![now, machine_id],
        )?;
        Ok(())
    }

    pub fn get_all_machines(&self) -> anyhow::Result<Vec<Machine>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM machines")?;
        let machines = stmt
            .query_map([], Self::row_to_machine)?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(machines)
    }

    pub fn get_machines_by_group(&self, group_name: &str) -> anyhow::Result<Vec<Machine>> {
        let all = self.get_all_machines()?;
        Ok(all
            .into_iter()
            .filter(|m| m.groups.contains(&group_name.to_string()))
            .collect())
    }

    fn row_to_machine(row: &rusqlite::Row<'_>) -> SqlResult<Machine> {
        Ok(Machine {
            id: row.get(0)?,
            machine_id: row.get(1)?,
            hostname: row.get(2)?,
            groups: {
                let s: String = row.get(3)?;
                serde_json::from_str(&s).unwrap_or_default()
            },
            os_type: row.get(4)?,
            auth_token: row.get(5)?,
            last_seen: row.get(6)?,
            created_at: row.get(7)?,
            public_key: row.get(8)?,
        })
    }

    // ===== ALIASES =====

    pub fn add_alias(
        &self,
        name: &str,
        command: &str,
        group_name: &str,
        created_by_machine: &str,
    ) -> anyhow::Result<Alias> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();

        let result = conn.execute(
            "INSERT INTO aliases (name, command, group_name, created_by_machine, created_at, updated_at, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
            params![name, command, group_name, created_by_machine, now, now],
        );

        match result {
            Ok(_) => {
                let id = conn.last_insert_rowid();
                self.log_history_inner(
                    &conn,
                    created_by_machine,
                    "add",
                    name,
                    Some(command),
                    Some(group_name),
                )?;
                Ok(Alias {
                    id,
                    name: name.to_string(),
                    command: command.to_string(),
                    group_name: group_name.to_string(),
                    created_by_machine: created_by_machine.to_string(),
                    created_at: now,
                    updated_at: now,
                    version: 1,
                })
            }
            Err(e) if e.to_string().contains("UNIQUE constraint failed") => {
                anyhow::bail!("Alias '{}' already exists in group '{}'", name, group_name)
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn update_alias(
        &self,
        id: i64,
        command: &str,
        machine_id: &str,
    ) -> anyhow::Result<Option<Alias>> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();

        let changes = conn.execute(
            "UPDATE aliases SET command = ?1, updated_at = ?2, version = version + 1 WHERE id = ?3",
            params![command, now, id],
        )?;

        if changes > 0 {
            let alias = Self::get_alias_by_id_inner(&conn, id)?;
            if let Some(ref a) = alias {
                self.log_history_inner(
                    &conn,
                    machine_id,
                    "update",
                    &a.name,
                    Some(command),
                    Some(&a.group_name),
                )?;
            }
            Ok(alias)
        } else {
            Ok(None)
        }
    }

    pub fn delete_alias(&self, id: i64, machine_id: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let alias = Self::get_alias_by_id_inner(&conn, id)?;

        if let Some(alias) = alias {
            let changes = conn.execute("DELETE FROM aliases WHERE id = ?1", params![id])?;
            if changes > 0 {
                self.log_history_inner(
                    &conn,
                    machine_id,
                    "delete",
                    &alias.name,
                    Some(&alias.command),
                    Some(&alias.group_name),
                )?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn delete_alias_by_name(
        &self,
        name: &str,
        group_name: &str,
        machine_id: &str,
    ) -> anyhow::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let alias = Self::get_alias_by_name_inner(&conn, name, group_name)?;

        if let Some(alias) = alias {
            let changes = conn.execute(
                "DELETE FROM aliases WHERE name = ?1 AND group_name = ?2",
                params![name, group_name],
            )?;
            if changes > 0 {
                self.log_history_inner(
                    &conn,
                    machine_id,
                    "delete",
                    name,
                    Some(&alias.command),
                    Some(group_name),
                )?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn get_alias_by_id(&self, id: i64) -> anyhow::Result<Option<Alias>> {
        let conn = self.conn.lock().unwrap();
        Self::get_alias_by_id_inner(&conn, id)
    }

    fn get_alias_by_id_inner(conn: &Connection, id: i64) -> anyhow::Result<Option<Alias>> {
        let mut stmt = conn.prepare("SELECT * FROM aliases WHERE id = ?1")?;
        let alias = stmt.query_row(params![id], Self::row_to_alias).optional()?;
        Ok(alias)
    }

    pub fn get_alias_by_name(&self, name: &str, group_name: &str) -> anyhow::Result<Option<Alias>> {
        let conn = self.conn.lock().unwrap();
        Self::get_alias_by_name_inner(&conn, name, group_name)
    }

    fn get_alias_by_name_inner(
        conn: &Connection,
        name: &str,
        group_name: &str,
    ) -> anyhow::Result<Option<Alias>> {
        let mut stmt = conn.prepare("SELECT * FROM aliases WHERE name = ?1 AND group_name = ?2")?;
        let alias = stmt
            .query_row(params![name, group_name], Self::row_to_alias)
            .optional()?;
        Ok(alias)
    }

    pub fn get_aliases_by_groups(&self, groups: &[String]) -> anyhow::Result<Vec<Alias>> {
        let conn = self.conn.lock().unwrap();
        if groups.is_empty() {
            return Ok(vec![]);
        }

        let placeholders: String = groups
            .iter()
            .enumerate()
            .map(|(i, _)| {
                if i == 0 {
                    format!("?{}", i + 1)
                } else {
                    format!(", ?{}", i + 1)
                }
            })
            .collect();

        let sql = format!(
            "SELECT * FROM aliases WHERE group_name IN ({}) ORDER BY name",
            placeholders
        );

        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = groups
            .iter()
            .map(|g| g as &dyn rusqlite::types::ToSql)
            .collect();

        let aliases = stmt
            .query_map(params.as_slice(), Self::row_to_alias)?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(aliases)
    }

    pub fn get_all_aliases(&self) -> anyhow::Result<Vec<Alias>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM aliases ORDER BY group_name, name")?;
        let aliases = stmt
            .query_map([], Self::row_to_alias)?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(aliases)
    }

    fn row_to_alias(row: &rusqlite::Row<'_>) -> SqlResult<Alias> {
        Ok(Alias {
            id: row.get(0)?,
            name: row.get(1)?,
            command: row.get(2)?,
            group_name: row.get(3)?,
            created_by_machine: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            version: row.get(7)?,
        })
    }

    // ===== CONFLICTS =====

    pub fn create_conflict(
        &self,
        alias_name: &str,
        group_name: &str,
        local_command: &str,
        remote_command: &str,
        machine_id: &str,
    ) -> anyhow::Result<i64> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO conflicts (alias_name, group_name, local_command, remote_command, machine_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![alias_name, group_name, local_command, remote_command, machine_id, now],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_conflicts_by_machine(&self, machine_id: &str) -> anyhow::Result<Vec<Conflict>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM conflicts WHERE machine_id = ?1 AND resolved = 0 ORDER BY created_at DESC",
        )?;
        let conflicts = stmt
            .query_map(params![machine_id], |row| {
                Ok(Conflict {
                    id: row.get(0)?,
                    alias_name: row.get(1)?,
                    group_name: row.get(2)?,
                    local_command: row.get(3)?,
                    remote_command: row.get(4)?,
                    machine_id: row.get(5)?,
                    created_at: row.get(6)?,
                    resolved: row.get(7)?,
                    resolution: row.get(8)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(conflicts)
    }

    pub fn resolve_conflict(&self, conflict_id: i64, resolution: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let changes = conn.execute(
            "UPDATE conflicts SET resolved = 1, resolution = ?1 WHERE id = ?2",
            params![resolution, conflict_id],
        )?;
        Ok(changes > 0)
    }

    // ===== HISTORY =====

    fn log_history_inner(
        &self,
        conn: &Connection,
        machine_id: &str,
        action: &str,
        alias_name: &str,
        alias_command: Option<&str>,
        group_name: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO sync_history (timestamp, machine_id, action, alias_name, alias_command, group_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now, machine_id, action, alias_name, alias_command, group_name],
        )?;
        Ok(())
    }

    pub fn get_history(&self, limit: i64) -> anyhow::Result<Vec<SyncHistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT * FROM sync_history ORDER BY timestamp DESC LIMIT ?1")?;
        let entries = stmt
            .query_map(params![limit], |row| {
                Ok(SyncHistoryEntry {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    machine_id: row.get(2)?,
                    action: row.get(3)?,
                    alias_name: row.get(4)?,
                    alias_command: row.get(5)?,
                    group_name: row.get(6)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(entries)
    }

    // ===== SHELL HISTORY =====

    fn row_to_history_entry(row: &rusqlite::Row<'_>) -> SqlResult<HistoryEntry> {
        Ok(HistoryEntry {
            id: row.get(0)?,
            command: row.get(1)?,
            cwd: row.get(2)?,
            exit_code: row.get(3)?,
            duration_ms: row.get(4)?,
            session_id: row.get(5)?,
            machine_id: row.get(6)?,
            hostname: row.get(7)?,
            timestamp: row.get(8)?,
            shell: row.get(9)?,
            group_name: row.get(10)?,
        })
    }

    pub fn insert_history_entry(&self, entry: &HistoryEntry) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO history (id, command, cwd, exit_code, duration_ms, session_id, machine_id, hostname, timestamp, shell, group_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                entry.id,
                entry.command,
                entry.cwd,
                entry.exit_code,
                entry.duration_ms,
                entry.session_id,
                entry.machine_id,
                entry.hostname,
                entry.timestamp,
                entry.shell,
                entry.group_name,
            ],
        )?;
        Ok(())
    }

    pub fn insert_history_batch(&self, entries: &[HistoryEntry]) -> usize {
        let conn = self.conn.lock().unwrap();
        let mut count = 0usize;
        let tx = match conn.unchecked_transaction() {
            Ok(tx) => tx,
            Err(_) => return 0,
        };
        for entry in entries {
            let result = tx.execute(
                "INSERT OR IGNORE INTO history (id, command, cwd, exit_code, duration_ms, session_id, machine_id, hostname, timestamp, shell, group_name)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    entry.id,
                    entry.command,
                    entry.cwd,
                    entry.exit_code,
                    entry.duration_ms,
                    entry.session_id,
                    entry.machine_id,
                    entry.hostname,
                    entry.timestamp,
                    entry.shell,
                    entry.group_name,
                ],
            );
            if let Ok(changes) = result {
                count += changes;
            }
        }
        let _ = tx.commit();
        count
    }

    pub fn search_history(
        &self,
        query: &str,
        machine_id: Option<&str>,
        session_id: Option<&str>,
        cwd: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = String::from("SELECT * FROM history WHERE command LIKE ?1");
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(format!("%{}%", query))];
        let mut idx = 2;

        if let Some(mid) = machine_id {
            sql.push_str(&format!(" AND machine_id = ?{idx}"));
            param_values.push(Box::new(mid.to_string()));
            idx += 1;
        }
        if let Some(sid) = session_id {
            sql.push_str(&format!(" AND session_id = ?{idx}"));
            param_values.push(Box::new(sid.to_string()));
            idx += 1;
        }
        if let Some(c) = cwd {
            sql.push_str(&format!(" AND cwd = ?{idx}"));
            param_values.push(Box::new(c.to_string()));
            idx += 1;
        }

        sql.push_str(&format!(
            " ORDER BY timestamp DESC LIMIT ?{idx} OFFSET ?{}",
            idx + 1
        ));
        param_values.push(Box::new(limit));
        param_values.push(Box::new(offset));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let entries = stmt
            .query_map(params_ref.as_slice(), Self::row_to_history_entry)?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(entries)
    }

    pub fn get_history_after_timestamp(
        &self,
        after: i64,
        group_name: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM history WHERE timestamp > ?1 AND group_name = ?2 ORDER BY timestamp ASC LIMIT ?3",
        )?;
        let entries = stmt
            .query_map(
                params![after, group_name, limit],
                Self::row_to_history_entry,
            )?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(entries)
    }

    pub fn get_history_count(&self) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap_or(0)
    }

    pub fn delete_history_entry(&self, id: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM history WHERE id = ?1", params![id])
            .map(|changes| changes > 0)
            .unwrap_or(false)
    }

    pub fn add_history_pending(&self, entry: &HistoryEntry) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let json = serde_json::to_string(entry)?;
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT OR IGNORE INTO history_pending (id, entry_json, created_at) VALUES (?1, ?2, ?3)",
            params![entry.id, json, now],
        )?;
        Ok(())
    }

    pub fn get_pending_history(&self, limit: i64) -> anyhow::Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT entry_json FROM history_pending ORDER BY created_at ASC LIMIT ?1")?;
        let entries = stmt
            .query_map(params![limit], |row| {
                let json: String = row.get(0)?;
                Ok(json)
            })?
            .filter_map(|r| r.ok())
            .filter_map(|json| serde_json::from_str::<HistoryEntry>(&json).ok())
            .collect();
        Ok(entries)
    }

    pub fn remove_pending_history(&self, ids: &[String]) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        for id in ids {
            conn.execute("DELETE FROM history_pending WHERE id = ?1", params![id])?;
        }
        Ok(())
    }

    /// Expose the inner connection mutex for direct SQL queries (e.g. stats).
    pub fn raw_connection(&self) -> &Mutex<Connection> {
        &self.conn
    }

    pub fn get_machine_by_id(&self, machine_id: &str) -> anyhow::Result<Option<Machine>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM machines WHERE machine_id = ?1")?;
        let machine = stmt
            .query_row(params![machine_id], Self::row_to_machine)
            .optional()?;
        Ok(machine)
    }
}

/// Extension trait for converting `rusqlite::Result<T>` to `Option<T>`.
trait OptionalExt<T> {
    fn optional(self) -> SqlResult<Option<T>>;
}

impl<T> OptionalExt<T> for SqlResult<T> {
    fn optional(self) -> SqlResult<Option<T>> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (SyncDatabase, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = SyncDatabase::open(dir.path().join("test.db").to_str().unwrap()).unwrap();
        (db, dir)
    }

    fn seed_machine(db: &SyncDatabase, id: &str) -> String {
        let token = format!("tok-{id}");
        db.register_machine(
            id,
            &format!("host-{id}"),
            &["default".into()],
            "macos",
            &token,
            None,
        )
        .unwrap();
        token
    }

    // ===== Machine tests =====

    #[test]
    fn register_and_get_by_token() {
        let (db, _dir) = setup();
        let token = seed_machine(&db, "m1");
        let machine = db.get_machine_by_token(&token).unwrap().unwrap();
        assert_eq!(machine.machine_id, "m1");
        assert_eq!(machine.hostname, "host-m1");
        assert_eq!(machine.groups, vec!["default".to_string()]);
        assert_eq!(machine.os_type, Some("macos".to_string()));
        assert_eq!(machine.auth_token, token);
    }

    #[test]
    fn get_by_token_unknown_returns_none() {
        let (db, _dir) = setup();
        assert!(db.get_machine_by_token("nonexistent").unwrap().is_none());
    }

    #[test]
    fn register_upsert_updates_hostname_groups() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        // Re-register with different hostname and groups — note: token is ignored on upsert
        // because ON CONFLICT updates hostname/groups but not auth_token
        db.register_machine(
            "m1",
            "new-host",
            &["work".into(), "ops".into()],
            "linux",
            "tok-new",
            None,
        )
        .unwrap();
        let machine = db.get_machine_by_token("tok-m1").unwrap().unwrap();
        assert_eq!(machine.hostname, "new-host");
        assert_eq!(machine.groups, vec!["work".to_string(), "ops".to_string()]);
    }

    #[test]
    fn get_all_machines() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        seed_machine(&db, "m2");
        seed_machine(&db, "m3");
        assert_eq!(db.get_all_machines().unwrap().len(), 3);
    }

    #[test]
    fn get_machines_by_group() {
        let (db, _dir) = setup();
        db.register_machine("m1", "h1", &["default".into()], "macos", "t1", None)
            .unwrap();
        db.register_machine("m2", "h2", &["work".into()], "linux", "t2", None)
            .unwrap();
        db.register_machine(
            "m3",
            "h3",
            &["default".into(), "work".into()],
            "macos",
            "t3",
            None,
        )
        .unwrap();

        let default_machines = db.get_machines_by_group("default").unwrap();
        assert_eq!(default_machines.len(), 2);

        let work_machines = db.get_machines_by_group("work").unwrap();
        assert_eq!(work_machines.len(), 2);

        let empty = db.get_machines_by_group("nonexistent").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn update_last_seen() {
        let (db, _dir) = setup();
        let token = seed_machine(&db, "m1");
        let before = db.get_machine_by_token(&token).unwrap().unwrap().last_seen;
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.update_machine_last_seen("m1").unwrap();
        let after = db.get_machine_by_token(&token).unwrap().unwrap().last_seen;
        assert!(after >= before);
    }

    // ===== Alias tests =====

    #[test]
    fn add_alias_returns_correct_fields() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        let alias = db.add_alias("gs", "git status", "default", "m1").unwrap();
        assert!(alias.id > 0);
        assert_eq!(alias.version, 1);
        assert_eq!(alias.name, "gs");
        assert_eq!(alias.command, "git status");
        assert_eq!(alias.group_name, "default");
    }

    #[test]
    fn add_alias_logs_history() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        db.add_alias("gs", "git status", "default", "m1").unwrap();
        let history = db.get_history(10).unwrap();
        assert!(!history.is_empty());
        assert_eq!(history[0].action, "add");
        assert_eq!(history[0].alias_name, "gs");
    }

    #[test]
    fn add_alias_duplicate_fails() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        db.add_alias("gs", "git status", "default", "m1").unwrap();
        let err = db
            .add_alias("gs", "git status -sb", "default", "m1")
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn add_alias_same_name_different_group() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        db.add_alias("gs", "git status", "default", "m1").unwrap();
        db.add_alias("gs", "git stash", "work", "m1").unwrap();
        let a1 = db.get_alias_by_name("gs", "default").unwrap().unwrap();
        let a2 = db.get_alias_by_name("gs", "work").unwrap().unwrap();
        assert_eq!(a1.command, "git status");
        assert_eq!(a2.command, "git stash");
    }

    #[test]
    fn get_alias_by_id() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        let alias = db.add_alias("gs", "git status", "default", "m1").unwrap();
        let fetched = db.get_alias_by_id(alias.id).unwrap().unwrap();
        assert_eq!(fetched.name, "gs");
        assert_eq!(fetched.command, "git status");
    }

    #[test]
    fn get_alias_by_id_missing() {
        let (db, _dir) = setup();
        assert!(db.get_alias_by_id(99999).unwrap().is_none());
    }

    #[test]
    fn get_alias_by_name() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        db.add_alias("gs", "git status", "default", "m1").unwrap();
        let alias = db.get_alias_by_name("gs", "default").unwrap().unwrap();
        assert_eq!(alias.command, "git status");
    }

    #[test]
    fn get_alias_by_name_wrong_group() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        db.add_alias("gs", "git status", "default", "m1").unwrap();
        assert!(db.get_alias_by_name("gs", "work").unwrap().is_none());
    }

    #[test]
    fn update_alias_changes_command_and_version() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        let alias = db.add_alias("gs", "git status", "default", "m1").unwrap();
        let updated = db
            .update_alias(alias.id, "git status -sb", "m1")
            .unwrap()
            .unwrap();
        assert_eq!(updated.version, 2);
        assert_eq!(updated.command, "git status -sb");
    }

    #[test]
    fn update_alias_nonexistent() {
        let (db, _dir) = setup();
        assert!(db.update_alias(99999, "cmd", "m1").unwrap().is_none());
    }

    #[test]
    fn delete_alias_removes_and_logs() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        let alias = db.add_alias("gs", "git status", "default", "m1").unwrap();
        assert!(db.delete_alias(alias.id, "m1").unwrap());
        assert!(db.get_alias_by_id(alias.id).unwrap().is_none());
        let history = db.get_history(10).unwrap();
        assert!(history
            .iter()
            .any(|h| h.action == "delete" && h.alias_name == "gs"));
    }

    #[test]
    fn delete_alias_nonexistent() {
        let (db, _dir) = setup();
        assert!(!db.delete_alias(99999, "m1").unwrap());
    }

    #[test]
    fn delete_alias_by_name() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        db.add_alias("gs", "git status", "default", "m1").unwrap();
        assert!(db.delete_alias_by_name("gs", "default", "m1").unwrap());
        assert!(db.get_alias_by_name("gs", "default").unwrap().is_none());
    }

    // ===== Group filtering tests =====

    #[test]
    fn get_aliases_by_groups_single() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        db.add_alias("gs", "git status", "default", "m1").unwrap();
        db.add_alias("dc", "docker-compose", "work", "m1").unwrap();
        let result = db.get_aliases_by_groups(&["default".into()]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "gs");
    }

    #[test]
    fn get_aliases_by_groups_multiple() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        db.add_alias("gs", "git status", "default", "m1").unwrap();
        db.add_alias("dc", "docker-compose", "work", "m1").unwrap();
        let result = db
            .get_aliases_by_groups(&["default".into(), "work".into()])
            .unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn get_aliases_by_groups_empty() {
        let (db, _dir) = setup();
        let result = db.get_aliases_by_groups(&[]).unwrap();
        assert!(result.is_empty());
    }

    // ===== Conflict tests =====

    #[test]
    fn create_conflict_returns_id() {
        let (db, _dir) = setup();
        let id = db
            .create_conflict("gs", "default", "git status", "git status -sb", "m1")
            .unwrap();
        assert!(id > 0);
    }

    #[test]
    fn get_conflicts_unresolved_only() {
        let (db, _dir) = setup();
        let c1 = db
            .create_conflict("gs", "default", "cmd1", "cmd2", "m1")
            .unwrap();
        let _c2 = db
            .create_conflict("dc", "default", "cmd3", "cmd4", "m1")
            .unwrap();
        db.resolve_conflict(c1, "keep_local").unwrap();
        let conflicts = db.get_conflicts_by_machine("m1").unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].alias_name, "dc");
    }

    #[test]
    fn get_conflicts_wrong_machine() {
        let (db, _dir) = setup();
        db.create_conflict("gs", "default", "cmd1", "cmd2", "m1")
            .unwrap();
        let conflicts = db.get_conflicts_by_machine("nonexistent").unwrap();
        assert!(conflicts.is_empty());
    }

    #[test]
    fn resolve_conflict() {
        let (db, _dir) = setup();
        let id = db
            .create_conflict("gs", "default", "cmd1", "cmd2", "m1")
            .unwrap();
        assert!(db.resolve_conflict(id, "keep_remote").unwrap());
        let conflicts = db.get_conflicts_by_machine("m1").unwrap();
        assert!(conflicts.is_empty());
    }

    // ===== History tests =====

    #[test]
    fn history_respects_limit() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        for i in 0..5 {
            db.add_alias(&format!("a{i}"), &format!("cmd{i}"), "default", "m1")
                .unwrap();
        }
        let history = db.get_history(3).unwrap();
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn history_ordered_desc() {
        let (db, _dir) = setup();
        seed_machine(&db, "m1");
        db.add_alias("first", "cmd1", "default", "m1").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        db.add_alias("second", "cmd2", "default", "m1").unwrap();
        let history = db.get_history(10).unwrap();
        assert_eq!(history[0].alias_name, "second");
        assert_eq!(history[1].alias_name, "first");
    }
}
