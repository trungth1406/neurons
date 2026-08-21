#!/usr/bin/env bash
set -euo pipefail

REPO="trungth1406/neurons"
BIN_DIR="${NEURON_BIN_DIR:-$HOME/.cargo/bin}"

command -v gh >/dev/null 2>&1 || { echo "error: gh CLI required (https://cli.github.com)"; exit 1; }

TAG=$(gh release view --repo "$REPO" --json tagName -q .tagName 2>/dev/null) || {
  echo "error: cannot read releases — run 'gh auth login' first"; exit 1
}

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)  TARGET="aarch64-apple-darwin" ;;
  Linux-x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
  *) echo "error: no prebuilt binary for $(uname -s)-$(uname -m); use: cargo install --git https://github.com/$REPO --locked"; exit 1 ;;
esac

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
echo "downloading neuron-mcp $TAG ($TARGET)..."
gh release download "$TAG" --repo "$REPO" -p "*$TARGET*" -D "$TMP"
tar -xzf "$TMP"/neuron-mcp-*.tar.gz -C "$TMP"
mkdir -p "$BIN_DIR"
mv -f "$TMP/neuron-mcp" "$BIN_DIR/neuron-mcp"
chmod +x "$BIN_DIR/neuron-mcp"
echo "installed: $("$BIN_DIR/neuron-mcp" --version)"

if command -v claude >/dev/null 2>&1; then
  if claude mcp list 2>/dev/null | grep -q "^neurons:"; then
    echo "MCP already registered"
  else
    claude mcp add neurons -- "$BIN_DIR/neuron-mcp" >/dev/null 2>&1 && echo "MCP registered (restart Claude Code sessions to load)"
  fi
else
  echo "register manually: claude mcp add neurons -- $BIN_DIR/neuron-mcp"
fi
