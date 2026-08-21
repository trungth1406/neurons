#!/usr/bin/env bash
set -euo pipefail

REPO="trungth1406/neurons"
BIN_DIR="${NEURON_BIN_DIR:-$HOME/.cargo/bin}"
API="https://api.github.com/repos/$REPO/releases/latest"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)  TARGET="aarch64-apple-darwin" ;;
  Linux-x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
  *) echo "error: no prebuilt binary for $(uname -s)-$(uname -m); use: cargo install --git https://github.com/$REPO --locked"; exit 1 ;;
esac

echo "fetching latest release..."
RELEASE_JSON=$(curl -fsSL "$API")
DOWNLOAD_URL=$(echo "$RELEASE_JSON" | grep -o "https://[^\"]*neuron-mcp[^\"]*${TARGET}[^\"]*\.tar\.gz" | head -1 || true)
if [ -z "$DOWNLOAD_URL" ]; then
  echo "error: cannot find release asset for $TARGET"; exit 1
fi
TAG=$(echo "$RELEASE_JSON" | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4)

CURRENT=$("$BIN_DIR/neuron-mcp" --version 2>/dev/null | awk '{print $2}') || CURRENT="none"
if [ "v$CURRENT" = "$TAG" ]; then
  echo "neuron-mcp already at $TAG"
else
  TMP=$(mktemp -d)
  trap 'rm -rf "$TMP"' EXIT
  echo "downloading neuron-mcp $TAG for $TARGET..."
  curl -fsSL -o "$TMP/neuron-mcp.tar.gz" "$DOWNLOAD_URL"
  tar -xzf "$TMP/neuron-mcp.tar.gz" -C "$TMP"
  mkdir -p "$BIN_DIR"
  mv -f "$TMP/neuron-mcp" "$BIN_DIR/neuron-mcp"
  chmod +x "$BIN_DIR/neuron-mcp"
  echo "installed neuron-mcp $TAG"
fi

if command -v claude >/dev/null 2>&1; then
  if claude mcp list 2>/dev/null | grep -q "^neurons:"; then
    echo "MCP already registered"
  else
    claude mcp add neurons -- "$BIN_DIR/neuron-mcp" >/dev/null 2>&1 && echo "MCP registered (restart Claude Code sessions to load)"
  fi
else
  echo "register manually: claude mcp add neurons -- $BIN_DIR/neuron-mcp"
fi
