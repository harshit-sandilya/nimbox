#!/bin/sh
set -e

REPO="harshit-sandilya/nimbox"

# Detect OS + arch
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
  linux)  OS="linux" ;;
  darwin) OS="macos" ;;
  *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64)  ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "Unsupported arch: $ARCH"; exit 1 ;;
esac

# OS-specific install dir
# macOS: ~/.local/bin — user-owned, no Gatekeeper quarantine issues
# Linux: /usr/local/bin — standard system bin
if [ "$OS" = "macos" ]; then
  DEFAULT_BIN_DIR="$HOME/.local/bin"
else
  DEFAULT_BIN_DIR="/usr/local/bin"
fi

BIN_DIR="${NIMBOX_BIN_DIR:-$DEFAULT_BIN_DIR}"

# Get latest release tag
VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
  | grep '"tag_name"' | cut -d'"' -f4)

ARTIFACT="nimbox-${OS}-${ARCH}"
URL="https://github.com/$REPO/releases/download/$VERSION/$ARTIFACT"

echo "Installing nimbox $VERSION ($OS/$ARCH)..."

mkdir -p "$BIN_DIR"
curl -fsSL "$URL" -o /tmp/nimbox
chmod +x /tmp/nimbox

if [ -w "$BIN_DIR" ]; then
  mv /tmp/nimbox "$BIN_DIR/nimbox"
else
  sudo mv /tmp/nimbox "$BIN_DIR/nimbox"
fi

echo ""
echo "nimbox $VERSION installed to $BIN_DIR/nimbox"
echo ""

# Check if BIN_DIR already on PATH
case ":$PATH:" in
  *":$BIN_DIR:"*)
    echo "nimbox is ready. Run: nimbox --help"
    ;;
  *)
    echo "$BIN_DIR is not in your PATH."
    echo ""
    echo "Run this command to add it:"
    echo ""
    echo "  For zsh:  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc && source ~/.zshrc"
    echo "  For bash: echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.bashrc && source ~/.bashrc"
    echo ""
    echo "Or for this session only:"
    echo "  export PATH=\"$BIN_DIR:\$PATH\""
    ;;
esac