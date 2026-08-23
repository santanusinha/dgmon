# Batch Query API for dgmon

Status: IMPLEMENTED
Date: 2026-08-21
Author: Sai Dev (session c24a2351-2b90-47e0-8c41-9604e9050077)

## Goal

Let the ESP32 firmware fetch many fixed PromQL queries in one HTTP round
trip. Each query has an id. The response maps each id to its result. The
firmware multiplexes the results to the relevant screen or widget.

## Why

The dashboard has fixed queries per screen/widget. A naive approach is one
HTTP request per query. That is many round trips per poll cycle. A batch
endpoint returns all results in one request. This reduces latency and
connection churn.

## Endpoint

`POST /api/v1/query_batch`

### Request

JSON body:

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
- Optional `range` object for range queries (future sparklines):
  ```json
  { "id": "spark.cpu", "expr": "dgmon_cpu_usage_pct{hostname=\"node1\"}", "start": 1710000000, "end": 1710000300, "step": 15 }
  ```

### Response

```json
{
  "status": "success",
  "data": {
    "overview.cpu": {
      "resultType": "vector",
      "result": [
        { "metric": { "hostname": "node1" }, "value": [1710000000, "12.5"] },
        { "metric": { "hostname": "node2" }, "value": [1710000000, "8.1"] }
      ]
    },
    "overview.gpu_util": {
      "resultType": "vector",
      "result": [
        { "metric": { "hostname": "node1" }, "value": [1710000000, "85"] }
      ]
    },
    "detail.node1.gpu0.temp": {
      "resultType": "vector",
      "result": [
        { "metric": { "hostname": "node1", "gpu": "0" }, "value": [1710000000, "62"] }
      ]
    }
  }
}
```

- `data` is a map from query id to its Prometheus-style result.
- Each result keeps the standard Prometheus envelope (`resultType`, `result`).
- On error for one query, that id maps to an error object:
  ```json
  { "id": "overview.cpu", "error": "parse error: ..." }
  ```
  Other queries still succeed.

## Firmware multiplexing

The central fetch layer:

1. Builds the batch request from the active screen's widget query set.
2. Sends one `POST /api/v1/query_batch`.
3. Parses the response into a map: `id -> PromResult`.
4. Dispatches each result to the widget that registered that id.

Each widget registers its query id and a handler (or a slot in the shared
model). The fetch layer does not know what a widget means. It only routes
by id.

```cpp
struct QuerySpec { const char *id; const char *expr; };
struct QueryResult { String id; String resultType; JsonDocument data; };

class DataFetcher {
public:
  void add_query(const QuerySpec &q);
  bool poll();                       // one batch round trip
  const QueryResult *get(const char *id) const;
};
```

## Error handling

- HTTP non-200: whole batch fails. Firmware keeps last good data.
- Per-query error: that id has no result. Widget shows stale or blank.
- Missing id in response: treat as error for that widget.

## Resolved questions

- Range queries: supported. Each query may carry an optional `range` object
  with `start`, `end`, and `step` (unix seconds). When present, dgmon runs a
  range query; otherwise it runs an instant query.
- Shared `time`: not implemented. Each query carries its own optional
  `time`. A shared top-level time is future work.
- Duplicate ids: rejected. dgmon returns an error when two queries share the
  same `id`, so the response map stays unambiguous.

## Related

- `plan/dgx-cc-firmware-requirements.md` — firmware requirements.
- `plans/prometheus-api-and-clustering.md` — existing Prometheus API work in
  dgmon.
