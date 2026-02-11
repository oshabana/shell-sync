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
                created_at INTEGER NOT NULL
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

            CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO schema_version (rowid, version) VALUES (1, 1);
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
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let groups_json = serde_json::to_string(groups)?;

        conn.execute(
            "INSERT INTO machines (machine_id, hostname, groups, os_type, auth_token, last_seen, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(machine_id) DO UPDATE SET
                hostname = excluded.hostname,
                groups = excluded.groups,
                os_type = excluded.os_type,
                last_seen = excluded.last_seen",
            params![machine_id, hostname, groups_json, os_type, auth_token, now, now],
        )?;

        Ok(())
    }

    pub fn get_machine_by_token(&self, auth_token: &str) -> anyhow::Result<Option<Machine>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM machines WHERE auth_token = ?1")?;
        let machine = stmt
            .query_row(params![auth_token], |row| {
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
                })
            })
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
            .query_map([], |row| {
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
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(machines)
    }

    pub fn get_machines_by_group(&self, group_name: &str) -> anyhow::Result<Vec<Machine>> {
        let all = self.get_all_machines()?;
        Ok(all.into_iter().filter(|m| m.groups.contains(&group_name.to_string())).collect())
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
                self.log_history_inner(&conn, created_by_machine, "add", name, Some(command), Some(group_name))?;
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
                self.log_history_inner(&conn, machine_id, "update", &a.name, Some(command), Some(&a.group_name))?;
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

        let placeholders: String = groups.iter().enumerate().map(|(i, _)| {
            if i == 0 { format!("?{}", i + 1) } else { format!(", ?{}", i + 1) }
        }).collect();

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
        let mut stmt = conn.prepare(
            "SELECT * FROM sync_history ORDER BY timestamp DESC LIMIT ?1",
        )?;
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
