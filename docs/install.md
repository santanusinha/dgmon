---
icon: lucide/download
---

# Install

dgmon supports several deployment strategies. Choose the one that fits your
setup. Most people have one or two DGX nodes, so start there.

## Single node

For one DGX node, run the standalone service. It collects locally and serves
the dashboard and API on the same machine.

To run it as a systemd service, use the installer script and choose
`service` mode:

```sh
curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/install.sh | sudo bash
```

The installer detects the local architecture, downloads the matching release
binary from GitHub, and sets up the service. Logs go to journald:

```sh
journalctl -u dgmon-service -f
```

To avoid the installer, either download the binary from github or run `cargo install dgmon` to install.
Then use the following command to run it:

```sh
dgmon service --listen 0.0.0.0:9401 --config /etc/dgmon/dgmon.json
```

Open the dashboard at `http://<node-ip>:9401/`.

## Two nodes

For two DGX nodes, run the server on one node and the push agent on the
other. The server node collects nothing itself; it only aggregates.

To run both as systemd services, use the installer script on each node and
choose the matching mode:

```sh
# On the server node, choose "server":
curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/install.sh | sudo bash

# On the other node, choose "push":
curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/install.sh | sudo bash
```

The installer writes the config to `/etc/dgmon/dgmon.json` and enables the
service. Logs go to journald:

```sh
journalctl -u dgmon-server -f
journalctl -u dgmon-push -f
```

To avoid the installer, either download the binary from github or run `cargo install dgmon` to install.

On the **server node**:

```sh
dgmon server --listen 0.0.0.0:9401 --config /etc/dgmon/dgmon.json
```

On the **other node**, create a push config and run the push agent:

```sh
dgmon push --config /etc/dgmon/dgmon.json
```

The push config points at the server node:

```json
{
  "server_url": "http://<server-node-ip>:9401/ingest",
  "interval_secs": 5,
  "mock": false,
  "labels": {
    "cluster": "dgx-spark-prod",
    "rack": "r1"
  }
}
```

See [Usage](usage.md) for the full config reference.

## Larger cluster

For three or more nodes, use the same push architecture as the two-node
setup. One central node runs `dgmon server`; every other node runs
`dgmon push`.

Use the installer script on each node, choosing `server` on the central
node and `push` on every other node:

```sh
curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/install.sh | sudo bash
```


To avoid the installer, either download the binary from github or run `cargo install dgmon` to install.
Then use the following command to run it:

```sh
# On the central node:
dgmon server --listen 0.0.0.0:9401 --config /etc/dgmon/dgmon.json

# On each GPU node:
dgmon push --config /etc/dgmon/dgmon.json
```

See [Architecture](architecture.md) for how the pieces fit together.

## Install from crates.io

```sh
cargo install dgmon
```

This installs the `dgmon` binary to `~/.cargo/bin/dgmon`. Add that
directory to your `PATH` if it is not already there.

## Install from source

```sh
git clone https://github.com/santanusinha/dgmon
cd dgmon
cargo build --release
```

The binary is at `target/release/dgmon`.

## Remove the service

To remove the service and its config:

```sh
sudo ./deploy/uninstall.sh
```

## Update the binary

To update the installed binary to the latest release from GitHub, run the
updater script:

```sh
sudo ./deploy/update.sh
```

Or run it directly from GitHub:

```sh
curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/update.sh | sudo bash
```

The updater detects the local architecture, downloads the matching release
binary, verifies its sha256 checksum, and replaces `/usr/local/bin/dgmon`.
If dgmon runs as a systemd service, the updater restarts it after the
update.

To pin a specific version instead of `latest`, set the `VERSION` variable:

```sh
VERSION=v0.1.0 sudo ./deploy/update.sh
```

## CLI debugging

```sh
# Collect once:
dgmon once

# Collect on a loop:
dgmon loop

# Use mock data (no GPU needed):
dgmon --mock once
```
