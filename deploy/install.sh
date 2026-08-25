#!/usr/bin/env bash
#
# dgmon systemd installer.
#
# This script installs dgmon as a systemd service. It asks for the mode:
#   server - the central aggregation server
#   push   - a collector agent on a GPU node
#
# It installs:
#   /usr/local/bin/dgmon          the binary
#   /etc/dgmon/dgmon.json         the push config (push mode only)
#   /etc/systemd/system/dgmon-*.service
#   /var/lib/dgmon                the time-series data dir (server mode only)
#
# Logs go to journald. View them with:
#   journalctl -u dgmon-server -f
#   journalctl -u dgmon-push -f
#
# Usage:
#   sudo ./deploy/install.sh
#   sudo BIN_SRC= ./deploy/install.sh   # install from crates.io
#
set -euo pipefail

BIN_SRC="${BIN_SRC:-target/release/dgmon}"
BIN_DST="/usr/local/bin/dgmon"
CONFIG_DIR="/etc/dgmon"
CONFIG_FILE="${CONFIG_DIR}/dgmon.json"
DATA_DIR="/var/lib/dgmon"
UNIT_DIR="/etc/systemd/system"

if [[ "${EUID}" -ne 0 ]]; then
    echo "error: run this script as root (sudo)." >&2
    exit 1
fi

# If BIN_SRC is not a local file, install from crates.io.
if [[ ! -f "${BIN_SRC}" ]]; then
    echo "==> installing dgmon from crates.io"
    cargo install dgmon
    BIN_SRC="$(command -v dgmon || echo ~/.cargo/bin/dgmon)"
fi

if [[ ! -f "${BIN_SRC}" ]]; then
    echo "error: binary not found at ${BIN_SRC}." >&2
    echo "build it first with: cargo build --release" >&2
    echo "or install it with:  cargo install dgmon" >&2
    exit 1
fi

echo "==> dgmon systemd installer"
echo ""

# Ask for the mode.
MODE=""
while [[ "${MODE}" != "server" && "${MODE}" != "push" ]]; do
    read -r -p "Install mode (server|push): " MODE
    MODE="$(echo "${MODE}" | tr '[:upper:]' '[:lower:]')"
done

# Ask for the server node IP in push mode.
SERVER_IP=""
if [[ "${MODE}" == "push" ]]; then
    while [[ -z "${SERVER_IP}" ]]; do
        read -r -p "IP of the dgmon server node: " SERVER_IP
    done
fi

echo ""
echo "==> installing binary"
install -D -m 0755 "${BIN_SRC}" "${BIN_DST}"
echo "    ${BIN_DST}"

echo "==> creating config dir"
install -d -m 0755 "${CONFIG_DIR}"
echo "    ${CONFIG_DIR}"

if [[ "${MODE}" == "push" ]]; then
    echo "==> writing push config"
    cat > "${CONFIG_FILE}" <<EOF
{
  "server_url": "http://${SERVER_IP}:9401/ingest",
  "interval_secs": 5,
  "mock": false,
  "labels": {
    "cluster": "dgx-spark-prod",
    "rack": "r1",
    "node_role": "worker"
  }
}
EOF
    chmod 0644 "${CONFIG_FILE}"
    echo "    ${CONFIG_FILE}"
    echo "    edit it to set labels and interval for this node."
fi

if [[ "${MODE}" == "server" ]]; then
    echo "==> creating data dir"
    install -d -m 0755 "${DATA_DIR}"
    echo "    ${DATA_DIR}"
fi

echo "==> installing systemd unit"
if [[ "${MODE}" == "server" ]]; then
    install -D -m 0644 deploy/systemd/dgmon-server.service "${UNIT_DIR}/dgmon-server.service"
    UNIT="dgmon-server"
else
    install -D -m 0644 deploy/systemd/dgmon-push.service "${UNIT_DIR}/dgmon-push.service"
    UNIT="dgmon-push"
fi
echo "    ${UNIT_DIR}/${UNIT}.service"

echo "==> reloading systemd"
systemctl daemon-reload

echo "==> enabling and starting ${UNIT}"
systemctl enable --now "${UNIT}"

echo ""
echo "==> done"
echo "    service: ${UNIT}"
echo "    status:  systemctl status ${UNIT}"
echo "    logs:    journalctl -u ${UNIT} -f"
