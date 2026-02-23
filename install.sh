#!/bin/sh
# Ryvos installer — downloads the latest release binary for your platform.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Ryvos/ryvos/main/install.sh | sh
#
# Options:
#   RYVOS_VERSION=v0.1.0  Pin a specific version (default: latest)
#   RYVOS_INSTALL_DIR=... Override install directory (default: ~/.local/bin)

set -e

REPO="Ryvos/ryvos"
INSTALL_DIR="${RYVOS_INSTALL_DIR:-$HOME/.local/bin}"
BINARY_NAME="ryvos"

# Colors (disabled if not a terminal)
if [ -t 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[1;33m'
    CYAN='\033[0;36m'
    BOLD='\033[1m'
    RESET='\033[0m'
else
    RED='' GREEN='' YELLOW='' CYAN='' BOLD='' RESET=''
fi

info()  { printf "${CYAN}info${RESET}  %s\n" "$1"; }
ok()    { printf "${GREEN}  ok${RESET}  %s\n" "$1"; }
warn()  { printf "${YELLOW}warn${RESET}  %s\n" "$1"; }
error() { printf "${RED}error${RESET} %s\n" "$1" >&2; exit 1; }

# Detect OS
detect_os() {
    case "$(uname -s)" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "macos" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *) error "Unsupported operating system: $(uname -s)" ;;
    esac
}

# Detect architecture
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *) error "Unsupported architecture: $(uname -m)" ;;
    esac
}

# Get latest version from GitHub API
get_latest_version() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | \
            grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" | \
            grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
    else
        error "Neither curl nor wget found. Install one and try again."
    fi
}

# Download a file
download() {
    url="$1"
    dest="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$dest"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$dest" "$url"
    fi
}

main() {
    printf "\n${BOLD}  Ryvos Installer${RESET}\n\n"

    OS=$(detect_os)
    ARCH=$(detect_arch)
    info "Detected platform: ${OS} ${ARCH}"

    # Resolve version
    VERSION="${RYVOS_VERSION:-}"
    if [ -z "$VERSION" ]; then
        info "Fetching latest release..."
        VERSION=$(get_latest_version)
        if [ -z "$VERSION" ]; then
            error "Could not determine latest version. Set RYVOS_VERSION=v0.1.0 and retry."
        fi
    fi
    ok "Version: ${VERSION}"

    # Build artifact name
    case "$OS" in
        windows) ARTIFACT="${BINARY_NAME}-${OS}-${ARCH}.exe" ;;
        *)       ARTIFACT="${BINARY_NAME}-${OS}-${ARCH}" ;;
    esac

    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARTIFACT}"
    info "Downloading ${DOWNLOAD_URL}"

    # Create temp directory
    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    TMP_FILE="${TMP_DIR}/${ARTIFACT}"
    download "$DOWNLOAD_URL" "$TMP_FILE" || error "Download failed. Check that ${VERSION} exists at https://github.com/${REPO}/releases"
    ok "Downloaded ${ARTIFACT}"

    # Verify checksum if available
    CHECKSUM_URL="https://github.com/${REPO}/releases/download/${VERSION}/sha256sums.txt"
    CHECKSUM_FILE="${TMP_DIR}/sha256sums.txt"
    if download "$CHECKSUM_URL" "$CHECKSUM_FILE" 2>/dev/null; then
        EXPECTED=$(grep "$ARTIFACT" "$CHECKSUM_FILE" | awk '{print $1}')
        if [ -n "$EXPECTED" ]; then
            if command -v sha256sum >/dev/null 2>&1; then
                ACTUAL=$(sha256sum "$TMP_FILE" | awk '{print $1}')
            elif command -v shasum >/dev/null 2>&1; then
                ACTUAL=$(shasum -a 256 "$TMP_FILE" | awk '{print $1}')
            else
                ACTUAL=""
            fi

            if [ -n "$ACTUAL" ]; then
                if [ "$EXPECTED" = "$ACTUAL" ]; then
                    ok "Checksum verified"
                else
                    error "Checksum mismatch!\n  Expected: ${EXPECTED}\n  Got:      ${ACTUAL}"
                fi
            else
                warn "No sha256sum/shasum available — skipping checksum verification"
            fi
        fi
    fi

    # Install
    mkdir -p "$INSTALL_DIR"

    case "$OS" in
        windows)
            DEST="${INSTALL_DIR}/${BINARY_NAME}.exe"
            cp "$TMP_FILE" "$DEST"
            ;;
        *)
            DEST="${INSTALL_DIR}/${BINARY_NAME}"
            cp "$TMP_FILE" "$DEST"
            chmod +x "$DEST"
            ;;
    esac

    ok "Installed to ${DEST}"

    # Check if install dir is in PATH
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) ;;
        *)
            warn "${INSTALL_DIR} is not in your PATH"
            printf "\n  Add it to your shell profile:\n"
            printf "    ${BOLD}export PATH=\"\$HOME/.local/bin:\$PATH\"${RESET}\n"
            printf "\n  Then restart your shell or run:\n"
            printf "    ${BOLD}source ~/.bashrc${RESET}  (or ~/.zshrc)\n"
            ;;
    esac

    # Done
    printf "\n${GREEN}${BOLD}  Ryvos ${VERSION} installed successfully!${RESET}\n\n"
    printf "  Get started:\n"
    printf "    ${BOLD}ryvos init${RESET}     Set up your LLM provider\n"
    printf "    ${BOLD}ryvos${RESET}          Start talking to your assistant\n"
    printf "    ${BOLD}ryvos --help${RESET}   See all commands\n\n"
}

main
