# dgmon

A lightweight system monitor for NVIDIA DGX Spark GPU clusters.

## Quick start

```sh
curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/install.sh | sudo bash
```

The installer detects the local architecture, downloads the matching release
binary from GitHub, and sets up a systemd service. It asks for the mode:
`server` or `push`.

For a single node:

```sh
dgmon service --listen 0.0.0.0:9401
```

## Documentation

Full documentation is available at
[https://santanusinha.github.io/dgmon/](https://santanusinha.github.io/dgmon/).

- [Install](docs/install.md) — all install methods
- [Architecture](docs/architecture.md) — push architecture and modes
- [Usage](docs/usage.md) — commands, CLI options, config
- [API Reference](docs/api.md) — REST and Prometheus endpoints
- [Grafana](docs/grafana.md) — Grafana setup and dashboard
- [Metrics](docs/metrics.md) — every metric
- [Development](docs/development.md) — source layout and collector abstraction

## License

Apache-2.0
