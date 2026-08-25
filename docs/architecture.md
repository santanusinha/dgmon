---
icon: lucide/network
---

# Architecture

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
                    │ /nodes       │
                    │ /health      │
                    └──────────────┘
```

- Each GPU node runs `dgmon push` as a service (systemd).
- One central node (or a separate management node) runs `dgmon server`.
- Collectors push snapshots to the server via HTTP POST.
- Prometheus scrapes `/metrics` on the server to get all nodes in one pull.

For single-node use, `dgmon service` collects locally and serves directly.

## Design decisions

- **Push architecture**: nodes push data to a central server. No polling
  needed. This scales better for large clusters — the server does not need
  to know every node's address.
- The server stores the latest snapshot per node in a HashMap keyed by
  hostname. Prometheus scrapes one endpoint to get all nodes.
- No link-time dependency on libnvidia-ml; the NVIDIA collector shells out
  to `nvidia-smi` and parses CSV. The binary runs on any DGX with the
  driver installed.
- The `Collector` trait is the vendor abstraction. New GPU vendors implement
  it in a new module under `src/collector/`. No server or push changes are
  needed.
- The push agent and service mode use an async tokio runtime (reqwest) to
  keep system load low. Collection stays blocking and runs on a blocking
  task.
- Inference discovery: bollard (docker inspect) → process table → netstat →
  manual config. Re-run periodically. Capture all metrics with `engine` and
  `model_name` labels.
- Metadata (hostname, cluster, rack, model_name) is stored as labels on
  every metric for filtering in PromQL.

## Time-series storage

- When the server or service starts with `--data-dir <path>`, every snapshot
  is written to a tsink embedded time-series database at that path.
- The tsink database stores full history with a 30-day retention window.
- Without `--data-dir`, the server and service operate in memory-only mode
  and store only the latest snapshot per node for Prometheus scraping.
- The `DGMON_DATA_DIR` environment variable sets the same option.
