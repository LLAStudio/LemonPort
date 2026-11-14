#!/bin/sh
# 🍋 LemonPort — Linux & macOS installer.
#
#   curl -fsSL https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.sh | sh
#
# Options: --source, --version <v>, --dir <path>, --help
set -eu

REPO="${LEMONPORT_REPO:-LLAStudio/LemonPort}"
BIN="lemon-port"
VERSION="${LEMONPORT_VERSION:-}"
INSTALL_DIR="${LEMONPORT_INSTALL_DIR:-$HOME/.local/bin}"
FROM_SOURCE=0

usage() {
    cat <<'EOF'
🍋 LemonPort — Linux & macOS installer

Usage:
  install.sh [options]

Options:
  --source          Build and install from source (Cargo required)
  --version <v>     Version to install (default: latest release)
  --dir <path>      Installation directory (default: ~/.local/bin)
  -h, --help        Show this help

Variables:
  LEMONPORT_VERSION       Version to install
  LEMONPORT_INSTALL_DIR   Installation directory
  LEMONPORT_REPO          GitHub repository (default: LLAStudio/LemonPort)
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --source)
            FROM_SOURCE=1
            shift
            ;;
        --version)
            [ "$#" -ge 2 ] || { echo "✗ --version requires a value" >&2; exit 1; }
            VERSION="$2"
            shift 2
            ;;
        --version=*)
            VERSION="${1#*=}"
            shift
            ;;
        --dir)
            [ "$#" -ge 2 ] || { echo "✗ --dir requires a value" >&2; exit 1; }
            INSTALL_DIR="$2"
            shift 2
            ;;
        --dir=*)
            INSTALL_DIR="${1#*=}"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "✗ Unknown option: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

if [ "$FROM_SOURCE" -eq 1 ]; then
    if ! command -v cargo >/dev/null 2>&1; then
        echo "✗ Rust/Cargo is required for --source: https://rustup.rs" >&2
        exit 1
    fi
    echo "🍋 Installing LemonPort from source…"
    cargo install --git "https://github.com/$REPO" --locked --force
    echo "✓ lemon-port and lp installed in ${CARGO_HOME:-$HOME/.cargo}/bin"
    exit 0
fi

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux) OS_KEY="linux" ;;
    Darwin) OS_KEY="macos" ;;
    *)
        echo "✗ Unsupported system: $OS (Linux and macOS only)." >&2
        echo "  From source: curl -fsSL .../install.sh | sh -s -- --source" >&2
        exit 1
        ;;
esac

case "$ARCH" in
    x86_64|amd64) ARCH_KEY="x86_64" ;;
    arm64|aarch64) ARCH_KEY="aarch64" ;;
    *)
        echo "✗ Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

if [ "$OS_KEY" = "linux" ]; then
    if [ "$ARCH_KEY" = "x86_64" ]; then
        TARGET="x86_64-unknown-linux-musl"
    else
        TARGET="aarch64-unknown-linux-gnu"
    fi
else
    TARGET="$ARCH_KEY-apple-darwin"
fi

if [ -z "$VERSION" ]; then
    VERSION="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
        | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1 || true)"
fi

if [ -z "$VERSION" ]; then
    echo "✗ No binary release available for $REPO." >&2
    echo "  Install from source:" >&2
    echo "    curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | sh -s -- --source" >&2
    exit 1
fi

case "$VERSION" in
    v[0-9]*) ;;
    [0-9]*) VERSION="v$VERSION" ;;
    *)
        echo "✗ Invalid version: $VERSION" >&2
        exit 1
        ;;
esac

TMP="$(mktemp -d "${TMPDIR:-/tmp}/lemonport.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

ASSET="lemon-port-$VERSION-$TARGET.tar.gz"
BASE="https://github.com/$REPO/releases/download/$VERSION"

echo "🍋 Downloading $ASSET…"
curl -fL --retry 3 --proto '=https' -o "$TMP/$ASSET" "$BASE/$ASSET"
curl -fL --retry 3 --proto '=https' -o "$TMP/SHA256SUMS" "$BASE/SHA256SUMS"

EXPECTED="$(grep " $ASSET\$" "$TMP/SHA256SUMS" | awk '{print $1}' | head -n 1)"
if [ -z "$EXPECTED" ]; then
    echo "✗ Checksum not found for $ASSET" >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "$TMP/$ASSET" | awk '{print $1}')"
else
    ACTUAL="$(shasum -a 256 "$TMP/$ASSET" | awk '{print $1}')"
fi

if [ "$EXPECTED" != "$ACTUAL" ]; then
    echo "✗ Invalid checksum for $ASSET" >&2
    exit 1
fi

tar -xzf "$TMP/$ASSET" -C "$TMP"
mkdir -p "$INSTALL_DIR"
install -m 0755 "$TMP/$BIN" "$INSTALL_DIR/$BIN"
ln -sf "$BIN" "$INSTALL_DIR/lp"

case ":${PATH:-}:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "⚠ Add $INSTALL_DIR to PATH, for example:"
        echo "    export PATH=\"$INSTALL_DIR:\$PATH\""
        ;;
esac

echo "✓ LemonPort $VERSION installed in $INSTALL_DIR (lemon-port + lp)"
"$INSTALL_DIR/$BIN" --version 2>/dev/null || true
