# Plan: Prometheus-compatible API + Grafana console for dgmon

Status: COMPLETE
Date: 2026-08-19
Author: Sai Dev (session 213e80e4-459a-4e89-82b4-e33512b5816d)

## Goal

Make dgmon usable as a Prometheus datasource in Grafana, and add a Grafana
console on Docker to visualize the cluster.

## Part 1 — Prometheus-compatible HTTP API (DONE)

### Why not a proxy?
dgmon already evaluates PromQL via tsink and returns data in `/query`. The
only difference is the JSON envelope. A separate Python proxy adds a process,
a port, and a translation layer for no reason. We add the Prometheus HTTP API
endpoints directly in dgmon's actix-web app.

### What Grafana needs
Grafana's Prometheus datasource calls:
- `GET /api/v1/query?query=<expr>&time=<unix_seconds>` → instant query
- `GET /api/v1/query_range?query=<expr>&start=<s>&end=<s>&step=<s>` → range query
- `GET /api/v1/labels` and `GET /api/v1/label/<name>/values` → label discovery
- `GET /api/v1/metadata` → metric metadata (optional)
- `GET /api/v1/status/buildinfo` → version info (Grafana checks this)

Response envelope (Prometheus):
```json
{
  "status": "success",
  "data": {
    "resultType": "vector" | "matrix" | "scalar" | "string",
    "result": [ ... ]
  }
}
```
- vector result item: `{"metric": {"__name__": "...", "label": "val"}, "value": [ts_sec, "val"]}`
- matrix result item: `{"metric": {...}, "values": [[ts_sec, "val"], ...]}`

### Approach
Add a new module `src/promapi.rs` with handlers that:
1. Parse Prometheus query params (time in SECONDS, not ms).
2. Call the same tsink `promql_instant` / `promql_range`.
3. Convert `PromqlValue` to the Prometheus envelope.
4. Add `/api/v1/query`, `/api/v1/query_range`, `/api/v1/labels`,
   `/api/v1/label/{name}/values`, `/api/v1/status/buildinfo`.
5. Register routes in server.rs and service.rs.

### Current state (what is already done)
- `src/promapi.rs` written (344 lines) with handlers:
  - `query` — instant query
  - `query_range` — range query
  - `labels` — label discovery
  - `label_values` — label values
  - `buildinfo` — version info
- Routes registered in `server.rs` and `service.rs` via `promapi::configure`.
- `cargo check` passes, `cargo build --release` passes, aarch64 cross-compile passes.
- Binary contains promapi strings (confirmed `query_range`, `buildinfo`,
  `label/{name}` present).

### KNOWN ISSUE — routes return 404
When tested against a local mock server, `/api/v1/query` and
`/api/v1/status/buildinfo` return **404 Not Found**, while the existing
`/api/v1/metrics` and `/api/v1/nodes` (from `api::configure`) work fine.

### RESOLVED — 404 root cause
actix-web matches the FIRST scope registered for a prefix and never falls
through to a second scope with the same prefix. Both `api::configure` and
`promapi::configure` registered scopes at `/api/v1`; the api scope won, so
`/api/v1/query` etc. fell to 404.

Fix: `promapi::configure` now registers routes directly (no scope), and
`api::configure` calls `promapi::configure` inside its `/api/v1` scope.
Removed redundant `promapi::configure` calls from server.rs and service.rs.

Verified working with real data:
- /api/v1/status/buildinfo -> 200
- /api/v1/query -> 200 with vector results
- /api/v1/query_range -> 200 with matrix results
- /api/v1/labels -> 200 with label names
- /api/v1/label/{name}/values -> 200 with values

### Steps to finish Part 1
1. Cross-compile aarch64, deploy to 120, restart server.
2. Point Grafana datasource at `http://192.168.3.120:9401` (Prometheus type).
3. Build dashboard JSON.

### Open questions
- Keep existing custom `/query` for backward compat? (Yes — keep it.)
- Route naming: use `/api/v1/query` (Prometheus style) — no conflict with
  existing `/api/v1/metrics`. Confirmed `/api/v1/query` is not currently used.
- Time units: Prometheus uses seconds. tsink uses ms. Convert.
- Do we need `/api/v1/metadata`? Grafana can work without it. Add buildinfo
  (cheap) and labels (useful for variable dropdowns).

### Risks
- Prometheus API uses seconds; tsink uses ms. Conversion must be correct.
- Grafana may probe `/api/v1/status/buildinfo` — return a minimal valid body.
- Label endpoints need tsink support. Check what tsink exposes for listing
  label names/values. If not available, return empty or derive from metrics.

---

## Part 2 — Grafana console (REVISED; clustering DROPPED)

### Decision (2026-08-20)
The user decided federation/clustering is NOT needed. dgx spark clusters are
small and used for experimentation. Production setups use other monitoring.
The architecture stays: ONE dgmon server, all cluster nodes push to it, and a
Grafana console visualizes the data. The PromQL API (Part 1) is what Grafana
needs as a datasource.

### Goals
- Run a Grafana console on Docker.
- Configure a Prometheus datasource pointing at the dgmon server.
- Build a dashboard to monitor the cluster.

### Steps
1. Cross-compile aarch64, deploy to 120, restart server (Part 1 finish).
2. Spin up Grafana on Docker (grafana/grafana image, port 3000).
3. Configure Prometheus datasource -> http://192.168.3.120:9401.
4. Build a dashboard with key dgmon metrics.

### Key dgmon metrics for the dashboard
- gpu_utilization, gpu_temp_c, gpu_power_w, gpu_sm_clock_mhz
- cpu_usage_pct, cpu_core_usage_pct, memory_used_mb
- net_rx/tx_bytes
- inference_kv_cache_usage_perc, inference_num_requests_running,
  inference_generation_tokens_total

### Open questions
- Where does Grafana run? (User: on this machine here.)
- Datasource URL: localhost vs LAN IP.

---

## Deferred items (from earlier sessions)
- Filter virtual interfaces (docker0, br-*) from net metrics.
- Decide on systemd deployment (deploy/systemd/*.service) vs manual nohup.
- Clean up test data/processes on 120/121.
- Post-test cleanup: stop test push agents + server, remove /tmp/dgmon-data.

## Live environment reference
- ai1 = 192.168.3.120 (node-rank 0, has vLLM /metrics on port 8000)
- ai2 = 192.168.3.121 (node-rank 1, headless, no /metrics)
- Cluster net 192.168.100.x (enp1s0f1np1), 192.168.101.x (enP2p1s0f1np1)
- Main net 192.168.3.x (enP7s7)
- dgmon server: http://192.168.3.120:9401
- 156+ metrics available; key ones: gpu_utilization, gpu_temp_c, gpu_power_w,
  gpu_sm_clock_mhz, cpu_usage_pct, cpu_core_usage_pct, memory_used_mb,
  net_rx/tx_bytes, inference_kv_cache_usage_perc,
  inference_num_requests_running, inference_generation_tokens_total.
