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
| `--config <file>` | `DGMON_CONFIG` | (none) | `push`, `service` | Path to the JSON config file (inference servers, interface roles). |

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
  },
  "inference_servers": ["http://127.0.0.1:8000"],
  "interface_role_overrides": {
    "enp1s0f0np0": "cluster"
  }
}
```

| Key | Default | Description |
|---|---|---|
| `server_url` | (required) | URL of the dgmon server ingest endpoint. |
| `interval_secs` | `5` | Push interval in seconds. |
| `mock` | `false` | Use the mock collector instead of `nvidia-smi`. |
| `labels` | `{}` | Extra labels merged into every snapshot from this node. |
| `inference_servers` | `[]` | Manual inference server base URLs (e.g. `http://127.0.0.1:8000`). When set, discovery is skipped for these. |
| `interface_role_overrides` | `{}` | Optional per-interface role overrides. Keys are interface names, values are roles (`main`, `cluster`, `other`). |
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

### Prometheus API (requires `--data-dir`)

These endpoints follow the Prometheus HTTP API envelope. A Prometheus
datasource can point directly at dgmon without a proxy.

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/query?query=<expr>&time=<s>` | Instant query. |
| `GET` | `/api/v1/query_range?query=<expr>&start=<s>&end=<s>&step=<s>` | Range query. |
| `GET` | `/api/v1/labels` | List label names. |
| `GET` | `/api/v1/label/{name}/values` | List values for one label. |
| `GET` | `/api/v1/status/buildinfo` | Version info. |
| `POST` | `/api/v1/query_batch` | Evaluate many queries in one round trip. |

#### Batch query

`POST /api/v1/query_batch` evaluates many fixed PromQL queries in one HTTP
round trip. This is useful for low-power clients (for example an ESP32
firmware) that poll a fixed set of queries per screen or widget.

Request body:

```json
{
  "queries": [
    { "id": "overview.cpu", "expr": "dgmon_cpu_usage_pct" },
    { "id": "overview.gpu_util", "expr": "max by (hostname) (dgmon_gpu_utilization)" },
    { "id": "detail.node1.gpu0.temp", "expr": "dgmon_gpu_temp_c{hostname=\"node1\",gpu=\"0\"}" }
  ]
}
```

- `id` is a client-chosen string. It must be unique within the request.
- `expr` is a PromQL expression.
- Optional `time` (unix seconds) per query. Defaults to now.
- Optional `range` object for a range query:
  `{ "start": <s>, "end": <s>, "step": <s> }`.

Response:

```json
{
  "status": "success",
  "data": {
    "overview.cpu": { "resultType": "vector", "result": [...] },
    "overview.gpu_util": { "resultType": "vector", "result": [...] }
  }
}
```

`data` maps each query id to its Prometheus-style result. On error for one
query, that id maps to `{ "error": "..." }`. Other queries still succeed.

### Response conventions
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
- **CPU cores** (per core): utilization percentage.
- **Network interfaces** (per interface): role (main/cluster/other), bytes
  and packets received/transmitted, link speed, up state.
- **GPU** (per device): index, UUID, model name, GPU utilization, memory
  utilization, temperature, memory temperature, power draw, power limit,
  memory used/total, fan speed, P-state, SM clock, memory clock, max clocks,
  PCIe link generation and width.
- **Inference** (per engine): scraped Prometheus metrics from sglang and
  vLLM, tagged with the engine and model name.

### Inference metrics

The push agent (and service mode) discovers local inference servers and
scrapes their `/metrics` endpoint. Discovery order:

1. Manual config targets (`inference_servers` in the config file).
2. Docker containers (bollard) running sglang or vLLM images.
3. Process table (sysinfo) for sglang/vLLM processes.
4. `netstat` output as a last resort.

Each discovered server is scraped for `/metrics` and the model name is
fetched from the `/v1/models` API. All metrics are captured and stored with
`engine` and `model_name` labels. If no inference server is found, the
snapshot contains no inference metrics and does not fail.

### Metadata labels

Every metric carries the `hostname` label plus any `extra` labels from the
config (e.g. `cluster`, `rack`, `node_role`). Inference metrics also carry
`engine` and `model_name`. This lets you filter in PromQL:

```promql
# All GPU utilization for the production cluster
{dgmon_gpu_utilization}{cluster="dgx-spark-prod"}

# Generation tokens for a specific model
rate(dgmon_inference_generation_tokens_total{model_name="llama-3-8b"}[5m])
```

## Project layout

```
src/
  main.rs         — CLI entry point, mode dispatch
  config.rs       — Push agent JSON config
  collector.rs    — Collector trait + Snapshot/GpuSample/HostSample structs
  collector/
    nvidia.rs     — NvidiaSmiCollector (nvidia-smi CSV)
    mock.rs       — MockCollector (fake data)
  inference.rs    — Inference discovery + scraping (sglang/vLLM)
  push.rs         — Push agent: async collect + POST to server
  server.rs       — Aggregation server: receive pushes, expose /metrics
  service.rs      — Standalone single-node service
  http.rs         — Shared actix-web handlers (dashboard, static, health, history, query)
  storage.rs      — TsinkStore wrapper (time-series storage)
  collect.rs      — CLI collector: once / loop
```

## License

Apache-2.0