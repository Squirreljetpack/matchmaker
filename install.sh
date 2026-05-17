#!/bin/sh
#
# matchmaker installation script
#

set -e

REPO="Squirreljetpack/matchmaker"
BINARY_BASE_NAME="mm"
INSTALL_DIR_CARGO="$HOME/.cargo/bin"
INSTALL_DIR_LOCAL="$HOME/.local/bin"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

error() { printf "%bError: %s%b\n" "${RED}" "$1" "${NC}" >&2; exit 1; }
info() { printf "%b%s%b\n" "${GREEN}" "$1" "${NC}"; }
warn() { printf "%bWarning: %s%b\n" "${YELLOW}" "$1" "${NC}"; }

detect_os() {
    case "$(uname -s)" in
        Linux*) echo "linux" ;;
        Darwin*) echo "mac" ;;
        CYGWIN*|MINGW*|MSYS*) echo "windows" ;;
        *) error "Unsupported OS: $(uname -s)" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64) echo "x86_64" ;;
        arm64|aarch64) echo "aarch64" ;;
        *) echo "x86_64" ;; # Default to x86_64 if unknown
    esac
}

get_install_dir() {
    # 1. Cargo priority
    case ":$PATH:" in
        *":$INSTALL_DIR_CARGO:"*) echo "$INSTALL_DIR_CARGO"; return ;;
    esac

    # 2. Local bin priority
    if [ -d "$INSTALL_DIR_LOCAL" ] || mkdir -p "$INSTALL_DIR_LOCAL" 2>/dev/null; then
        echo "$INSTALL_DIR_LOCAL"
    # 3. Windows-specific fallback
	elif [ "$OS" = "windows" ]; then
		_win_appdata="${LOCALAPPDATA:-}"
		[ -z "$_win_appdata" ] && error "LOCALAPPDATA not set"

		_win_path="$_win_appdata/Programs/matchmaker"
		mkdir -p "$_win_path" || error "Could not create $_win_path"

		echo "$_win_path"
	fi
}

get_latest_release() {
    _url="https://api.github.com/repos/$REPO/releases/latest"
    _version=$(curl -s "$_url" | grep '"tag_name":' | sed 's/.*"tag_name": "//;s/".*//')
    [ -z "$_version" ] && error "Failed to fetch latest release version"
    echo "$_version"
}

main() {
    OS=$(detect_os)
    ARCH=$(detect_arch)
    BINARY_NAME="$BINARY_BASE_NAME"
    [ "$OS" = "windows" ] && BINARY_NAME="${BINARY_BASE_NAME}.exe"

    info "Detected OS: $OS ($ARCH)"
    INSTALL_DIR=$(get_install_dir)
    VERSION=$(get_latest_release)

    # Asset Naming Logic
    if [ "$OS" = "windows" ]; then
        ASSET_NAME="matchmaker-windows.zip"
    else
        ASSET_NAME="matchmaker-${OS}.tar.gz"
    fi

    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET_NAME"

    TEMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t 'mm')
    trap 'rm -rf "$TEMP_DIR"' 0  # POSIX trap on exit 0

    info "Downloading $ASSET_NAME..."
    curl -sL "$DOWNLOAD_URL" -o "$TEMP_DIR/$ASSET_NAME" || error "Download failed"

    # Extraction
    case "$ASSET_NAME" in
        *.zip)
            if command -v unzip >/dev/null 2>&1; then
                unzip -q "$TEMP_DIR/$ASSET_NAME" -d "$TEMP_DIR"
            else
                tar -xf "$TEMP_DIR/$ASSET_NAME" -C "$TEMP_DIR"
            fi
            ;;
        *)
            tar -xzf "$TEMP_DIR/$ASSET_NAME" -C "$TEMP_DIR"
            ;;
    esac

    # Smart Sudo: Only use on Unix if write-access is denied
    SUDO=""
    if [ ! -w "$INSTALL_DIR" ]; then
        if [ "$OS" = "windows" ]; then
            error "No write permission for $INSTALL_DIR. Please run as Administrator."
        else
            warn "No write permission. Attempting sudo..."
            SUDO="sudo"
        fi
    fi

    FOUND_BIN=$(find "$TEMP_DIR" -name "$BINARY_NAME" -type f | head -n 1)
    [ -z "$FOUND_BIN" ] && error "Binary $BINARY_NAME not found in archive"

    $SUDO rm -f "$INSTALL_DIR/$BINARY_NAME"
    $SUDO mv "$FOUND_BIN" "$INSTALL_DIR/"
    $SUDO chmod +x "$INSTALL_DIR/$BINARY_NAME"

    info "Successfully installed to $INSTALL_DIR/$BINARY_NAME"

    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *) warn "$INSTALL_DIR is not in PATH. Please add it." ;;
    esac
}

main "$@"