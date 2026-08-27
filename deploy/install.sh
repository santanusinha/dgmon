#!/usr/bin/env bash
#
# dgmon installer.
#
# This script installs dgmon as a systemd service. It asks for the mode:
#   server  - the central aggregation server
#   push    - a collector agent on a GPU node
#   service - standalone single-node mode (collect + serve locally)
#
# It installs:
#   /usr/local/bin/dgmon          the binary
#   /etc/dgmon/dgmon.json         the config (push/server/service modes)
#   /etc/systemd/system/dgmon-*.service
#   /var/lib/dgmon                the time-series data dir (server/service mode only)
#
# Logs go to journald. View them with:
#   journalctl -u dgmon-server -f
#   journalctl -u dgmon-push -f
#   journalctl -u dgmon-service -f
#
# Usage:
#   sudo ./deploy/install.sh
#   curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/install.sh | sudo bash
#
set -euo pipefail

REPO="santanusinha/dgmon"
BIN_DST="/usr/local/bin/dgmon"
CONFIG_DIR="/etc/dgmon"
CONFIG_FILE="${CONFIG_DIR}/dgmon.json"
DATA_DIR="/var/lib/dgmon"
UNIT_DIR="/etc/systemd/system"
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
        x86_64|amd64)  echo "dgmon-x86_64-unknown-linux-musl" ;;
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
    # The checksum file names the asset (dist/<asset>). Rewrite it to name
    # the local file so sha256sum -c can find it.
    sed -i "s|dist/.*|dgmon|" "${tmp}/dgmon.sha256"
    (cd "${tmp}" && sha256sum -c dgmon.sha256)
    BIN_SRC="${tmp}/dgmon"
}

# Resolve the binary source. Prefer a local build, then a release binary,
# then crates.io.
BIN_SRC="${BIN_SRC:-}"
if [[ -z "${BIN_SRC}" ]]; then
    if [[ -f "target/release/dgmon" ]]; then
        BIN_SRC="target/release/dgmon"
    else
        download_release "$(detect_asset)"
    fi
fi

if [[ ! -f "${BIN_SRC}" ]]; then
    echo "error: binary not found at ${BIN_SRC}." >&2
    echo "build it first with: cargo build --release" >&2
    echo "or install it with:  cargo install dgmon" >&2
    exit 1
fi

echo "==> dgmon installer"
echo ""

MODE=""
while [[ "${MODE}" != "server" && "${MODE}" != "push" && "${MODE}" != "service" ]]; do
    read -r -p "Install mode (server|push|service): " MODE
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
elif [[ "${MODE}" == "server" || "${MODE}" == "service" ]]; then
    echo "==> writing config"
    cat > "${CONFIG_FILE}" <<EOF
{
  "interval_secs": 5,
  "mock": false,
  "labels": {
    "cluster": "dgx-spark-prod"
  },
  "data_dir": "${DATA_DIR}",
  "listen": "0.0.0.0:9401"
}
EOF
    chmod 0644 "${CONFIG_FILE}"
    echo "    ${CONFIG_FILE}"
    echo "    edit it to set labels and interval for this node."
fi
if [[ "${MODE}" == "server" || "${MODE}" == "service" ]]; then
    echo "==> creating data dir"
    install -d -m 0755 "${DATA_DIR}"
    echo "    ${DATA_DIR}"
fi

# Create a dedicated dgmon user for the push agent. This user runs the
# agent and has a sudoers rule that allows only the shutdown binary.
if [[ "${MODE}" == "push" ]]; then
    echo "==> creating dgmon user"
    if ! id -u dgmon >/dev/null 2>&1; then
        useradd --system --home-dir /var/lib/dgmon --shell /usr/sbin/nologin dgmon
    fi
    echo "    user dgmon"

    echo "==> installing sudoers rule"
    install -d -m 0750 /etc/sudoers.d
    cat > /etc/sudoers.d/dgmon <<'EOF'
# Allow the dgmon user to run only the shutdown binary.
dgmon ALL=(root) NOPASSWD: /usr/sbin/shutdown
EOF
    chmod 0440 /etc/sudoers.d/dgmon
    echo "    /etc/sudoers.d/dgmon"
fi
echo "==> installing systemd unit"
if [[ "${MODE}" == "server" ]]; then
    cat > "${UNIT_DIR}/dgmon-server.service" <<'EOF'
[Unit]
Description=dgmon aggregation server
Documentation=https://github.com/santanusinha/dgmon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/dgmon server --config /etc/dgmon/dgmon.json
Environment=DGMON_CONFIG=/etc/dgmon/dgmon.json
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/dgmon
StateDirectory=dgmon

[Install]
WantedBy=multi-user.target
EOF
    UNIT="dgmon-server"
elif [[ "${MODE}" == "service" ]]; then
    cat > "${UNIT_DIR}/dgmon-service.service" <<'EOF'
[Unit]
Description=dgmon standalone service
Documentation=https://github.com/santanusinha/dgmon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/usr/local/bin/dgmon service --config /etc/dgmon/dgmon.json
Environment=DGMON_CONFIG=/etc/dgmon/dgmon.json
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/dgmon
ReadOnlyPaths=/etc/dgmon
StateDirectory=dgmon

[Install]
WantedBy=multi-user.target
EOF
    UNIT="dgmon-service"
else
    cat > "${UNIT_DIR}/dgmon-push.service" <<'EOF'
[Unit]
Description=dgmon push agent
Documentation=https://github.com/santanusinha/dgmon
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=dgmon
Group=dgmon
ExecStart=/usr/local/bin/dgmon push --config /etc/dgmon/dgmon.json
Environment=DGMON_CONFIG=/etc/dgmon/dgmon.json
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadOnlyPaths=/etc/dgmon

[Install]
WantedBy=multi-user.target
EOF
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
