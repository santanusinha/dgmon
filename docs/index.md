---
icon: lucide/rocket
---

# dgmon

A lightweight system monitor for NVIDIA DGX Spark GPU clusters.

dgmon uses a **push** architecture. Each GPU node runs a collector agent
that pushes snapshots to a central server. Prometheus scrapes one endpoint
to get all nodes.

## Quick start

Install dgmon with the installer script. It detects the local architecture,
downloads the matching release binary from GitHub, and sets up a systemd
service.

```sh
curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/install.sh | sudo bash
```

The installer asks for the mode:

- `service` — a collector agent as well as aggregator server on a GPU node
- `push` — a collector agent on a GPU node
- `server` — the central aggregation server

For a single node, run the standalone service:

```sh
dgmon service --listen 0.0.0.0:9401 --config /etc/dgmon/dgmon.json
```

See [Install](install.md) for all install methods and
[Usage](usage.md) for the full command reference.

## Features

- Push-based cluster monitoring
- Built-in HTML dashboard
- Prometheus-compatible API for Grafana
- Time-series storage with 30-day retention
- Inference metrics for vLLM and sglang
- Collector abstraction for multiple GPU vendors

## Next steps

- [Install](install.md) dgmon
- [Architecture](architecture.md) explains the design
- [API Reference](api.md) documents the REST and Prometheus endpoints
- [Metrics](metrics.md) lists every metric
- [Grafana](grafana.md) sets up visualization
- [Development](development.md) covers the source layout
