# dgmon

A lightweight system monitor for NVIDIA DGX Spark GPU clusters.

## Quick start

### Single node

```sh
dgmon service --listen 0.0.0.0:9401
```

Open the dashboard at `http://<node-ip>:9401/`.

### Two nodes

On the server node:

```sh
dgmon server --listen 0.0.0.0:9401
```

On the other node, create a push config and run the push agent:

```sh
dgmon push --config /etc/dgmon/dgmon.json
```

### Larger cluster

One central node runs `dgmon server`; every other node runs `dgmon push`.

```sh
# On the central node:
dgmon server --listen 0.0.0.0:9401

# On each GPU node:
dgmon push --config /etc/dgmon/dgmon.json
```

### Installer script

To run any of these as a systemd service, use the installer script. It
detects the local architecture, downloads the matching release binary from
GitHub, and sets up the service. It asks for the mode: `server` or `push`.

```sh
curl -fsSL https://raw.githubusercontent.com/santanusinha/dgmon/master/deploy/install.sh | sudo bash
```

## Documentation

Full documentation is available at
[https://santanusinha.github.io/dgmon/](https://santanusinha.github.io/dgmon/).

- [Install](docs/install.md) — deployment strategies and install methods
- [Architecture](docs/architecture.md) — push architecture and modes
- [Usage](docs/usage.md) — commands, CLI options, config
- [API Reference](docs/api.md) — REST and Prometheus endpoints
- [Grafana](docs/grafana.md) — Grafana setup and dashboard
- [Metrics](docs/metrics.md) — every metric
- [Development](docs/development.md) — source layout and collector abstraction

## License

Apache-2.0
