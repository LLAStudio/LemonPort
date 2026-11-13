#!/bin/sh
# 🍋 LemonPort — installateur Linux & macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/LLAStudio/LemonPort/main/install.sh | sh
#
# Options : --source, --version <v>, --dir <chemin>, --help
set -eu

REPO="${LEMONPORT_REPO:-LLAStudio/LemonPort}"
BIN="lemon-port"
VERSION="${LEMONPORT_VERSION:-}"
INSTALL_DIR="${LEMONPORT_INSTALL_DIR:-$HOME/.local/bin}"
FROM_SOURCE=0

usage() {
    cat <<'EOF'
🍋 LemonPort — installateur Linux & macOS

Usage :
  install.sh [options]

Options :
  --source          Compile et installe depuis les sources (Cargo requis)
  --version <v>     Version à installer (défaut : dernière release)
  --dir <chemin>    Dossier d'installation (défaut : ~/.local/bin)
  -h, --help        Affiche cette aide

Variables :
  LEMONPORT_VERSION       Version à installer
  LEMONPORT_INSTALL_DIR   Dossier d'installation
  LEMONPORT_REPO          Dépôt GitHub (défaut : LLAStudio/LemonPort)
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --source)
            FROM_SOURCE=1
            shift
            ;;
        --version)
            [ "$#" -ge 2 ] || { echo "✗ --version attend une valeur" >&2; exit 1; }
            VERSION="$2"
            shift 2
            ;;
        --version=*)
            VERSION="${1#*=}"
            shift
            ;;
        --dir)
            [ "$#" -ge 2 ] || { echo "✗ --dir attend une valeur" >&2; exit 1; }
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
            echo "✗ Option inconnue : $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

if [ "$FROM_SOURCE" -eq 1 ]; then
    if ! command -v cargo >/dev/null 2>&1; then
        echo "✗ Rust/Cargo est requis pour --source : https://rustup.rs" >&2
        exit 1
    fi
    echo "🍋 Installation de LemonPort depuis les sources…"
    cargo install --git "https://github.com/$REPO" --locked --force
    echo "✓ lemon-port et lp installés dans ${CARGO_HOME:-$HOME/.cargo}/bin"
    exit 0
fi

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux) OS_KEY="linux" ;;
    Darwin) OS_KEY="macos" ;;
    *)
        echo "✗ Système non supporté : $OS (Linux et macOS uniquement)." >&2
        echo "  Depuis les sources : curl -fsSL .../install.sh | sh -s -- --source" >&2
        exit 1
        ;;
esac

case "$ARCH" in
    x86_64|amd64) ARCH_KEY="x86_64" ;;
    arm64|aarch64) ARCH_KEY="aarch64" ;;
    *)
        echo "✗ Architecture non supportée : $ARCH" >&2
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
    echo "✗ Aucune release binaire disponible pour $REPO." >&2
    echo "  Installez depuis les sources :" >&2
    echo "    curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | sh -s -- --source" >&2
    exit 1
fi

case "$VERSION" in
    v[0-9]*) ;;
    [0-9]*) VERSION="v$VERSION" ;;
    *)
        echo "✗ Version invalide : $VERSION" >&2
        exit 1
        ;;
esac

TMP="$(mktemp -d "${TMPDIR:-/tmp}/lemonport.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

ASSET="lemon-port-$VERSION-$TARGET.tar.gz"
BASE="https://github.com/$REPO/releases/download/$VERSION"

echo "🍋 Téléchargement de $ASSET…"
curl -fL --retry 3 --proto '=https' -o "$TMP/$ASSET" "$BASE/$ASSET"
curl -fL --retry 3 --proto '=https' -o "$TMP/SHA256SUMS" "$BASE/SHA256SUMS"

EXPECTED="$(grep " $ASSET\$" "$TMP/SHA256SUMS" | awk '{print $1}' | head -n 1)"
if [ -z "$EXPECTED" ]; then
    echo "✗ Checksum introuvable pour $ASSET" >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "$TMP/$ASSET" | awk '{print $1}')"
else
    ACTUAL="$(shasum -a 256 "$TMP/$ASSET" | awk '{print $1}')"
fi

if [ "$EXPECTED" != "$ACTUAL" ]; then
    echo "✗ Checksum invalide pour $ASSET" >&2
    exit 1
fi

tar -xzf "$TMP/$ASSET" -C "$TMP"
mkdir -p "$INSTALL_DIR"
install -m 0755 "$TMP/$BIN" "$INSTALL_DIR/$BIN"
ln -sf "$BIN" "$INSTALL_DIR/lp"

case ":${PATH:-}:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "⚠ Ajoutez $INSTALL_DIR au PATH, par exemple :"
        echo "    export PATH=\"$INSTALL_DIR:\$PATH\""
        ;;
esac

echo "✓ LemonPort $VERSION installé dans $INSTALL_DIR (lemon-port + lp)"
"$INSTALL_DIR/$BIN" --version 2>/dev/null || true
