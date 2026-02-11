# Shell Sync: Atuin-Inspired Features Design

**Date:** 2026-02-11
**Goal:** Evolve shell-sync from an alias sync tool into a full shell state sync platform that replaces Atuin. Single binary, zero-knowledge encryption, real-time sync.

## What Exists Today

Shell-sync is a Rust workspace (4 crates) that syncs shell aliases across machines:
- `shell-sync-core` — models, DB, config, protocol, secret detection, shell integration
- `shell-sync-server` — Axum REST API + WebSocket hub + mDNS + git backup
- `shell-sync-client` — daemon, registration, mDNS discovery, offline queue, shell writer
- `shell-sync-cli` — 18 CLI commands via clap

Current capabilities: real-time alias sync, group-based filtering, mDNS auto-discovery, offline queue, conflict resolution, web UI dashboard, git backup.

## What We're Adding

Four major features, in priority order:

1. Shell history sync with rich context
2. End-to-end encryption (zero-knowledge)
3. Interactive TUI search (Ctrl+R replacement)
4. Stats and analytics

---

## 1. Shell History Sync

### Data Model

New `HistoryEntry` struct in `shell-sync-core/src/models.rs`:

```rust
pub struct HistoryEntry {
    pub id: Uuid,
    pub command: String,
    pub cwd: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub session_id: Uuid,
    pub machine_id: Uuid,
    pub hostname: String,
    pub timestamp: i64,       // unix epoch millis
    pub shell: String,        // "zsh", "bash", "fish"
}
```

### Database Schema

New tables on both client and server:

```sql
CREATE TABLE history (
    id TEXT PRIMARY KEY,
    command_encrypted BLOB NOT NULL,
    cwd_encrypted BLOB NOT NULL,
    exit_code_encrypted BLOB NOT NULL,
    duration_ms_encrypted BLOB NOT NULL,
    session_id TEXT NOT NULL,
    machine_id TEXT NOT NULL,
    hostname_encrypted BLOB NOT NULL,
    timestamp INTEGER NOT NULL,
    shell TEXT NOT NULL,
    group_name TEXT NOT NULL DEFAULT 'default',
    nonce BLOB NOT NULL
);

CREATE INDEX idx_history_timestamp ON history(timestamp);
CREATE INDEX idx_history_machine ON history(machine_id);
CREATE INDEX idx_history_session ON history(session_id);
CREATE INDEX idx_history_group ON history(group_name);
```

Client-side additional table for pending sync:

```sql
CREATE TABLE history_pending (
    id TEXT PRIMARY KEY,
    entry_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
```

### Shell Hooks

Generated shell scripts that capture every command with context. Hooks use preexec/precmd (zsh), DEBUG trap + PROMPT_COMMAND (bash), and fish_preexec/fish_postexec events (fish).

Each hook:
1. Records start time before command execution
2. Captures exit code and duration after execution
3. Sends a JSON payload to the daemon via Unix domain socket at `~/.shell-sync/sock`

Shell hook scripts are generated at:
- `shells/shell-sync.zsh`
- `shells/shell-sync.bash`
- `shells/shell-sync.fish`

### Communication: Unix Domain Socket

The client daemon listens on `~/.shell-sync/sock` for incoming history entries from shell hooks. This avoids HTTP overhead for high-frequency local writes.

New module: `shell-sync-client/src/socket_listener.rs`
- Binds Unix socket at `~/.shell-sync/sock`
- Accepts connections, reads JSON lines
- Validates and stores in local SQLite
- Adds to `history_pending` for server sync

### Sync Protocol

New WebSocket events:

Client to server:
- `history_batch` — batch of encrypted history entries (pushed every 5s or at 50 entries)
- `history_query` — request history page by timestamp range

Server to client:
- `history_sync` — new entries from other machines in the group
- `history_page` — response to history_query with pagination

### Initial Sync

On first connect, client requests all history after its last known timestamp. Server pages results in batches of 1000.

---

## 2. End-to-End Encryption

### Design Principles

- Server is zero-knowledge: stores and routes ciphertext only
- Encryption keys never leave client machines
- Group-based key sharing: machines in the same group share a symmetric key
- Key rotation supported but not required

### Key Hierarchy

```
Machine keypair (X25519)
  └── Used to encrypt/decrypt group keys during key exchange

Group symmetric key (AES-256-GCM)
  └── Used to encrypt/decrypt all data (history, aliases)
```

### Key Generation and Storage

On machine registration:
1. Generate X25519 keypair
2. Store private key at `~/.shell-sync/keys/private.key`
3. Send public key to server during registration

On group join:
1. If first machine in group: generate random 256-bit AES key
2. If joining existing group: existing member wraps group key with new machine's public key

```
~/.shell-sync/
  keys/
    private.key
    public.key
    groups/
      default.key
      dev.key
```

### Encryption Module

New module: `shell-sync-core/src/encryption.rs`

Core operations:
- `encrypt_field(group, plaintext) -> (ciphertext, nonce)` — AES-256-GCM encrypt
- `decrypt_field(group, ciphertext, nonce) -> plaintext` — AES-256-GCM decrypt
- `encrypt_history_entry(entry) -> EncryptedHistoryEntry` — encrypt all sensitive fields
- `decrypt_history_entry(entry) -> HistoryEntry` — decrypt all fields
- `encrypt_alias(alias) -> EncryptedAlias` — encrypt alias command
- `decrypt_alias(alias) -> Alias` — decrypt alias command
- `wrap_group_key(group, recipient_pubkey) -> wrapped_bytes` — X25519 key exchange
- `unwrap_group_key(wrapped) -> key_bytes` — unwrap received group key

### Key Exchange Protocol

New WebSocket messages for key negotiation:
- `key_request` — new machine requests group key, includes public key
- `key_request` event — server forwards to existing group member
- `key_response` — existing member sends wrapped key back via server

### Server-Side Changes

- `aliases` table: add `command_encrypted BLOB`, `nonce BLOB` columns
- Server stops reading alias commands; routes by group_name only
- Git backup writes encrypted blobs (content opaque to server)
- Web UI requires client-side key to view decrypted data

### What Gets Encrypted

Everything except routing metadata:
- command, cwd, hostname, exit_code, duration_ms (encrypted)
- machine_id, group_name, session_id, timestamp, shell (plaintext, needed for routing/indexing)

### Migration

Existing plaintext aliases are encrypted on first client startup after upgrade.

### Rust Crates

- `aes-gcm` 0.10 — AES-256-GCM authenticated encryption
- `x25519-dalek` 2.0 — X25519 Diffie-Hellman key exchange
- `sha2` 0.10 — SHA-256 for key derivation
- `rand` 0.8 — Secure random generation
- `base64` 0.22 — Encoding for wire format
- `zeroize` 1.7 — Secure memory wiping for keys

---

## 3. Interactive TUI Search

### Overview

Full-screen terminal UI invoked via Ctrl+R that searches across local history (already synced and decrypted in local SQLite).

### New Crate: `shell-sync-tui`

Added to workspace, depends on `shell-sync-core` for DB access and decryption.

### Search Modes

Cycled via Ctrl+R while TUI is open:

| Mode | Behavior |
|------|----------|
| Fuzzy (default) | Typo-tolerant matching via `nucleo` crate |
| Prefix | Matches from start of command |
| Fulltext | Substring match anywhere |
| Regex | Full regex support |

### Filter Modes

Cycled via Ctrl+S:

| Filter | Scope |
|--------|-------|
| Global | All history across all synced machines |
| Host | Only current machine's history |
| Session | Only current terminal session |
| Directory | Only commands run in current cwd |

### Layout

```
+-------------------------------------------------+
| > docker co_                            [fuzzy] |
| [global] 1,247 results                         |
|-------------------------------------------------|
| > docker compose up -d              0     2s    |
|   docker compose logs -f            0     -     |
|   docker compose build --no-cache   1    45s    |
|   docker container ls -a            0     1s    |
|   docker compose down               0     3s    |
|   docker compose ps                 0     1s    |
|   docker compose pull               0    12s    |
|                                                 |
|-------------------------------------------------|
| macbook ~ ~/projects/app ~ 10 min ago           |
| Ctrl+R: mode  Ctrl+S: filter  Enter: run       |
+-------------------------------------------------+
```

### Keybindings

| Key | Action |
|-----|--------|
| Type | Filter results |
| Up/Down | Navigate entries |
| Enter | Execute selected command |
| Tab | Paste into prompt for editing |
| Ctrl+R | Cycle search mode |
| Ctrl+S | Cycle filter mode |
| Ctrl+D | Delete selected entry |
| Alt+1..9 | Quick-select by number |
| Esc | Cancel and return to prompt |

### Shell Integration

Shell hooks also rebind Ctrl+R to invoke `shell-sync search --inline`. The `--inline` flag causes the TUI to output the selected command to stdout (for shell insertion) instead of executing it directly.

### Implementation

The TUI reads directly from local SQLite (decrypted on the fly). No network calls during search.

### Rust Crates

- `ratatui` 0.29 — Terminal UI framework
- `crossterm` 0.28 — Terminal backend (cross-platform)
- `nucleo` 0.5 — Fuzzy matching engine (same as helix editor)

---

## 4. Stats and Analytics

### Overview

Client-side computation over decrypted local history. Available via CLI and web UI.

### CLI Command

`shell-sync stats [--last 7d|30d|90d|1y] [--machine NAME] [--group GROUP] [--directory PATH]`

### Metrics

| Metric | Description |
|--------|-------------|
| Total commands | Count in time range |
| Unique commands | Distinct command strings |
| Success rate | Percentage with exit_code == 0 |
| Top commands | Most frequent, with count and bar chart |
| Top prefixes | First word frequency (git, docker, npm) |
| Avg duration | Mean command duration, excluding outliers > 60s |
| Activity heatmap | Commands per hour-of-day, per day-of-week |
| Per-directory | Top directories by command count |
| Per-machine | Comparison across synced machines |
| Streak | Longest consecutive days with activity |

### Stats Engine

New module: `shell-sync-core/src/stats.rs`

```rust
pub struct StatsResult {
    pub total_commands: u64,
    pub unique_commands: u64,
    pub success_rate: f64,
    pub top_commands: Vec<(String, u64)>,
    pub top_prefixes: Vec<(String, u64)>,
    pub avg_duration_ms: f64,
    pub median_duration_ms: f64,
    pub p95_duration_ms: f64,
    pub hourly_distribution: [u64; 24],
    pub daily_distribution: [u64; 7],
    pub per_directory: Vec<(String, u64)>,
    pub per_machine: Vec<(String, u64)>,
    pub streak_days: u32,
}
```

The engine decrypts entries on the fly while computing stats. Streams through entries rather than loading all into memory.

### Web UI Stats

New "Stats" page at `#stats` in the web UI.

Approach: The daemon exposes a local-only HTTP endpoint (127.0.0.1) that serves pre-computed, decrypted stats as JSON. The web UI fetches from this local proxy. No keys are needed in the browser.

---

## Build Phases

| Phase | Scope | Dependencies |
|-------|-------|-------------|
| **1** | History model, DB migration, Unix socket listener, shell hooks | None |
| **2** | History sync protocol (batch push, server store, broadcast) | Phase 1 |
| **3** | Encryption module, key generation, key exchange protocol | None (parallel with 1-2) |
| **4** | Encrypt history + aliases, migration of existing plaintext data | Phases 2 + 3 |
| **5** | TUI search (ratatui, fuzzy matching, shell Ctrl+R binding) | Phase 1 (needs local history DB) |
| **6** | Stats engine + CLI output | Phase 4 (needs decryption) |
| **7** | Web UI stats page + local proxy endpoint | Phase 6 |

Phases 1-2 and 3 can run in parallel. Phase 5 can start once Phase 1 is done.

## New Workspace Dependencies

```toml
ratatui = "0.29"
crossterm = "0.28"
nucleo = "0.5"
aes-gcm = "0.10"
x25519-dalek = { version = "2.0", features = ["static_secrets"] }
sha2 = "0.10"
rand = "0.8"
base64 = "0.22"
zeroize = { version = "1.7", features = ["derive"] }
```
