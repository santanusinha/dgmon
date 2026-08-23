# Plan: Cluster-level dashboard with inference metrics

Status: IN PROGRESS
Date: 2026-08-23
Author: Sai Web Developer (session de062449-45ab-4a7f-85f9-c11bb80c6427)

## Goal

Make the main dashboard view show **cluster-level** data and important
**inferencing** metrics (TTFT, tok/sec total, input/output tokens, total
GPU memory used). The current host-level view moves into a **tab**.

## Background

The current dashboard (dashboard/index.html + app.js) shows one node at a
time. The user selects a node from the top bar. All charts and tables are
scoped to that node. This is useful for host-level debugging but not for
cluster-wide operations.

The user wants:
1. A **cluster view** as the default/main view showing:
   - Total GPU memory used across all nodes
   - Total GPU utilization
   - Inference metrics: TTFT, tok/sec, input/output tokens
   - Per-node breakdown
2. The **host view** (current) becomes a tab.
3. Tabs to switch between views.

## Data model

### Cluster-level metrics (PromQL)
All metrics carry a `hostname` label. Cluster aggregation uses `sum()`:

- **Total GPU memory used**: `sum(dgmon_gpu_memory_used_mb) / 1024` → GB
- **Total GPU memory capacity**: `sum(dgmon_gpu_memory_total_mb) / 1024` → GB
- **Total GPU utilization**: `avg(dgmon_gpu_utilization)` → %
- **Total GPU count**: `count(dgmon_gpu_utilization)` → GPUs
- **Total nodes**: `count(dgmon_cpu_usage_pct)` → nodes

### Inference metrics (PromQL)
Inference metrics carry `engine` and `model_name` labels. Cluster aggregation:

- **Tokens/sec (output)**: `sum(rate(dgmon_inference_generation_tokens_total[1m]))`
- **Tokens/sec (input)**: `sum(rate(dgmon_inference_prompt_tokens_total[1m]))`
- **Requests running**: `sum(dgmon_inference_num_requests_running)`
- **Requests waiting**: `sum(dgmon_inference_num_requests_waiting)`
- **KV cache usage**: `avg(dgmon_inference_kv_cache_usage_perc)`
- **TTFT (p50)**: `histogram_quantile(0.5, sum by (le) (rate(dgmon_inference_time_to_first_token_seconds_bucket[5m])))`
- **TTFT (p95)**: `histogram_quantile(0.95, sum by (le) (rate(dgmon_inference_time_to_first_token_seconds_bucket[5m])))`
- **Inter-token latency (p50)**: `histogram_quantile(0.5, sum by (le) (rate(dgmon_inference_inter_token_latency_seconds_bucket[5m])))`
- **Throughput (tokens/sec)**: `sum(rate(dgmon_inference_generation_tokens_total[1m]))`

Note: tsink stores raw metrics. Histogram buckets are stored as separate
metric names ending in `_bucket`. The `histogram_quantile` function is
supported by tsink.

### Per-node breakdown (cluster view)
Show a table with one row per node:
- Node hostname
- GPU count
- Total GPU util (avg)
- Total GPU mem used (GB)
- Total GPU mem capacity (GB)
- Tokens/sec (output)
- Tokens/sec (input)
- Requests running
- TTFT (p50)

## Implementation

### 1. index.html — add tabs
Add a tab bar between the header and main content:
- **Cluster** tab (default)
- **Host** tab

Wrap the existing host sections in a `<div id="host-view">`.
Add a new `<div id="cluster-view">` with cluster sections.

### 2. app.js — add tab logic + cluster queries
- Add tab switching logic (show/hide views)
- Add cluster queries to the batch request in refreshAll()
- Add cluster render functions
- Keep existing host render functions

### 3. style.css — tab styling
Add styles for the tab bar.

### 4. Rebuild binary
Rebuild release binary to embed updated dashboard.

### 5. Verify in browser
Start mock server, verify both views render correctly.

## Files to change
- dashboard/index.html — add tabs, cluster view sections
- dashboard/app.js — add tab logic, cluster queries and render functions
- dashboard/style.css — add tab styles

## Mock data consideration
The mock collector currently produces `inference: Vec::new()`. To test the
cluster view, I need to add mock inference metrics. I will update
`src/collector/mock.rs` to produce realistic inference samples.

## Risks
- tsink PromQL engine may not support all functions needed (e.g. `by (le)`).
  Need to verify `histogram_quantile` works with `sum by (le)` syntax.
- Inference metrics are dynamic; the dashboard must handle missing data
  gracefully (show "—" when no data).
- Histogram metrics from vLLM/sglang have `_bucket` suffix and `le` label.
  Need to verify the exact naming from the README.

## Verification
1. `node --check dashboard/app.js` — syntax check
2. `cargo build --release` — rebuild binary
3. Start mock server with `--data-dir`
4. Load dashboard in browser
5. Verify cluster view shows aggregated data
6. Verify host view still works
7. Check console for errors
8. Take screenshots
