// SPDX-License-Identifier: Apache-2.0
//! Mock collector for development and testing.
//!
//! Produces realistic GPU samples without a real GPU.

use std::collections::HashMap;

use super::{Collector, CpuCoreSample, GpuSample, HostSample, InferenceSample, NetSample, Snapshot};

pub struct MockCollector;

impl Collector for MockCollector {
    fn name(&self) -> &str {
        "mock"
    }

    fn collect(&self) -> anyhow::Result<Snapshot> {
        let gpus = (0..8)
            .map(|i| GpuSample {
                index: i,
                uuid: format!("GPU-{i}-mock"),
                name: "NVIDIA DGX H200 Mock".into(),
                utilization_gpu: 40 + i * 5,
                utilization_memory: 50,
                temperature_c: 55 + i,
                power_w: Some(350.0 + i as f32 * 10.0),
                power_limit_w: Some(700.0),
                memory_used_mb: Some(40_000 + i as u64 * 5_000),
                memory_total_mb: Some(140_000),
                fan_speed_pct: Some(60),
                pstate: format!("P{i}"),
                sm_clock_mhz: Some(1_800 + i * 10),
                mem_clock_mhz: Some(2_600),
                sm_clock_max_mhz: Some(1_980),
                mem_clock_max_mhz: Some(2_600),
                mem_temp_c: Some(60 + i),
                pcie_link_gen: Some(5),
                pcie_link_gen_max: Some(5),
                pcie_link_width: Some(16),
                pcie_link_width_max: Some(16),
                throttle_active: 0,
                throttle_hw_thermal: false,
                throttle_sw_thermal: false,
                throttle_hw_slowdown: false,
                throttle_power_brake: false,
            })
            .collect();

        // Per-CPU-core utilization (simulate a 64-core DGX Spark).
        let cpu_cores = (0..64)
            .map(|i| CpuCoreSample {
                index: i,
                usage_pct: 5.0 + (i as f32 % 20.0),
                freq_mhz: Some(2800),
                governor: Some("performance".into()),
                max_freq_mhz: Some(3900),
            })
            .collect();

        // Per-interface network utilization.
        let networks = vec![
            NetSample {
                interface: "eth0".into(),
                role: "main".into(),
                rx_bytes: 1_000_000,
                tx_bytes: 2_000_000,
                rx_packets: 10_000,
                tx_packets: 20_000,
                speed_mbps: Some(1000),
                up: true,
            },
            NetSample {
                interface: "enp1s0f0np0".into(),
                role: "cluster".into(),
                rx_bytes: 50_000_000,
                tx_bytes: 60_000_000,
                rx_packets: 500_000,
                tx_packets: 600_000,
                speed_mbps: Some(200_000),
                up: true,
            },
            NetSample {
                interface: "enp1s0f1np1".into(),
                role: "cluster".into(),
                rx_bytes: 55_000_000,
                tx_bytes: 65_000_000,
                rx_packets: 550_000,
                tx_packets: 650_000,
                speed_mbps: Some(200_000),
                up: true,
            },
        ];

        // Mock inference metrics (simulate sglang/vLLM server).
        let inference = vec![
            InferenceSample {
                engine: "vllm".into(),
                model_name: "llama-3.1-8b-instruct".into(),
                metrics: vec![
                    ("vllm:num_requests_running".into(), 3.0),
                    ("vllm:num_requests_waiting".into(), 2.0),
                    ("vllm:kv_cache_usage_perc".into(), 0.45),
                    ("vllm:generation_tokens_total".into(), 1_250_000.0),
                    ("vllm:prompt_tokens_total".into(), 860_000.0),
                    ("vllm:prompt_tokens_cached_total".into(), 320_000.0),
                    ("vllm:time_to_first_token_seconds_sum".into(), 450.0),
                    ("vllm:time_to_first_token_seconds_count".into(), 500.0),
                    ("vllm:time_to_first_token_seconds_bucket{le=\"0.01\"}".into(), 0.0),
                    ("vllm:time_to_first_token_seconds_bucket{le=\"0.05\"}".into(), 10.0),
                    ("vllm:time_to_first_token_seconds_bucket{le=\"0.1\"}".into(), 80.0),
                    ("vllm:time_to_first_token_seconds_bucket{le=\"0.25\"}".into(), 220.0),
                    ("vllm:time_to_first_token_seconds_bucket{le=\"0.5\"}".into(), 380.0),
                    ("vllm:time_to_first_token_seconds_bucket{le=\"1.0\"}".into(), 470.0),
                    ("vllm:time_to_first_token_seconds_bucket{le=\"+Inf\"}".into(), 500.0),
                    ("vllm:inter_token_latency_seconds_sum".into(), 1200.0),
                    ("vllm:inter_token_latency_seconds_count".into(), 8000.0),
                    ("vllm:inter_token_latency_seconds_bucket{le=\"0.01\"}".into(), 100.0),
                    ("vllm:inter_token_latency_seconds_bucket{le=\"0.05\"}".into(), 1500.0),
                    ("vllm:inter_token_latency_seconds_bucket{le=\"0.1\"}".into(), 4000.0),
                    ("vllm:inter_token_latency_seconds_bucket{le=\"0.25\"}".into(), 6500.0),
                    ("vllm:inter_token_latency_seconds_bucket{le=\"0.5\"}".into(), 7600.0),
                    ("vllm:inter_token_latency_seconds_bucket{le=\"1.0\"}".into(), 7950.0),
                    ("vllm:inter_token_latency_seconds_bucket{le=\"+Inf\"}".into(), 8000.0),
                    ("vllm:request_success_total".into(), 500.0),
                    ("vllm:num_preemptions_total".into(), 5.0),
                    ("vllm:prefix_cache_hits_total".into(), 320_000.0),
                    ("vllm:prefix_cache_queries_total".into(), 860_000.0),
                    ("vllm:process_cpu_seconds_total".into(), 7200.0),
                    ("vllm:process_resident_memory_bytes".into(), 14_000_000_000.0),
                    ("vllm:process_virtual_memory_bytes".into(), 20_000_000_000.0),
                ],
            },
            InferenceSample {
                engine: "sglang".into(),
                model_name: "qwen2.5-32b".into(),
                metrics: vec![
                    ("sglang:num_requests_running".into(), 2.0),
                    ("sglang:num_requests_waiting".into(), 1.0),
                    ("sglang:kv_cache_usage_perc".into(), 0.62),
                    ("sglang:generation_tokens_total".into(), 980_000.0),
                    ("sglang:prompt_tokens_total".into(), 640_000.0),
                    ("sglang:prompt_tokens_cached_total".into(), 210_000.0),
                    ("sglang:time_to_first_token_seconds_sum".into(), 320.0),
                    ("sglang:time_to_first_token_seconds_count".into(), 400.0),
                    ("sglang:time_to_first_token_seconds_bucket{le=\"0.01\"}".into(), 0.0),
                    ("sglang:time_to_first_token_seconds_bucket{le=\"0.05\"}".into(), 5.0),
                    ("sglang:time_to_first_token_seconds_bucket{le=\"0.1\"}".into(), 60.0),
                    ("sglang:time_to_first_token_seconds_bucket{le=\"0.25\"}".into(), 180.0),
                    ("sglang:time_to_first_token_seconds_bucket{le=\"0.5\"}".into(), 300.0),
                    ("sglang:time_to_first_token_seconds_bucket{le=\"1.0\"}".into(), 380.0),
                    ("sglang:time_to_first_token_seconds_bucket{le=\"+Inf\"}".into(), 400.0),
                    ("sglang:inter_token_latency_seconds_sum".into(), 900.0),
                    ("sglang:inter_token_latency_seconds_count".into(), 6000.0),
                    ("sglang:inter_token_latency_seconds_bucket{le=\"0.01\"}".into(), 50.0),
                    ("sglang:inter_token_latency_seconds_bucket{le=\"0.05\"}".into(), 800.0),
                    ("sglang:inter_token_latency_seconds_bucket{le=\"0.1\"}".into(), 2500.0),
                    ("sglang:inter_token_latency_seconds_bucket{le=\"0.25\"}".into(), 4800.0),
                    ("sglang:inter_token_latency_seconds_bucket{le=\"0.5\"}".into(), 5700.0),
                    ("sglang:inter_token_latency_seconds_bucket{le=\"1.0\"}".into(), 5950.0),
                    ("sglang:inter_token_latency_seconds_bucket{le=\"+Inf\"}".into(), 6000.0),
                    ("sglang:request_success_total".into(), 400.0),
                    ("sglang:num_preemptions_total".into(), 3.0),
                    ("sglang:prefix_cache_hits_total".into(), 210_000.0),
                    ("sglang:prefix_cache_queries_total".into(), 640_000.0),
                    ("sglang:process_cpu_seconds_total".into(), 5400.0),
                    ("sglang:process_resident_memory_bytes".into(), 28_000_000_000.0),
                    ("sglang:process_virtual_memory_bytes".into(), 35_000_000_000.0),
                ],
            },
        ];

        Ok(Snapshot {
            timestamp: chrono::Utc::now(),
            host: HostSample {
                hostname: "mock-dgx-spark".into(),
                cpu_usage_pct: 12.5,
                memory_used_mb: 64_000,
                memory_total_mb: 512_000,
                disk_used_gb: 1_200.0,
                disk_total_gb: 15_000.0,
                network_rx_bytes: 1_000_000,
                network_tx_bytes: 2_000_000,
                uptime_seconds: 86400,
                cpu_cores,
                networks,
            },
            gpus,
            inference,
            extra: HashMap::new(),
        })
    }
}