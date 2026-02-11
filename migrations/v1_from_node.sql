-- Migration from Node.js shell-sync to Rust version
-- Run: shell-sync migrate /path/to/old/sync.db
--
-- This SQL documents the schema differences. The `shell-sync migrate` command
-- handles the migration programmatically — this file is for reference only.
--
-- Schema is 100% compatible. The Rust version adds:
--   1. schema_version table for future migrations
--   2. Identical 4 tables: aliases, machines, conflicts, sync_history
--
-- Data preserved during migration:
--   - All aliases (name, command, group_name, versions)
--   - All machines (machine_id, hostname, groups, auth_token)
--   - Auth tokens preserved — clients don't need to re-register
--   - UUIDs preserved — machine_id stays the same
--
-- What changes:
--   - Server runs on single port (8888) instead of 8888 + 8889
--   - WebSocket URL changes from ws://host:8889 to ws://host:8888/ws
--   - Config format changes from JSON to TOML (~/.shell-sync/config.toml)
--   - Alias file location changes to ~/.shell-sync/aliases.sh

-- New table added by Rust version
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);
INSERT OR IGNORE INTO schema_version (rowid, version) VALUES (1, 1);
