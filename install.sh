#!/bin/sh
# glances-rs installer
# Downloads the latest release binary, verifies SHA-256, and drops it into
# ${XDG_BIN_HOME:-$HOME/.local/bin}.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/UberMetroid/glances-rs/Rust/install/install.sh | sh
#
# Environment variables:
#   INSTALL_DIR    target bin directory (default: $XDG_BIN_HOME or $HOME/.local/bin)
#   VERSION        pinned version to install (default: latest)
#   REPO           GitHub repo (default: UberMetroid/glances-rs)
#   NO_COLOR       set to disable ANSI color output
set -e

REPO="${REPO:-UberMetroid/glances-rs}"
REF="${REF:-rust}"
LATEST_URL="https://api.github.com/repos/${REPO}/releases/latest"
DOWNLOAD_BASE="https://github.com/${REPO}/releases/download"
RAW_BASE="https://raw.githubusercontent.com/${REPO}/${REF}"

DEFAULT_DEST="${XDG_BIN_HOME:-$HOME/.local/bin}"
DEST_DIR="${INSTALL_DIR:-$DEFAULT_DEST}"
BIN_NAME="glances-rs"

INTEGRITY_BASE="${INTEGRITY_BASE:-$RAW_BASE}"

# Pick a binary name suffix that matches the host OS/arch.
HOST_OS=$(uname -s 2>/dev/null || echo unknown)
HOST_ARCH=$(uname -m 2>/dev/null || echo unknown)

# glances-rs is Linux-only as of v0.9.0. The maintainer does not have
# macOS or Windows hardware to test FFI on; cross-compiling produces a
# binary that errors at runtime, which is worse than no install.
# Refuse early with a clear message.
case "$HOST_OS" in
    Linux) ;;
    *) err "glances-rs is Linux-only. Detected host: ${HOST_OS}
macOS and Windows support was removed in v0.9.0. If you can donate
hardware or CI time for porting, open an issue at
github.com/UberMetroid/glances-rs/issues." ;;
esac

case "$HOST_ARCH" in
    x86_64|amd64)   arch_part="x86_64" ;;
    aarch64|arm64)  arch_part="aarch64" ;;
    *)              arch_part="unknown" ;;
esac

ASSET_BASE="${BIN_NAME}-linux-${arch_part}"

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    BOLD="\033[1m" GREEN="\033[32m" YELLOW="\033[33m"
    RED="\033[31m" CYAN="\033[36m" RESET="\033[0m"
else
    BOLD="" GREEN="" YELLOW="" RED="" CYAN="" RESET=""
fi

info()    { printf "${CYAN}==>${RESET} ${BOLD}%s${RESET}\n" "$1"; }
success() { printf "${GREEN}==>${RESET} %s\n" "$1"; }
warn()    { printf "${YELLOW}warning:${RESET} %s\n" "$1" >&2; }
err()     { printf "${RED}error:${RESET} %s\n" "$1" >&2; exit 1; }

usage() {
    cat <<EOF
glances-rs installer

Usage:
    curl -fsSL <install.sh> | sh                    # install latest
    VERSION=v0.8.0 sh install.sh                   # install specific
    INSTALL_DIR=/usr/local/bin sh install.sh        # install root location

Environment:
    REPO             GitHub repo (default: UberMetroid/glances-rs)
    REF              Git ref / branch (default: rust)
    VERSION          pinned release tag (default: latest)
    INSTALL_DIR      target bin dir (default: \$XDG_BIN_HOME or \$HOME/.local/bin)
    INTEGRITY_BASE   base URL for install.sh.sha256 (default: raw github)
    NO_COLOR         disable ANSI color output

Integrity check:
    When run from a file (not piped), the installer fetches
    install.sh.sha256 from INTEGRITY_BASE and verifies the script
    bytes against it before doing anything. Set INTEGRITY_BASE=''
    to skip. Pipe-mode (curl URL | sh) skips the check by design.
EOF
}

case "${1:-}" in
    -h|--help) usage; exit 0 ;;
    *) ;;
esac

# Step 0: detect tools.
command -v curl >/dev/null 2>&1 || err "curl is required"
command -v shasum >/dev/null 2>&1 || command -v sha256sum >/dev/null 2>&1 \
    || err "shasum or sha256sum is required"

# Step 0a: self-integrity check. If the script was loaded from a file
# (not piped through `curl URL | sh`), fetch the published .sha256 for
# this script and verify the bytes match. If the script is being read
# from stdin, BASH_SOURCE and $0 are unreliable so we skip the check.
sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

self_path="${BASH_SOURCE[0]:-$0}"
if [ -n "${INTEGRITY_BASE}" ] && [ -f "$self_path" ]; then
    EXPECTED_SHA_URL="${INTEGRITY_BASE%/}/install.sh.sha256"
    info "Verifying install.sh integrity against ${EXPECTED_SHA_URL}"
    EXPECTED_SHA=$(curl -fsSL --max-time 15 "$EXPECTED_SHA_URL" 2>/dev/null \
        | awk '{print $1}' | head -n1)
    ACTUAL_SHA=$(sha256_of "$self_path")
    if [ -z "$EXPECTED_SHA" ]; then
        warn "could not fetch expected hash; skipping integrity check"
    elif [ "$EXPECTED_SHA" != "$ACTUAL_SHA" ]; then
        err "install.sh integrity check FAILED
  expected: ${EXPECTED_SHA}
  actual:   ${ACTUAL_SHA}
The script you ran does not match the published hash. Either the
download was corrupted, the .sha256 file was not refreshed, or the
script was tampered with. Re-download and try again, or set
INTEGRITY_BASE='' to skip this check (not recommended)."
    else
        success "install.sh integrity verified (${ACTUAL_SHA:0:12}\u2026)"
    fi
elif [ -n "${INTEGRITY_BASE}" ]; then
    info "Skipping integrity check (running from stdin, not a file)"
fi

# Step 1: resolve the version.
if [ -z "${VERSION:-}" ]; then
    info "Resolving latest release from ${REPO}..."
    VERSION=$(curl -fsSL -H "Accept: application/vnd.github+json" \
        "$LATEST_URL" \
        | grep '"tag_name"' \
        | head -n1 \
        | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/') \
        || err "could not determine latest version (is the repo public?)"
fi
info "Installing ${BIN_NAME} ${VERSION}"

# Step 2: pick a destination directory.
mkdir -p "$DEST_DIR" || err "could not create ${DEST_DIR}"

# Step 3: pick an asset name. The release may publish per-OS binaries; if
# not, fall back to the source build (handled outside this script).
TARBALL=""
if [ "$os_part" != "unknown" ] && [ "$arch_part" != "unknown" ]; then
    for candidate in "${ASSET_BASE}.tar.gz" "${ASSET_BASE}.tar.xz" "${ASSET_BASE}.zip"; do
        if curl -fsSLI -o /dev/null -w "%{http_code}\n" \
                "${DOWNLOAD_BASE}/${VERSION}/${candidate}" 2>/dev/null \
                | grep -q '^200$'; then
            TARBALL="$candidate"
            break
        fi
    done
fi

WORK=$(mktemp -d 2>/dev/null || mktemp -d -t 'glances-rs')
trap 'rm -rf "$WORK"' EXIT INT TERM

if [ -n "$TARBALL" ]; then
    info "Downloading ${TARBALL}..."
    curl -fsSL -o "${WORK}/${TARBALL}" \
        "${DOWNLOAD_BASE}/${VERSION}/${TARBALL}" \
        || err "download failed"
    info "Extracting..."
    case "$TARBALL" in
        *.tar.gz) tar -xzf "${WORK}/${TARBALL}" -C "$WORK" ;;
        *.tar.xz) tar -xJf "${WORK}/${TARBALL}" -C "$WORK" ;;
        *.zip)    (cd "$WORK" && unzip -q "${TARBALL}") ;;
    esac
    BIN_PATH=$(find "$WORK" -type f -name "${BIN_NAME}" \
        -perm -u+x 2>/dev/null | head -n1)
    [ -n "$BIN_PATH" ] || err "binary not found in ${TARBALL}"
    CHECKSUMS_URL="${DOWNLOAD_BASE}/${VERSION}/SHA256SUMS"
    if curl -fsSL -o "${WORK}/SHA256SUMS" "$CHECKSUMS_URL" 2>/dev/null; then
        info "Verifying SHA-256..."
        EXPECTED=$(grep "  ${BIN_NAME}\$" "${WORK}/SHA256SUMS" \
            | awk '{print $1}' | head -n1)
        [ -n "$EXPECTED" ] || err "checksum not found for ${BIN_NAME}"
        if command -v sha256sum >/dev/null 2>&1; then
            ACTUAL=$(sha256sum "$BIN_PATH" | awk '{print $1}')
        else
            ACTUAL=$(shasum -a 256 "$BIN_PATH" | awk '{print $1}')
        fi
        if [ "$EXPECTED" != "$ACTUAL" ]; then
            err "SHA-256 mismatch
  expected: ${EXPECTED}
  actual:   ${ACTUAL}"
        fi
        success "SHA-256 verified"
    else
        warn "SHA256SUMS not published for ${VERSION}; skipping verification"
    fi
else
    info "No prebuilt binary for ${HOST_OS}/${HOST_ARCH}."
    info "Building from source..."
    command -v cargo >/dev/null 2>&1 || err "cargo is required to build from source"
    command -v rustc >/dev/null 2>&1 || err "rustc is required to build from source"
    SRC="${WORK}/src"
    mkdir -p "$SRC"
    curl -fsSL "https://codeload.github.com/${REPO}/tar.gz/refs/heads/${REF}" \
        -o "${WORK}/src.tar.gz" \
        || err "could not fetch source"
    tar -xzf "${WORK}/src.tar.gz" -C "$SRC" --strip-components=1
    (cd "$SRC" && cargo build --release --locked) \
        || err "cargo build --release failed"
    BIN_PATH="${SRC}/target/release/${BIN_NAME}"
    [ -x "$BIN_PATH" ] || err "binary not produced at ${BIN_PATH}"
fi

# Step 4: install.
info "Installing to ${DEST_DIR}/${BIN_NAME}"
mv "$BIN_PATH" "${DEST_DIR}/${BIN_NAME}"
chmod +x "${DEST_DIR}/${BIN_NAME}"

success "Installed ${BIN_NAME} ${VERSION} -> ${DEST_DIR}/${BIN_NAME}"

if ! echo ":$PATH:" | grep -q ":${DEST_DIR}:"; then
    warn "${DEST_DIR} is not on your PATH"
    warn "Add this to your shell profile:"
    warn "    export PATH=\"${DEST_DIR}:\$PATH\""
fi

"${DEST_DIR}/${BIN_NAME}" --version
