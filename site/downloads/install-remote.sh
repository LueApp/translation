#!/usr/bin/env bash
# AI Translate — one-shot remote installer.
#
# Downloads the release tarball, verifies its published SHA-256, extracts it,
# and runs the bundled install.sh (binary -> ~/.local/bin, systemd user
# service, app-menu entry). Nothing here needs sudo.
#
# This is the script the website hands to web2local's /deploy endpoint: the
# daemon shows you this full source + its SHA-256 in a native dialog before it
# writes or runs anything. You can also run it yourself:
#     curl -fsSL https://translate.lue-app.com/downloads/install-remote.sh | bash

set -euo pipefail

VER="0.1.0"
BASE="https://translate.lue-app.com/downloads"
PKG="ai-translate-${VER}-x86_64-linux"

need() { command -v "$1" >/dev/null 2>&1 || { echo "error: '$1' is required but not installed." >&2; exit 1; }; }
need curl
need tar
need sha256sum

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> Downloading ${PKG}.tar.gz"
curl -fSL --proto '=https' "${BASE}/${PKG}.tar.gz"        -o "${TMP}/${PKG}.tar.gz"
curl -fSL --proto '=https' "${BASE}/${PKG}.tar.gz.sha256" -o "${TMP}/${PKG}.tar.gz.sha256"

echo "==> Verifying checksum"
( cd "${TMP}" && sha256sum -c "${PKG}.tar.gz.sha256" )

echo "==> Extracting"
tar -xzf "${TMP}/${PKG}.tar.gz" -C "${TMP}"

echo "==> Running installer"
bash "${TMP}/${PKG}/install.sh"

echo "==> AI Translate installed. Open the app menu, or use the global hotkeys."
