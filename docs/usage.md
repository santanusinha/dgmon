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
| `--listen <addr>` | `DGMON_LISTEN` | `0.0.0.0:9401` | `server`, `service` | Address and port to bind the HTTP server to. |
| `--data-dir <path>` | `DGMON_DATA_DIR` | (required) | `server`, `service` | Path for time-series storage. Adds `/history`. |
| `--config <file>` | `DGMON_CONFIG` | (required) | `push`, `server`, `service` | Path to the JSON config file (inference servers, interface roles). |

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
`server_url` key is used by `push` only; it is optional for `server` and
`service`.

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
  }
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

See `examples/dgmon-push.json` for a complete example.
