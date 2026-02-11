# Shell Sync

**Real-time shell alias synchronization across all your machines.**

Shell Sync keeps your shell aliases, functions, and environment configurations synchronized across multiple machines in real-time. Add an alias on your laptop, and it instantly appears on your desktop and servers.

## Features

- **Real-time sync** via WebSockets with offline support
- **End-to-end encryption** using X25519 + AES-256-GCM
- **Powerful TUI search** (Ctrl+R replacement with fuzzy matching)
- **Usage analytics** tracking command frequency and patterns
- **Web dashboard** for managing aliases across machines
- **Groups** to organize aliases by context (work, personal, ops, etc.)
- **mDNS discovery** for zero-configuration local network setup
- **Shell hooks** for automatic command history capture
- **Git backups** with automatic versioning
- **Multi-shell support** (bash, zsh, fish)

---

## Quick Start

### Installation

**Using the install script (recommended):**

```bash
curl -fsSL https://raw.githubusercontent.com/oshabana/shell-sync/master/install.sh | sh
```

This will try to download a pre-built binary first, and fall back to building from
source if none is available. If Rust is not installed, the script will install it
automatically via [rustup](https://rustup.rs).

**Or build from source manually:**

```bash
git clone https://github.com/oshabana/shell-sync.git
cd shell-sync
cargo build --release
cp target/release/shell-sync ~/.local/bin/
```

**Or install directly with cargo:**

```bash
cargo install --git https://github.com/oshabana/shell-sync.git
```

### Basic Setup

**1. Start the server** (on one machine):

```bash
shell-sync serve --foreground
```

The server will start on port 8888 and broadcast itself via mDNS.

**2. Register clients** (on all machines, including the server):

```bash
shell-sync register
shell-sync connect --foreground
```

The client will auto-discover the server on your local network.

**3. Add your first alias:**

```bash
shell-sync add gst "git status"
```

Your new alias is now available on all connected machines!

---

## Basic Usage

### Managing Aliases

```bash
# Add an alias
shell-sync add ll "ls -lah"

# Update an alias
shell-sync update ll "ls -lah --color=auto"

# Remove an alias
shell-sync rm ll

# List all aliases
shell-sync ls

# List aliases in a specific group
shell-sync ls --group work
```

### Using Groups

Groups help organize aliases by context:

```bash
# Add to a specific group
shell-sync add deploy "kubectl apply -f" --group work
shell-sync add gc "git commit -m" --group personal

# Register a machine with specific groups
shell-sync register --groups "work,personal,ops"
```

Only aliases in groups you've registered for will sync to your machine.

### Import/Export

```bash
# Import from your existing shell config
shell-sync import --file ~/.bash_aliases --group default

# Export all aliases (useful for backups)
shell-sync export > aliases-backup.txt

# Dry run to see what would be imported
shell-sync import --file ~/.zshrc --dry-run
```

### Checking Status

```bash
# Check daemon status and connection
shell-sync status

# View sync history
shell-sync history

# List all registered machines
shell-sync machines

# Check for conflicts
shell-sync conflicts
```

---

## Advanced Features

### End-to-End Encryption

Shell Sync uses **X25519 key exchange** + **AES-256-GCM** encryption to protect your aliases.

**Enabling encryption for an existing setup:**

```bash
# Migrate plaintext data to encrypted storage
shell-sync encrypt-migrate
```

**How it works:**
- Each machine generates an X25519 keypair
- Group-specific AES keys are exchanged securely
- All aliases are encrypted before transmission and storage
- Keys are stored in `~/.config/shell-sync/keys/`

### Web Dashboard

Access the web UI at `http://localhost:8888` (or your server's IP):

- View and edit aliases across all machines
- Resolve conflicts with a visual diff interface
- Monitor sync activity and history
- View usage statistics and analytics
- Manage machine registrations

**Disable the web UI:**

```bash
shell-sync serve --no-web-ui
```

### Interactive Search (TUI)

Replace your shell's Ctrl+R with a powerful fuzzy search:

```bash
# Launch interactive search
shell-sync search

# Start with a query
shell-sync search "git"
```

**Shell integration (recommended):**

Add to your `~/.zshrc` or `~/.bashrc`:

```bash
# Zsh
__shell_sync_search() {
    local result=$(shell-sync search --inline)
    if [[ -n "$result" ]]; then
        BUFFER="$result"
        CURSOR=$#BUFFER
        zle redisplay
    fi
}
zle -N __shell_sync_search
bindkey '^R' __shell_sync_search

# Bash
bind -x '"\C-r": "eval \"$(shell-sync search --inline)\""'
```

**Features:**
- Fuzzy matching with nucleo (same engine as Helix editor)
- Search by command, directory, or exit code
- Real-time results across all synced machines
- Syntax highlighting

### Usage Statistics

Track your command usage patterns:

```bash
# Show stats for the last 30 days
shell-sync stats

# Filter by time period
shell-sync stats --last 7d
shell-sync stats --last 1y
shell-sync stats --last all

# Filter by machine or group
shell-sync stats --machine laptop
shell-sync stats --group work

# Filter by directory
shell-sync stats --directory ~/projects

# Output as JSON for processing
shell-sync stats --json | jq '.top_commands[0:10]'
```

**What's tracked:**
- Command frequency and recency
- Execution duration
- Exit codes (success/failure)
- Working directory context
- Time-based patterns

### Shell Hooks for History Capture

Automatically capture every command you run:

```bash
# Install hooks for your shell
shell-sync init-hooks

# Overwrite existing hooks
shell-sync init-hooks --force
```

**What this does:**
- Installs shell-specific hooks (zsh/bash/fish)
- Captures command, exit code, duration, and directory
- Sends data to local daemon via Unix socket
- Powers the search and stats features

**Hook locations:**
- Zsh: `~/.config/shell-sync/hooks/zsh.sh` (source from `~/.zshrc`)
- Bash: `~/.config/shell-sync/hooks/bash.sh` (source from `~/.bashrc`)
- Fish: `~/.config/shell-sync/hooks/fish.fish` (copy to `~/.config/fish/conf.d/`)

### Git Backups

Automatically version your aliases with Git:

```bash
# Force a backup commit
shell-sync git-backup
```

**Automatic backups:**

The server automatically creates Git commits when aliases change. Backups are stored in the server's data directory with full history.

**Benefits:**
- Track changes over time
- Revert to previous versions
- Disaster recovery
- Audit trail

### Advanced Group Management

Groups are powerful for separating contexts:

```bash
# Work machine
shell-sync register --groups "default,work,docker,k8s"

# Personal laptop
shell-sync register --groups "default,personal,media"

# Shared server
shell-sync register --groups "default,ops,monitoring"
```

**Use cases:**
- Separate work and personal aliases
- Environment-specific configurations (dev/staging/prod)
- Team-shared aliases (everyone in "ops" group gets the same tools)
- Machine-specific groups (only DB servers get DB aliases)

### Conflict Resolution

When the same alias is modified on multiple machines:

```bash
# List conflicts
shell-sync conflicts

# Resolve via TUI (choose which version to keep)
# or use the web dashboard for a visual diff
```

### Manual Server Connection

If mDNS discovery isn't working:

```bash
# Specify server explicitly
shell-sync register --server http://192.168.1.100:8888
shell-sync connect --server http://192.168.1.100:8888
```

### Docker Deployment

```bash
# Using docker-compose
docker-compose up -d

# Or manually
docker build -t shell-sync .
docker run -p 8888:8888 -v shell-sync-data:/data shell-sync
```

---

## Configuration

### Client Config

Located at `~/.config/shell-sync/config.toml`:

```toml
[client]
machine_id = "laptop-2024"
server_url = "http://192.168.1.100:8888"
groups = ["default", "work"]

[sync]
auto_sync = true
sync_interval_secs = 30
```

### Server Config

Pass options via CLI or environment variables:

```bash
# Custom port
shell-sync serve --port 9999

# Disable features
shell-sync serve --no-mdns --no-web-ui

# Environment variable
SHELL_SYNC_PORT=9999 shell-sync serve
```

### Shell Integration

Add to your shell config (`~/.zshrc`, `~/.bashrc`, etc.):

```bash
# Source shell-sync aliases
if [ -f ~/.config/shell-sync/aliases.sh ]; then
    source ~/.config/shell-sync/aliases.sh
fi

# Source hooks (if using init-hooks)
if [ -f ~/.config/shell-sync/hooks/zsh.sh ]; then
    source ~/.config/shell-sync/hooks/zsh.sh
fi

# Ctrl+R replacement (optional)
__shell_sync_search() {
    local result=$(shell-sync search --inline)
    if [[ -n "$result" ]]; then
        BUFFER="$result"
        CURSOR=$#BUFFER
        zle redisplay
    fi
}
zle -N __shell_sync_search
bindkey '^R' __shell_sync_search
```

---

## Architecture

### How It Works

1. **Server** runs on one machine, stores encrypted aliases in SQLite
2. **Clients** connect via WebSocket, maintain local cache
3. **Changes** propagate in real-time to all connected clients
4. **Offline mode** queues changes, syncs when reconnected
5. **Encryption** happens client-side before transmission
6. **Shell integration** writes aliases to `~/.config/shell-sync/aliases.sh`

### Components

- **shell-sync-core**: Shared models, encryption, protocol
- **shell-sync-server**: Axum HTTP/WebSocket API, mDNS, git backups
- **shell-sync-client**: Sync daemon, socket listener, shell writer
- **shell-sync-cli**: Command-line interface
- **shell-sync-tui**: Interactive search UI with fuzzy matching

### Data Storage

**Server:**
- Database: `~/.local/share/shell-sync/server.db` (SQLite)
- Git backups: `~/.local/share/shell-sync/backup.git`

**Client:**
- Database: `~/.local/share/shell-sync/client.db`
- Keys: `~/.config/shell-sync/keys/`
- Aliases: `~/.config/shell-sync/aliases.sh`
- Hooks: `~/.config/shell-sync/hooks/`

### Security

- **Encryption**: X25519 key exchange + AES-256-GCM
- **Authentication**: Machine registration with unique IDs
- **Group isolation**: Keys are group-specific
- **No plaintext**: Aliases encrypted in transit and at rest (when enabled)

---

## Troubleshooting

### Daemon won't start

```bash
# Check if already running
shell-sync status

# Stop existing daemon
shell-sync stop

# Start in foreground to see logs
shell-sync connect --foreground
```

### Can't discover server

```bash
# Check server is running
# On server machine:
shell-sync status

# Manually specify server
shell-sync register --server http://SERVER_IP:8888
shell-sync connect --server http://SERVER_IP:8888
```

### Aliases not loading

```bash
# Verify aliases are synced
shell-sync ls

# Check shell integration
grep "shell-sync" ~/.zshrc  # or ~/.bashrc

# Manually source aliases
source ~/.config/shell-sync/aliases.sh
```

### Enable debug logging

```bash
RUST_LOG=debug shell-sync connect --foreground
```

---

## Migration

### From Node.js version

```bash
shell-sync migrate /path/to/old/sync.db
```

### From other sync tools

```bash
# Export from old tool to a file with format: alias_name=command
shell-sync import --file aliases.txt
```

---

## Shell Completions

Generate completions for your shell:

```bash
# Bash
shell-sync completions bash > /etc/bash_completion.d/shell-sync

# Zsh
shell-sync completions zsh > ~/.zsh/completions/_shell-sync

# Fish
shell-sync completions fish > ~/.config/fish/completions/shell-sync.fish
```

---

## Contributing

Contributions welcome! This is a Rust workspace with 5 crates. See the individual crate directories for specific documentation.

```bash
# Build
cargo build

# Run tests
cargo test

# Run with debug logging
RUST_LOG=debug cargo run -- connect --foreground
```

---

## License

MIT

---

## See Also

- [atuin](https://github.com/atuinsh/atuin) - Shell history sync (inspired some features)
- [chezmoi](https://www.chezmoi.io/) - Dotfile management
- [mackup](https://github.com/lra/mackup) - Application settings sync
