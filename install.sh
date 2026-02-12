#!/bin/sh
# Shell Sync installer
# Usage: curl -fsSL https://raw.githubusercontent.com/oshabana/shell-sync/master/install.sh | sh
set -e

REPO="oshabana/shell-sync"
BINARY="shell-sync"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

info() { printf '\033[1;34m%s\033[0m\n' "$1"; }
warn() { printf '\033[1;33m%s\033[0m\n' "$1"; }
error() { printf '\033[1;31m%s\033[0m\n' "$1"; exit 1; }

# Use sudo only when not already root
SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  SUDO="sudo"
fi

# Detect OS and architecture
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) error "Unsupported architecture: $ARCH" ;;
esac

case "$OS" in
  linux) TARGET="${ARCH}-unknown-linux-gnu" ;;
  darwin) TARGET="${ARCH}-apple-darwin" ;;
  *) error "Unsupported OS: $OS" ;;
esac

info "Detected system: ${OS}/${ARCH} (${TARGET})"

# ---------------------------------------------------------------------------
# Strategy 1: Try downloading a pre-built binary from GitHub Releases
# ---------------------------------------------------------------------------
try_prebuilt() {
  LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
    | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/') || true

  if [ -z "$LATEST" ]; then
    return 1
  fi

  DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST}/${BINARY}-${TARGET}.tar.gz"

  TMPDIR=$(mktemp -d)
  trap 'rm -rf "$TMPDIR"' EXIT

  if curl -fsSL "$DOWNLOAD_URL" -o "${TMPDIR}/${BINARY}.tar.gz" 2>/dev/null; then
    tar -xzf "${TMPDIR}/${BINARY}.tar.gz" -C "$TMPDIR"
    mkdir -p "$INSTALL_DIR"
    cp "${TMPDIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
    chmod +x "${INSTALL_DIR}/${BINARY}"
    info "Installed shell-sync ${LATEST} from pre-built binary."
    return 0
  fi

  return 1
}

# ---------------------------------------------------------------------------
# Strategy 2: Build from source with cargo
# ---------------------------------------------------------------------------
has_working_cc() {
  # Check that a C compiler exists AND can actually compile something.
  # command -v alone is not enough -- the binary may be missing or broken.
  CC_BIN=""
  for c in cc gcc clang; do
    if command -v "$c" >/dev/null 2>&1; then
      CC_BIN="$c"
      break
    fi
  done

  [ -z "$CC_BIN" ] && return 1

  # Smoke-test: try to compile a trivial program
  TESTDIR=$(mktemp -d)
  printf 'int main(){return 0;}\n' > "$TESTDIR/test.c"
  if "$CC_BIN" -o "$TESTDIR/test" "$TESTDIR/test.c" >/dev/null 2>&1; then
    rm -rf "$TESTDIR"
    return 0
  fi
  rm -rf "$TESTDIR"
  return 1
}

install_build_deps() {
  if [ "$OS" = "linux" ]; then
    if command -v apt-get >/dev/null 2>&1; then
      info "Installing build tools via apt..."
      $SUDO apt-get update -qq
      $SUDO apt-get install -y -qq build-essential pkg-config libssl-dev git
    elif command -v dnf >/dev/null 2>&1; then
      info "Installing build tools via dnf..."
      $SUDO dnf install -y gcc make pkg-config openssl-devel git
    elif command -v yum >/dev/null 2>&1; then
      info "Installing build tools via yum..."
      $SUDO yum install -y gcc make pkgconfig openssl-devel git
    elif command -v pacman >/dev/null 2>&1; then
      info "Installing build tools via pacman..."
      $SUDO pacman -Sy --noconfirm base-devel openssl git
    elif command -v apk >/dev/null 2>&1; then
      info "Installing build tools via apk..."
      $SUDO apk add build-base pkgconf openssl-dev git
    else
      error "Could not detect package manager. Please install a C compiler (gcc), make, and git manually."
    fi
  elif [ "$OS" = "darwin" ]; then
    if ! xcode-select -p >/dev/null 2>&1; then
      info "Installing Xcode Command Line Tools..."
      xcode-select --install 2>/dev/null || true
      error "Please finish the Xcode CLT install prompt, then re-run this script."
    fi
  fi
}

ensure_build_deps() {
  NEED_INSTALL=false

  if ! has_working_cc; then
    warn "No working C compiler found."
    NEED_INSTALL=true
  fi

  if ! command -v make >/dev/null 2>&1; then
    warn "make not found."
    NEED_INSTALL=true
  fi

  if ! command -v git >/dev/null 2>&1; then
    warn "git not found."
    NEED_INSTALL=true
  fi

  if [ "$NEED_INSTALL" = true ]; then
    echo ""
    install_build_deps
    echo ""

    # Verify the compiler works after install
    if ! has_working_cc; then
      error "Build tools installation failed. Please install gcc, make, and git manually."
    fi
    info "Build dependencies installed."
  fi
}

ensure_rust() {
  if command -v cargo >/dev/null 2>&1; then
    return 0
  fi

  warn "Rust toolchain not found. Installing via rustup..."
  if ! command -v curl >/dev/null 2>&1; then
    error "curl is required to install Rust. Install curl and re-run."
  fi

  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --quiet
  . "$HOME/.cargo/env"

  if ! command -v cargo >/dev/null 2>&1; then
    error "Rust installation failed. Install manually: https://rustup.rs"
  fi
  info "Rust installed successfully."
}

build_from_source() {
  ensure_build_deps
  ensure_rust

  info "Building shell-sync from source (this may take a few minutes)..."

  TMPDIR=$(mktemp -d)
  trap 'rm -rf "$TMPDIR"' EXIT

  git clone --depth 1 "https://github.com/${REPO}.git" "$TMPDIR/shell-sync"
  cd "$TMPDIR/shell-sync"
  cargo build --release

  mkdir -p "$INSTALL_DIR"

  # Install all binaries from the workspace
  for bin in shell-sync shell-sync-server shell-sync-client shell-sync-tui; do
    if [ -f "target/release/$bin" ]; then
      cp "target/release/$bin" "${INSTALL_DIR}/$bin"
      chmod +x "${INSTALL_DIR}/$bin"
    fi
  done

  info "Built and installed shell-sync from source."
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
info "Installing shell-sync..."
echo ""

if try_prebuilt; then
  : # success via pre-built binary
else
  warn "No pre-built binary available for ${TARGET}. Building from source..."
  echo ""
  build_from_source
fi

# Ensure INSTALL_DIR is on PATH
case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    warn "${INSTALL_DIR} is not in your PATH."
    echo ""
    echo "Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
    echo ""
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    echo ""
    ;;
esac

# ---------------------------------------------------------------------------
# Install systemd services (Linux only)
# ---------------------------------------------------------------------------
install_systemd_services() {
  if [ "$OS" != "linux" ]; then
    return
  fi

  if ! command -v systemctl >/dev/null 2>&1; then
    return
  fi

  SHELL_SYNC_BIN="$(command -v shell-sync 2>/dev/null || echo "${INSTALL_DIR}/shell-sync")"

  # Determine the right systemd directory and user flags
  if [ "$(id -u)" -eq 0 ]; then
    SYSTEMD_DIR="/etc/systemd/system"
    SYSTEMCTL="systemctl"
  else
    SYSTEMD_DIR="$HOME/.config/systemd/user"
    SYSTEMCTL="systemctl --user"
    mkdir -p "$SYSTEMD_DIR"
  fi

  # Server service
  cat > "$SYSTEMD_DIR/shell-sync-server.service" <<SVCEOF
[Unit]
Description=Shell Sync Server
After=network.target

[Service]
Type=simple
ExecStart=${SHELL_SYNC_BIN} serve --foreground
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
SVCEOF

  # Client service
  cat > "$SYSTEMD_DIR/shell-sync-client.service" <<SVCEOF
[Unit]
Description=Shell Sync Client
After=network.target

[Service]
Type=simple
ExecStart=${SHELL_SYNC_BIN} connect --foreground
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
SVCEOF

  $SYSTEMCTL daemon-reload
  info "Systemd services installed."
  echo ""
  echo "  Enable the server (this machine hosts aliases):"
  echo "    $SYSTEMCTL enable --now shell-sync-server"
  echo ""
  echo "  Enable the client (this machine syncs aliases):"
  echo "    $SYSTEMCTL enable --now shell-sync-client"
  echo ""
  echo "  Enable both (this machine does both):"
  echo "    $SYSTEMCTL enable --now shell-sync-server shell-sync-client"
}

install_systemd_services

echo ""
info "Installation complete!"
echo ""
echo "Quick start:"
echo "  1. Start the server:   shell-sync serve --foreground"
echo "  2. Register a client:  shell-sync register"
echo "  3. Connect:            shell-sync connect --foreground"
echo "  4. Add an alias:       shell-sync add gst 'git status'"
echo ""
echo "Run 'shell-sync --help' for all commands."
echo "Docs: https://github.com/${REPO}"
