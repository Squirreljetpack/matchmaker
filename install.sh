#!/usr/bin/env bash
#
# matchmaker installation script
# Downloads and installs the latest release from GitHub
#

set -e

# Configuration
REPO="Squirreljetpack/matchmaker"
BINARY_NAME="mm"
INSTALL_DIR_CARGO="$HOME/.cargo/bin"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

error() {
	echo -e "${RED}Error: $1${NC}" >&2
	exit 1
}

info() {
	echo -e "${GREEN}$1${NC}"
}

warn() {
	echo -e "${YELLOW}$1${NC}"
}

# Detect OS and architecture
detect_os() {
	case "$(uname -s)" in
	Linux*) echo "linux" ;;
	Darwin*) echo "mac" ;;
	CYGWIN* | MINGW* | MSYS*) echo "windows" ;;
	*) error "Unsupported OS: $(uname -s)" ;;
	esac
}

detect_arch() {
	case "$(uname -m)" in
	x86_64) echo "x86_64" ;;
	arm64) echo "aarch64" ;;
	aarch64) echo "aarch64" ;;
	*) error "Unsupported architecture: $(uname -m)" ;;
	esac
}

# Determine install directory
get_install_dir() {
	if command -v cargo >/dev/null 2>&1 && [[ ":$PATH:" == *":$INSTALL_DIR_CARGO:"* ]]; then
		echo "$INSTALL_DIR_CARGO"
	else
		case "$OS" in
		linux | mac)
			echo "/usr/local/bin"
			;;
		windows)
			error "Windows installation not supported via this script. Please download manually from GitHub releases."
			;;
		esac
	fi
}

# Get the latest release version
get_latest_release() {
	local url="https://api.github.com/repos/$REPO/releases/latest"
	local version
	version=$(curl -s "$url" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
	if [[ -z "$version" ]]; then
		error "Failed to fetch latest release version"
	fi
	echo "$version"
}

# Download and install
main() {
	OS=$(detect_os)
	ARCH=$(detect_arch)

	info "Detected OS: $OS ($ARCH)"

	INSTALL_DIR=$(get_install_dir)
	info "Install directory: $INSTALL_DIR"

	# Check if we can write to install directory
	if [[ ! -w "$INSTALL_DIR" ]]; then
		error "Cannot write to $INSTALL_DIR. Please run with appropriate permissions."
	fi

	# Get latest release
	VERSION=$(get_latest_release)
	info "Latest version: $VERSION"

	# Construct download URL
	ASSET_NAME="matchmaker-${OS}.tar.gz"
	DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET_NAME"

	info "Downloading $ASSET_NAME..."

	# Create temporary directory
	TEMP_DIR=$(mktemp -d)
	trap "rm -rf $TEMP_DIR" EXIT

	# Download and extract
	if ! curl -sL "$DOWNLOAD_URL" -o "$TEMP_DIR/$ASSET_NAME"; then
		error "Failed to download from $DOWNLOAD_URL"
	fi

	tar -xzf "$TEMP_DIR/$ASSET_NAME" -C "$TEMP_DIR"

	# Install binary
	INSTALL_PATH="$INSTALL_DIR/$BINARY_NAME"
	if [[ -f "$INSTALL_PATH" ]]; then
		warn "Existing installation found at $INSTALL_PATH, replacing..."
		rm -f "$INSTALL_PATH"
	fi

	if [[ -f "$TEMP_DIR/$BINARY_NAME" ]]; then
		mv "$TEMP_DIR/$BINARY_NAME" "$INSTALL_DIR/"
	elif [[ -f "$TEMP_DIR/target/release/$BINARY_NAME" ]]; then
		mv "$TEMP_DIR/target/release/$BINARY_NAME" "$INSTALL_DIR/"
	else
		FOUND_BIN=$(find "$TEMP_DIR" -name "$BINARY_NAME" -type f | head -n 1)
		if [[ -n "$FOUND_BIN" ]]; then
			mv "$FOUND_BIN" "$INSTALL_DIR/"
		else
			error "Could not find binary $BINARY_NAME in the downloaded archive"
		fi
	fi

	chmod +x "$INSTALL_PATH"

	info "Successfully installed $BINARY_NAME to $INSTALL_PATH"
	info ""
	info "To get started, run:"
	info "  $BINARY_NAME --help"
}

main "$@"
