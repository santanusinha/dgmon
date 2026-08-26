---
icon: lucide/bar-chart
---

# Grafana

dgmon ships a ready-made Grafana dashboard in `extras/`. It has 12 panels
for GPU, host, network, and inference metrics, plus template variables for
`hostname` and `gpu`. You can import it into any Grafana instance.

## Run Grafana with Docker

```sh
docker run -d --name dgmon-grafana -p 3000:3000 \
  -e GF_SECURITY_ADMIN_USER=admin \
  -e GF_SECURITY_ADMIN_PASSWORD=admin \
  -e GF_USERS_ALLOW_SIGN_UP=false \
  grafana/grafana:latest
```

## Add the dgmon datasource

The datasource points at the dgmon server (or service) Prometheus API.
Use the server host and port. Do not append `/api/v1` to the URL.

```sh
curl -u admin:admin -X POST http://localhost:3000/api/datasources \
  -H "Content-Type: application/json" -d '{
    "name": "dgmon",
    "type": "prometheus",
    "url": "http://<server-host>:9401",
    "access": "proxy",
    "isDefault": true,
    "jsonData": {
      "httpMethod": "GET",
      "timeInterval": "15s"
    }
  }'
```

Notes:

- `httpMethod` must be `GET`. dgmon implements only `GET /api/v1/query`.
- The server must have a data directory (via `data_dir` in the config or
  `--data-dir`) so the Prometheus API has data.
- The datasource health check uses `/api/v1/status/buildinfo`.

## Import the dashboard

The dashboard JSON is the Grafana import payload. Import it with:

```sh
curl -u admin:admin -X POST http://localhost:3000/api/dashboards/db \
  -H "Content-Type: application/json" \
  -d @extras/dgmon-cluster-dashboard.json
```

Or import it from the Grafana UI: **Dashboards → New → Import**, then
upload `extras/dgmon-cluster-dashboard.json`.

The dashboard references the datasource by uid. If your datasource uid
differs, update the `uid` fields in the JSON before importing, or set the
datasource as default so Grafana resolves it.
