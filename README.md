# dgmon

A lightweight system monitor for NVIDIA DGX Spark GPU clusters.

## Architecture

dgmon uses a **push** architecture for cluster deployments:

```
  Node 1                Node 2                Node N
 ┌──────────┐         ┌──────────┐         ┌──────────┐
 │ dgmon    │         │ dgmon    │         │ dgmon    │
 │ push     │         │ push     │         │ push     │
 │ (collect  │         │ (collect  │         │ (collect  │
 │  + POST) │         │  + POST) │         │  + POST) │
 └────┬─────┘         └────┬─────┘         └────┬─────┘
      │                    │                    │
      └─────── HTTP ───────┼────────────────────┘
              POST /ingest │
                           ▼
                    ┌──────────────┐
                    │  dgmon       │
                    │  server      │
                    │              │
                    │ /metrics     │ ← Prometheus scrapes here
                    │ /snapshot   │
                    │ /nodes       │
                    │ /health      │
                    └──────────────┘
```

- Each GPU node runs `dgmon push` as a service (systemd).
- One central node (or a separate management node) runs `dgmon server`.
- Collectors push snapshots to the server via HTTP POST.
- Prometheus scrapes `/metrics` on the server to get all nodes in one pull.

For single-node use, `dgmon service` collects locally and serves directly.

## Commands

| Command | Purpose |
|---|---|
| `dgmon server` | Aggregation server. Receives pushes, exposes `/metrics`, `/snapshot`, `/nodes`, `/health`. |
| `dgmon push --config <file>` | Collector agent. Collects locally, pushes to a remote server. |
| `dgmon service` | Standalone single-node mode. Collects locally, serves HTTP directly. |
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
| `--data-dir <path>` | `DGMON_DATA_DIR` | (none) | `server`, `service` | Enable time-series storage at this path. Adds `/history`, `/query`, and `/metrics/list`. |
| `--config <file>` | `DGMON_CONFIG` | (none) | `push` | Path to the push agent JSON config file. |

### Examples

```sh
# Run the server on a specific port with history enabled
dgmon server --listen 0.0.0.0:9402 --data-dir /var/lib/dgmon

# Collect every 10 seconds using mock data
dgmon --mock --interval 10 loop

# Push agent reading its config from a file
dgmon push --config /etc/dgmon/dgmon.json
```

> Tip: run `dgmon --help` for the full list of options, and
> `dgmon <command> --help` for options specific to one command.

## Push agent config

Each push agent reads a JSON config file:

```json
{
  "server_url": "http://10.0.0.1:9401/ingest",
  "interval_secs": 5,
  "mock": false,
  "labels": {
    "cluster": "dgx-spark-prod",
    "rack": "r1"
  }
}
```

| Key | Default | Description |
|---|---|---|
| `server_url` | (required) | URL of the dgmon server ingest endpoint. |
| `interval_secs` | `5` | Push interval in seconds. |
| `mock` | `false` | Use the mock collector instead of `nvidia-smi`. |
| `labels` | `{}` | Extra labels merged into every snapshot from this node. |

See `examples/dgmon-push.json` for a complete example.

## Server endpoints

| Method | Path | Description |
|---|---|---|
| `POST` | `/ingest` | Receive a snapshot push from a collector agent. |
| `GET` | `/metrics` | All nodes in Prometheus exposition format. |
| `GET` | `/snapshot` | All nodes as a JSON array of snapshots. |
| `GET` | `/nodes` | List of node hostnames, GPU counts, and last-seen timestamps. |
| `GET` | `/health` | Returns `ok` (for liveness probes). |

## REST API

A versioned, resource-oriented REST API is available under `/api/v1/` in both
`server` and `service` modes. It is designed for cross-device querying from
browsers, scripts, and mobile apps. CORS is enabled for all origins.

### Nodes

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/nodes` | List all nodes (hostname, GPU count, last-seen time). |
| `GET` | `/api/v1/nodes/{hostname}` | Latest full snapshot for one node. |
| `GET` | `/api/v1/nodes/{hostname}/host` | Host metrics only. |
| `GET` | `/api/v1/nodes/{hostname}/gpus` | GPU list for one node. |
| `GET` | `/api/v1/nodes/{hostname}/gpus/{index}` | One GPU by index. |

### Metrics (requires `--data-dir`)

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/metrics` | List available metric names. |
| `GET` | `/api/v1/metrics/{name}` | Latest value(s) for a metric. |
| `GET` | `/api/v1/metrics/{name}/history?start=<ms>&end=<ms>` | Time-series for a metric. |

### Response conventions

- All responses are JSON.
- Timestamps are milliseconds since the Unix epoch.
- Errors use a consistent shape:
  `{"error": {"code": "...", "message": "..."}}`.

### Examples

```sh
# List nodes
curl http://localhost:9401/api/v1/nodes

# Latest snapshot for one node
curl http://localhost:9401/api/v1/nodes/host1

# GPU list for one node
curl http://localhost:9401/api/v1/nodes/host1/gpus

# Metric history (last hour)
curl 'http://localhost:9401/api/v1/metrics/dgmon_cpu_usage_pct/history?start=0&end=$(date +%s)000'
```

## Collector abstraction

Every GPU vendor implements the `Collector` trait (`src/collector.rs`).

| Module | Backend | Status |
|---|---|---|
| `collector/nvidia.rs` | `nvidia-smi` CSV | Implemented |
| `collector/mock.rs` | Fake data | Implemented |
| `collector/amd.rs` | `rocm-smi` | Future |
| `collector/intel.rs` | `intel-smi` | Future |

To support a new GPU vendor, add a module under `src/collector/` and implement
the `Collector` trait. No server or push agent changes are needed.

## Usage

```sh
# Build
cargo build --release

# --- Cluster deployment ---

# On the central node:
./target/release/dgmon server --listen 0.0.0.0:9401

# On each GPU node:
./target/release/dgmon push --config /etc/dgmon/dgmon.json

# --- Single node ---

# Collect and serve directly:
./target/release/dgmon service --listen 0.0.0.0:9401

# --- CLI debugging ---

# Collect once:
./target/release/dgmon once

# Collect on a loop:
./target/release/dgmon loop

# Use mock data (no GPU needed):
./target/release/dgmon --mock once
```

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `DGMON_MOCK` | unset | Use mock collector. |
| `DGMON_INTERVAL` | `5` | Collection interval in seconds. |
| `DGMON_LISTEN` | `0.0.0.0:9401` | Listen address (server/service mode). |
| `DGMON_CONFIG` | (none) | Path to push config file (push mode). |
| `RUST_LOG` | `dgmon=info` | Tracing log level. |

## Metrics

Each snapshot contains:

- **Host**: hostname, CPU usage, memory, disk, network counters, uptime.
- **GPU** (per device): index, UUID, model name, GPU utilization, memory
  utilization, temperature, power draw, power limit, memory used/total, fan
  speed, P-state, XID error count.

## Project layout

```
src/
  main.rs         — CLI entry point, mode dispatch
  config.rs       — Push agent JSON config
  collector.rs    — Collector trait + Snapshot/GpuSample/HostSample structs
  collector/
    nvidia.rs     — NvidiaSmiCollector (nvidia-smi CSV)
    mock.rs       — MockCollector (fake data)
  push.rs         — Push agent: collect + POST to server
  server.rs       — Aggregation server: receive pushes, expose /metrics
  service.rs      — Standalone single-node service
  http.rs         — Shared actix-web handlers (dashboard, static, health, history, query)
  storage.rs      — TsinkStore wrapper (optional time-series storage)
  collect.rs      — CLI collector: once / loop
```

## License

Apache-2.0