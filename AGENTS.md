# Build
- `cargo build --release` — build the binary
- `cargo check` — type-check without codegen
- `cargo run -- --mock once` — quick test with mock data
- `cargo run -- --mock service --listen 127.0.0.1:9401` — test standalone service

# Architecture
Push-based cluster monitoring:
- `dgmon push` runs on every GPU node, collects locally, POSTs snapshots to a central server.
- `dgmon server` runs on one node, receives pushes, exposes /metrics for Prometheus.
- `dgmon service` is the standalone single-node mode (collect + serve locally).

# Source layout
- `src/main.rs` — CLI entry point (clap), mode dispatch
- `src/config.rs` — Push agent JSON config (server_url, interval, labels, mock, inference_servers, interface_role_overrides)
- `src/collector.rs` — Collector trait + data structs (Snapshot, GpuSample, HostSample, CpuCoreSample, NetSample, InferenceSample)
- `src/collector/nvidia.rs` — NvidiaSmiCollector (reads nvidia-smi CSV, per-CPU-core, per-interface network)
- `src/collector/mock.rs` — MockCollector (fake data for testing)
- `src/inference.rs` — Inference discovery + scraping (sglang/vLLM)
- `src/push.rs` — Push agent: async collect loop + HTTP POST to server
- `src/server.rs` — Aggregation server: /ingest, /metrics, /nodes, /history, /health
- `src/service.rs` — Standalone single-node service: /nodes, /metrics, /history, /health
- `src/api.rs` — Versioned REST API under /api/v1/ (nodes, gpus, metrics)
- `src/promapi.rs` — Prometheus-compatible API under /api/v1/ (query, query_range, query_batch, labels, label values, buildinfo)
- `src/http.rs` — Shared actix-web handlers (dashboard, static, health, history)
- `src/storage.rs` — TsinkStore wrapper (time-series storage)
- `src/store.rs` — Shared multi-node in-memory store (NodeStore, NodeInfo)
- `src/metric_name.rs` — Shared metric-name helpers (sanitize_metric_name, strip_engine_prefix)
- `src/collect.rs` — CLI collector: once / loop

# Time-series storage
- When the server or service starts with `--data-dir <path>`, every snapshot
  is written to a tsink embedded time-series database at that path.
- The tsink database stores full history with a 30-day retention window.
- Without `--data-dir`, the server and service operate in memory-only mode and store
  only the latest snapshot per node for Prometheus scraping.
- The `DGMON_DATA_DIR` environment variable sets the same option.

# HTTP API routes
- `GET /` — HTML dashboard with endpoint links
- `GET /health` — returns `ok` (liveness probe)
- `GET /metrics` — Prometheus text format output
- `POST /ingest` — accepts a JSON snapshot from a push agent (server mode only)
- `GET /history?metric=<name>&hostname=<host>&start=<ms>&end=<ms>` — JSON time-series query (requires `--data-dir`)

# REST API routes (/api/v1/)
- `GET /api/v1/nodes` — list nodes
- `GET /api/v1/nodes/{hostname}` — latest snapshot for one node
- `GET /api/v1/nodes/{hostname}/host` — host metrics only
- `GET /api/v1/nodes/{hostname}/gpus` — GPU list for one node
- `GET /api/v1/nodes/{hostname}/gpus/{index}` — one GPU
- `GET /api/v1/metrics` — list available metric names (requires `--data-dir`)
- `GET /api/v1/metrics/{name}` — latest value(s) for a metric (requires `--data-dir`)
- `GET /api/v1/metrics/{name}/history?start=<ms>&end=<ms>` — time-series for a metric (requires `--data-dir`)
- `GET /api/v1/query?query=<expr>&time=<s>` — Prometheus instant query (requires `--data-dir`)
- `GET /api/v1/query_range?query=<expr>&start=<s>&end=<s>&step=<s>` — Prometheus range query (requires `--data-dir`)
- `GET /api/v1/labels` — list label names (requires `--data-dir`)
- `GET /api/v1/label/{name}/values` — list values for one label (requires `--data-dir`)
- `GET /api/v1/status/buildinfo` — version info
- `POST /api/v1/query_batch` — evaluate many queries in one round trip (requires `--data-dir`)
- CORS is enabled for all origins.

# Key design decisions
- Push architecture: nodes push data to a central server. No polling needed.
  This scales better for large clusters — the server does not need to know
  every node's address.
- The collector agent config is a simple JSON file with server_url, interval,
  mock flag, custom labels (cluster, rack, etc.), inference servers, and
  interface role overrides.
- The server stores the latest snapshot per node in a HashMap keyed by hostname.
  Prometheus scrapes one endpoint to get all nodes.
- No link-time dependency on libnvidia-ml; NVIDIA collector shells out to
  nvidia-smi and parses CSV. Binary runs on any DGX with the driver installed.
- The Collector trait is the vendor abstraction. New GPU vendors implement
  it in a new module under src/collector/. No server or push changes needed.
- The push agent and service mode use an async tokio runtime (reqwest) to
  keep system load low. Collection stays blocking and runs on a blocking task.
- Inference discovery: bollard (docker inspect) → process table → netstat →
  manual config. Re-run periodically. Capture all metrics with engine and
  model_name labels.
- Metadata (hostname, cluster, rack, model_name) is stored as labels on every
  metric for filtering in PromQL.

# Dependencies (kept minimal)
- clap (CLI + env var support)
- serde / serde_json (serialization)
- actix-web / actix-rt (async HTTP server)
- tokio (async runtime)
- reqwest (async HTTP client for push + inference scraping)
- bollard (Docker API client for inference discovery)
- sysinfo (host metrics: CPU, memory, disk, network, per-core, per-interface)
- chrono (timestamps)
- tracing / tracing-subscriber (structured logging)
- anyhow (error handling)
- tsink (embedded time-series database for historical metric storage)

# Testing the push architecture
- Start server: `./target/release/dgmon server --listen 127.0.0.1:9402`
- Start push agent: `./target/release/dgmon push --config examples/dgmon-push.json`
- Curl `http://localhost:9402/nodes` to see registered nodes
- Curl `http://localhost:9402/metrics` for Prometheus output
- Curl `http://localhost:9402/history?metric=dgmon_cpu_usage_pct&hostname=host1&start=0&end=$(date +%s)000` for historical data
- Curl `http://localhost:9402/metrics` for Prometheus output