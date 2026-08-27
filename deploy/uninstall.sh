#!/usr/bin/env bash
#
# dgmon systemd uninstaller.
#
# This script removes the dgmon systemd service, its config, and its data.
# It does NOT remove the binary from /usr/local/bin.
#
# Usage:
#   sudo ./deploy/uninstall.sh
#
set -euo pipefail

CONFIG_DIR="/etc/dgmon"
DATA_DIR="/var/lib/dgmon"
UNIT_DIR="/etc/systemd/system"

if [[ "${EUID}" -ne 0 ]]; then
    echo "error: run this script as root (sudo)." >&2
    exit 1
fi

echo "==> dgmon systemd uninstaller"
echo ""

# Stop and disable any dgmon unit that exists.
for UNIT in dgmon-server dgmon-push dgmon-service; do
    if systemctl list-unit-files | grep -q "^${UNIT}\\.service"; then
        echo "==> stopping and disabling ${UNIT}"
        systemctl stop "${UNIT}" || true
        systemctl disable "${UNIT}" || true
        rm -f "${UNIT_DIR}/${UNIT}.service"
    fi
done

echo "==> reloading systemd"
systemctl daemon-reload

if [[ -d "${CONFIG_DIR}" ]]; then
    echo "==> removing config dir"
    rm -rf "${CONFIG_DIR}"
fi

if [[ -d "${DATA_DIR}" ]]; then
    echo "==> removing data dir"
    rm -rf "${DATA_DIR}"
fi

echo "==> removing sudoers rule"
rm -f /etc/sudoers.d/dgmon

echo "==> removing dgmon user"
if id -u dgmon >/dev/null 2>&1; then
    userdel dgmon || true
fi

echo ""
echo "==> done"
echo "    binary still at /usr/local/bin/dgmon (remove it manually if wanted)."
