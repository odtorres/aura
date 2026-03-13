#!/usr/bin/env bash
# build-appimage.sh — Build an AppImage for AURA Editor
#
# Usage: ./dist/appimage/build-appimage.sh [VERSION]
#
# Prerequisites:
#   - Rust toolchain (stable)
#   - wget or curl
#   - FUSE (for running the resulting AppImage)
#
# The script must be run from the repository root.

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

PKGNAME="aura-editor"
VERSION="${1:-$(grep '^version' Cargo.toml | head -1 | sed 's/.*= *"\(.*\)"/\1/')}"
ARCH="$(uname -m)"
APPDIR="build/AppDir"
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${ARCH}.AppImage"
LINUXDEPLOY_BIN="build/linuxdeploy-${ARCH}.AppImage"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
OUTPUT_APPIMAGE="${REPO_ROOT}/${PKGNAME}-${VERSION}-${ARCH}.AppImage"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

info()  { printf '\033[1;34m[INFO]\033[0m  %s\n' "$*"; }
warn()  { printf '\033[1;33m[WARN]\033[0m  %s\n' "$*"; }
error() { printf '\033[1;31m[ERROR]\033[0m %s\n' "$*" >&2; exit 1; }

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || error "Required command not found: $1"
}

# ---------------------------------------------------------------------------
# Preflight checks
# ---------------------------------------------------------------------------

cd "${REPO_ROOT}"

require_cmd cargo
require_cmd rustc

if ! command -v wget >/dev/null 2>&1 && ! command -v curl >/dev/null 2>&1; then
    error "Either wget or curl is required to download linuxdeploy"
fi

# Only supported on Linux
if [[ "$(uname -s)" != "Linux" ]]; then
    error "AppImage builds are only supported on Linux (got: $(uname -s))"
fi

# ---------------------------------------------------------------------------
# Step 1: Build the release binary
# ---------------------------------------------------------------------------

info "Building AURA release binary (version ${VERSION}) ..."
cargo build --release --package aura

BINARY="target/release/aura"
[[ -f "${BINARY}" ]] || error "Binary not found at ${BINARY} after build"
info "Binary built: ${BINARY}"

# ---------------------------------------------------------------------------
# Step 2: Download linuxdeploy if not present
# ---------------------------------------------------------------------------

mkdir -p build

if [[ ! -f "${LINUXDEPLOY_BIN}" ]]; then
    info "Downloading linuxdeploy for ${ARCH} ..."
    if command -v wget >/dev/null 2>&1; then
        wget -q --show-progress -O "${LINUXDEPLOY_BIN}" "${LINUXDEPLOY_URL}"
    else
        curl -L --progress-bar -o "${LINUXDEPLOY_BIN}" "${LINUXDEPLOY_URL}"
    fi
    chmod +x "${LINUXDEPLOY_BIN}"
    info "linuxdeploy downloaded to ${LINUXDEPLOY_BIN}"
else
    info "linuxdeploy already present at ${LINUXDEPLOY_BIN}"
fi

# ---------------------------------------------------------------------------
# Step 3: Assemble AppDir
# ---------------------------------------------------------------------------

info "Assembling AppDir at ${APPDIR} ..."

rm -rf "${APPDIR}"
mkdir -p "${APPDIR}/usr/bin"
mkdir -p "${APPDIR}/usr/share/applications"
mkdir -p "${APPDIR}/usr/share/icons/hicolor/scalable/apps"
mkdir -p "${APPDIR}/usr/share/doc/${PKGNAME}"

# Binary
cp "${BINARY}" "${APPDIR}/usr/bin/aura"

# Desktop file
cp "${SCRIPT_DIR}/aura-editor.desktop" "${APPDIR}/usr/share/applications/aura-editor.desktop"

# Icon (SVG — scalable)
cp "${SCRIPT_DIR}/aura-editor.svg" "${APPDIR}/usr/share/icons/hicolor/scalable/apps/aura-editor.svg"

# Documentation
cp README.md "${APPDIR}/usr/share/doc/${PKGNAME}/README.md"
cp LICENSE   "${APPDIR}/usr/share/doc/${PKGNAME}/LICENSE"

info "AppDir assembled."

# ---------------------------------------------------------------------------
# Step 4: Run linuxdeploy to produce the AppImage
# ---------------------------------------------------------------------------

info "Running linuxdeploy to create AppImage ..."

ARCH="${ARCH}" "${LINUXDEPLOY_BIN}" \
    --appdir "${APPDIR}" \
    --desktop-file "${APPDIR}/usr/share/applications/aura-editor.desktop" \
    --icon-file "${APPDIR}/usr/share/icons/hicolor/scalable/apps/aura-editor.svg" \
    --output appimage

# linuxdeploy names the output based on the desktop Name= field and arch.
# Move it to a predictable location.
GENERATED_APPIMAGE="$(ls -1t AURA_Editor-*.AppImage 2>/dev/null | head -1 || true)"
if [[ -z "${GENERATED_APPIMAGE}" ]]; then
    # Fallback: look for any newly created AppImage
    GENERATED_APPIMAGE="$(ls -1t ./*.AppImage 2>/dev/null | head -1 || true)"
fi

if [[ -n "${GENERATED_APPIMAGE}" && "${GENERATED_APPIMAGE}" != "${OUTPUT_APPIMAGE}" ]]; then
    mv "${GENERATED_APPIMAGE}" "${OUTPUT_APPIMAGE}"
fi

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------

if [[ -f "${OUTPUT_APPIMAGE}" ]]; then
    info "AppImage created successfully:"
    info "  ${OUTPUT_APPIMAGE}"
else
    warn "Could not confirm output AppImage location — check current directory for *.AppImage files."
fi
