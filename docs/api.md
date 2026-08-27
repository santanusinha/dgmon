---
icon: lucide/code
---

# API Reference

## Server endpoints

| Method | Path | Description |
|---|---|---|
| `POST` | `/ingest` | Receive a snapshot push from a collector agent. |
| `GET` | `/metrics` | All nodes in Prometheus exposition format. |
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

### Metrics

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/metrics` | List available metric names. |
| `GET` | `/api/v1/metrics/{name}` | Latest value(s) for a metric. |
| `GET` | `/api/v1/metrics/{name}/history?start=<ms>&end=<ms>` | Time-series for a metric. |

### Prometheus API

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

- All responses are JSON.
- Timestamps are milliseconds since the Unix epoch.
- Errors use a consistent shape:
  `{"error": {"code": "...", "message": "..."}}`.

### Control plane

The control plane lets an operator perform basic node operations from the
central server. It is **disabled by default**. Enable it in the server
config:

```json
{
  "control": {
    "enabled": true
  }
}
```

When enabled, the following routes are available under `/api/v1/control/`:

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/v1/control/nodes/{hostname}/restart` | Queue a restart for a node. |
| `POST` | `/api/v1/control/nodes/{hostname}/shutdown` | Queue a shutdown for a node. |
| `GET` | `/api/v1/control/mailbox` | Poll for a pending command (agent). |
| `POST` | `/api/v1/control/mailbox/ack` | Ack + clear a pending command (agent). |
| `GET` | `/api/v1/control/nodes` | List nodes with pending commands. |

The push agent on each node polls the mailbox on every collection cycle.
When a command is pending, the agent acks it and executes it locally via
`sudo shutdown -r now` (restart) or `sudo shutdown -h now` (shutdown).

The agent identifies itself with the `User-Agent` header
`dgmon-push/<hostname>`.

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

# Queue a restart for a node (control plane must be enabled)
curl -X POST http://localhost:9401/api/v1/control/nodes/host1/restart

# Queue a shutdown for a node
curl -X POST http://localhost:9401/api/v1/control/nodes/host1/shutdown

# List nodes with pending commands
curl http://localhost:9401/api/v1/control/nodes
```
