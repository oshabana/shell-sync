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
