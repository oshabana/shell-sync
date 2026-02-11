#!/bin/sh
# Shell Sync installer — curl -fsSL https://raw.githubusercontent.com/.../install.sh | sh
set -e

REPO="user/helpful-stuff"
BINARY="shell-sync"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Detect OS and architecture
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

case "$OS" in
  linux) TARGET="${ARCH}-unknown-linux-gnu" ;;
  darwin) TARGET="${ARCH}-apple-darwin" ;;
  *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

echo "Detecting system: ${OS}/${ARCH}"
echo "Target: ${TARGET}"

# Get latest release
LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
if [ -z "$LATEST" ]; then
  echo "Could not determine latest release. Using 'latest'."
  LATEST="latest"
fi

echo "Installing shell-sync ${LATEST}..."

DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST}/${BINARY}-${TARGET}.tar.gz"

# Download and install
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

curl -fsSL "$DOWNLOAD_URL" -o "${TMPDIR}/${BINARY}.tar.gz"
tar -xzf "${TMPDIR}/${BINARY}.tar.gz" -C "$TMPDIR"

# Install binary
if [ -w "$INSTALL_DIR" ]; then
  cp "${TMPDIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
else
  echo "Installing to ${INSTALL_DIR} (requires sudo)..."
  sudo cp "${TMPDIR}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
fi

chmod +x "${INSTALL_DIR}/${BINARY}"

echo ""
echo "shell-sync installed to ${INSTALL_DIR}/${BINARY}"
echo ""
echo "Quick start:"
echo "  Server:  shell-sync serve --foreground"
echo "  Client:  shell-sync register && shell-sync connect"
echo ""
echo "Run 'shell-sync --help' for all commands."
