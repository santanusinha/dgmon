---
icon: lucide/code-2
---

# Development

## Build

- `cargo build --release` — build the binary
- `cargo check` — type-check without codegen
- `cargo run -- --mock once` — quick test with mock data
- `cargo run -- --mock service --listen 127.0.0.1:9401` — test standalone service

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
  http.rs         — Shared actix-web handlers (dashboard, static, health, history)
  storage.rs      — TsinkStore wrapper (time-series storage)
  store.rs        — Shared multi-node in-memory store (NodeStore, NodeInfo)
  metric_name.rs  — Shared metric-name helpers (sanitize_metric_name, strip_engine_prefix)
  collect.rs      — CLI collector: once / loop
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

## Dependencies (kept minimal)

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

## Testing the push architecture

- Start server: `./target/release/dgmon server --listen 127.0.0.1:9402 --data-dir /tmp/dgmon-data`
- Start push agent: `./target/release/dgmon push --config examples/dgmon-push.json`
- Curl `http://localhost:9402/nodes` to see registered nodes
- Curl `http://localhost:9402/metrics` for Prometheus output
- Curl `http://localhost:9402/history?metric=dgmon_cpu_usage_pct&hostname=host1&start=0&end=$(date +%s)000` for historical data
- Curl `http://localhost:9402/metrics` for Prometheus output

## Security

`POST /ingest` has no authentication. Any client that can reach the server
can push a snapshot. This is a deliberate design choice for a trusted
network. Run the server on a trusted network or behind a reverse proxy.

Do not expose the server directly to the public internet. If you must, put
a reverse proxy (for example nginx or Caddy) in front of it and add
authentication there.
