---
icon: lucide/download
---

# Install

## Installer script

The quickest way to install dgmon is with the installer script. It detects
the local architecture, downloads the matching release binary from GitHub,
and sets up dgmon as a systemd service. It asks for the mode:

- `server` — the central aggregation server
- `push` — a collector agent on a GPU node

```sh
# On the central node:
curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/install.sh | sudo bash

# On each GPU node:
curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/install.sh | sudo bash
```

The installer copies the binary to `/usr/local/bin/dgmon`, writes the push
config to `/etc/dgmon/dgmon.json` (push mode), installs the systemd unit,
and enables it. Logs go to journald:

```sh
journalctl -u dgmon-server -f
journalctl -u dgmon-push -f
```

To remove the service and its config:

```sh
sudo ./deploy/uninstall.sh
```

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

## Cluster deployment (manual)

```sh
# On the central node:
dgmon server --listen 0.0.0.0:9401

# On each GPU node:
dgmon push --config /etc/dgmon/dgmon.json
```

## Single node

```sh
# Collect and serve directly:
dgmon service --listen 0.0.0.0:9401
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
