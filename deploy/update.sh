#!/usr/bin/env bash
#
# dgmon updater.
#
# This script downloads the latest release binary from GitHub and replaces
# the installed binary. If dgmon runs as a systemd service, it restarts the
# service after the update.
#
# It updates:
#   /usr/local/bin/dgmon          the binary
#
# Usage:
#   sudo ./deploy/update.sh
#   curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/update.sh | sudo bash
#
set -euo pipefail

REPO="santanusinha/dgmon"
BIN_DST="/usr/local/bin/dgmon"
VERSION="${VERSION:-latest}"

if [[ "${EUID}" -ne 0 ]]; then
    echo "error: run this script as root (sudo)." >&2
    exit 1
fi

# Detect the architecture and map it to a release asset name.
detect_asset() {
    local arch
    arch="$(uname -m)"
    case "${arch}" in
        x86_64|amd64)  echo "dgmon-x86_64-unknown-linux-gnu" ;;
        aarch64|arm64) echo "dgmon-aarch64-unknown-linux-gnu" ;;
        *) echo "error: unsupported architecture ${arch}." >&2; exit 1 ;;
    esac
}

# Download the release binary and verify its sha256 checksum.
download_release() {
    local asset="$1"
    local base="https://github.com/${REPO}/releases/${VERSION}/download"
    local tmp
    tmp="$(mktemp -d)"
    echo "==> downloading ${asset}"
    curl -fsSL -o "${tmp}/dgmon" "${base}/${asset}"
    curl -fsSL -o "${tmp}/dgmon.sha256" "${base}/${asset}.sha256"
    (cd "${tmp}" && sha256sum -c dgmon.sha256)
    BIN_SRC="${tmp}/dgmon"
}

echo "==> dgmon updater"
echo ""

download_release "$(detect_asset)"

echo "==> replacing binary"
install -D -m 0755 "${BIN_SRC}" "${BIN_DST}"
echo "    ${BIN_DST}"

# Restart the systemd service if one is active.
UNIT=""
if systemctl is-active --quiet dgmon-server 2>/dev/null; then
    UNIT="dgmon-server"
elif systemctl is-active --quiet dgmon-push 2>/dev/null; then
    UNIT="dgmon-push"
fi

if [[ -n "${UNIT}" ]]; then
    echo "==> restarting ${UNIT}"
    systemctl restart "${UNIT}"
    echo "    status: systemctl status ${UNIT}"
    echo "    logs:   journalctl -u ${UNIT} -f"
else
    echo "==> no active systemd service found"
    echo "    the binary is updated. start it manually if needed."
fi

echo ""
echo "==> done"
echo "    binary: ${BIN_DST}"
echo "    version: $(dgmon --version 2>/dev/null || echo unknown)"
