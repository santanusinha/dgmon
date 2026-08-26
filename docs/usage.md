---
icon: lucide/terminal
---

# Usage

## Commands

| Command | Purpose |
|---|---|
| `dgmon server --config <file>` | Aggregation server. Receives pushes, exposes `/metrics`, `/nodes`, `/health`. |
| `dgmon push --config <file>` | Collector agent. Collects locally, pushes to a remote server. |
| `dgmon service --config <file>` | Standalone single-node mode. Collects locally, serves HTTP directly. |
| `dgmon once` | Collect once, print to stdout. |
| `dgmon loop` | Collect on a loop, print to stdout. |

## Command-line options

All commands accept the global options below. You can set each one with a
flag or an environment variable.

| Option | Env var | Default | Applies to | Description |
|---|---|---|---|---|
| `--mock` | `DGMON_MOCK` | off | all | Use the mock collector instead of `nvidia-smi`. Useful for testing without a GPU. |
| `--interval <secs>` | `DGMON_INTERVAL` | `5` | `push`, `service`, `loop` | How often to collect a snapshot, in seconds. |
| `--listen <addr>` | `DGMON_LISTEN` | `0.0.0.0:9401` | `server`, `service` | Address and port to bind the HTTP server to. Overrides the config file value. |
| `--data-dir <path>` | `DGMON_DATA_DIR` | (from config) | `server`, `service` | Path for time-series storage. Adds `/history`. Overrides the config file value. |
| `--config <file>` | `DGMON_CONFIG` | (none) | `push`, `server`, `service` | Path to the JSON config file. For `server`/`service`, optional when `--data-dir` is given. Required for `push`. |

### Examples

```sh
# Run the server on a specific port with history enabled
dgmon server --listen 0.0.0.0:9402 --data-dir /var/lib/dgmon --config /etc/dgmon/dgmon.json

# Collect every 10 seconds using mock data
dgmon --mock --interval 10 loop

# Push agent reading its config from a file
dgmon push --config /etc/dgmon/dgmon.json
```

> Tip: run `dgmon --help` for the full list of options, and
> `dgmon <command> --help` for options specific to one command.

## Config file

Each of `push`, `server`, and `service` reads a JSON config file. The
config holds the base setup. Command-line flags override config values.
For `server` and `service`, the config can carry `data_dir` and `listen`;
when the config is omitted, `--data-dir` becomes mandatory.

```json
{
  "server_url": "http://10.0.0.1:9401/ingest",
  "interval_secs": 5,
  "mock": false,
  "labels": {
    "cluster": "dgx-spark-prod",
    "rack": "r1"
  },
  "inference_servers": ["http://127.0.0.1:8000"],
  "interface_role_overrides": {
    "enp1s0f0np0": "cluster"
  },
  "data_dir": "/var/lib/dgmon",
  "listen": "0.0.0.0:9401"
}
```

| Key | Default | Description |
|---|---|---|
| `server_url` | (push only) | URL of the dgmon server ingest endpoint. Required for `push`. |
| `interval_secs` | `5` | Push interval in seconds. |
| `mock` | `false` | Use the mock collector instead of `nvidia-smi`. |
| `labels` | `{}` | Extra labels merged into every snapshot from this node. |
| `inference_servers` | `[]` | Manual inference server base URLs (e.g. `http://127.0.0.1:8000`). When set, discovery is skipped for these. |
| `interface_role_overrides` | `{}` | Optional per-interface role overrides. Keys are interface names, values are roles (`main`, `cluster`, `other`). |
| `data_dir` | (none) | Data directory for time-series storage (`server`, `service`). Required when `--data-dir` is not given. |
| `listen` | `0.0.0.0:9401` | Listen address for the HTTP server (`server`, `service`). |

See `examples/dgmon-push.json` for a push example and
`examples/dgmon-server.json` for a server example.
