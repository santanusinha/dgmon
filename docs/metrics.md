---
icon: lucide/activity
---

# Metrics

All metric names are prefixed with `dgmon_`. Every metric carries the
`hostname` label plus any `extra` labels from the config (for example
`cluster`, `rack`, `node_role`). Inference metrics also carry `engine` and
`model_name`.

## Host metrics

| Metric | Type | Unit | Description |
|---|---|---|---|
| `dgmon_cpu_usage_pct` | gauge | percent | Overall CPU utilization. |
| `dgmon_memory_used_mb` | gauge | MB | Used system memory. |
| `dgmon_memory_total_mb` | gauge | MB | Total system memory. |
| `dgmon_disk_used_gb` | gauge | GB | Used disk space. |
| `dgmon_disk_total_gb` | gauge | GB | Total disk space. |
| `dgmon_network_rx_bytes` | counter | bytes | Total received bytes. |
| `dgmon_network_tx_bytes` | counter | bytes | Total transmitted bytes. |
| `dgmon_uptime_seconds` | gauge | seconds | System uptime. |

## CPU core metrics (per core)

| Metric | Type | Unit | Description |
|---|---|---|---|
| `dgmon_cpu_core_usage_pct` | gauge | percent | Core utilization. |
| `dgmon_cpu_core_freq_mhz` | gauge | MHz | Current core frequency. |
| `dgmon_cpu_core_max_freq_mhz` | gauge | MHz | Maximum core frequency. |
| `dgmon_cpu_core_governor` | gauge | string | CPU scaling governor. |

Label: `core` (core index).

## Network interface metrics (per interface)

| Metric | Type | Unit | Description |
|---|---|---|---|
| `dgmon_net_rx_bytes` | counter | bytes | Received bytes. |
| `dgmon_net_tx_bytes` | counter | bytes | Transmitted bytes. |
| `dgmon_net_rx_packets` | counter | packets | Received packets. |
| `dgmon_net_tx_packets` | counter | packets | Transmitted packets. |
| `dgmon_net_speed_mbps` | gauge | Mbps | Link speed. |
| `dgmon_net_up` | gauge | 0/1 | Link up state. |

Labels: `interface`, `role` (`main`, `cluster`, `other`).

## GPU metrics (per device)

| Metric | Type | Unit | Description |
|---|---|---|---|
| `dgmon_gpu_utilization` | gauge | percent | GPU compute utilization. |
| `dgmon_gpu_mem_utilization` | gauge | percent | GPU memory utilization. |
| `dgmon_gpu_temp_c` | gauge | Celsius | GPU temperature. |
| `dgmon_gpu_mem_temp_c` | gauge | Celsius | GPU memory temperature. |
| `dgmon_gpu_power_w` | gauge | watts | Power draw. |
| `dgmon_gpu_power_limit_w` | gauge | watts | Power limit. |
| `dgmon_gpu_memory_used_mb` | gauge | MB | Used GPU memory. |
| `dgmon_gpu_memory_total_mb` | gauge | MB | Total GPU memory. |
| `dgmon_gpu_fan_speed_pct` | gauge | percent | Fan speed. |
| `dgmon_gpu_sm_clock_mhz` | gauge | MHz | SM (compute) clock. |
| `dgmon_gpu_sm_clock_max_mhz` | gauge | MHz | Maximum SM clock. |
| `dgmon_gpu_mem_clock_mhz` | gauge | MHz | Memory clock. |
| `dgmon_gpu_mem_clock_max_mhz` | gauge | MHz | Maximum memory clock. |
| `dgmon_gpu_pcie_link_gen` | gauge | generation | Current PCIe link generation. |
| `dgmon_gpu_pcie_link_gen_max` | gauge | generation | Maximum PCIe link generation. |
| `dgmon_gpu_pcie_link_width` | gauge | lanes | Current PCIe link width. |
| `dgmon_gpu_pcie_link_width_max` | gauge | lanes | Maximum PCIe link width. |

Labels: `gpu` (index), `uuid`, `model`.

## GPU throttle metrics (per device)

| Metric | Type | Unit | Description |
|---|---|---|---|
| `dgmon_gpu_throttle_active` | gauge | bitmask | Active throttle reasons bitmask. |
| `dgmon_gpu_throttle_hw_thermal` | gauge | 0/1 | HW thermal slowdown active. |
| `dgmon_gpu_throttle_sw_thermal` | gauge | 0/1 | SW thermal slowdown active. |
| `dgmon_gpu_throttle_hw_slowdown` | gauge | 0/1 | HW slowdown active. |
| `dgmon_gpu_throttle_power_brake` | gauge | 0/1 | HW power brake slowdown active. |

Labels: `gpu` (index), `uuid`, `model`.

## Inference metrics (per engine)

The push agent (and service mode) discovers local inference servers and
scrapes their `/metrics` endpoint. Discovery order:

1. Manual config targets (`inference_servers` in the config file).
2. Docker containers (bollard) running sglang or vLLM images.
3. Process table (sysinfo) for sglang/vLLM processes.
4. `netstat` output as a last resort.

Each discovered server is scraped for `/metrics` and the model name is
fetched from the `/v1/models` API. All metrics are captured and stored with
`engine` and `model_name` labels. If no inference server is found, the
snapshot contains no inference metrics and does not fail.

Inference metric names are prefixed with `dgmon_inference_`. The raw engine
prefix (`vllm:` or `sglang:`) is stripped and the rest is sanitized to a
valid Prometheus name. The set of inference metrics is dynamic and depends
on the engine version. Common ones include:

| Metric | Type | Description |
|---|---|---|
| `dgmon_inference_num_requests_running` | gauge | Requests currently running. |
| `dgmon_inference_num_requests_waiting` | gauge | Requests waiting in queue. |
| `dgmon_inference_kv_cache_usage_perc` | gauge | Fraction of the KV block pool in use (0-1). |
| `dgmon_inference_generation_tokens_total` | counter | Generated output tokens. |
| `dgmon_inference_prompt_tokens_total` | counter | Input prompt tokens. |
| `dgmon_inference_prompt_tokens_cached_total` | counter | Cached prompt tokens. |
| `dgmon_inference_time_to_first_token_seconds` | histogram | Time to first token. |
| `dgmon_inference_inter_token_latency_seconds` | histogram | Time between output tokens. |
| `dgmon_inference_e2e_request_latency_seconds` | histogram | End-to-end request latency. |
| `dgmon_inference_request_success_total` | counter | Successful requests. |
| `dgmon_inference_num_preemptions_total` | counter | Preempted requests. |
| `dgmon_inference_prefix_cache_hits_total` | counter | Prefix cache hits. |
| `dgmon_inference_prefix_cache_queries_total` | counter | Prefix cache queries. |
| `dgmon_inference_process_cpu_seconds_total` | counter | Engine process CPU time. |
| `dgmon_inference_process_resident_memory_bytes` | gauge | Engine process resident memory. |
| `dgmon_inference_process_virtual_memory_bytes` | gauge | Engine process virtual memory. |

Labels: `engine` (`vllm`, `sglang`), `model_name`.

## Metadata labels

Every metric carries the `hostname` label plus any `extra` labels from the
config (for example `cluster`, `rack`, `node_role`). Inference metrics also
carry `engine` and `model_name`. This lets you filter in PromQL:

```promql
# All GPU utilization for the production cluster
dgmon_gpu_utilization{cluster="dgx-spark-prod"}

# Generation tokens for a specific model
rate(dgmon_inference_generation_tokens_total{model_name="llama-3-8b"}[5m])
```
