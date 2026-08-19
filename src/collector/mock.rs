// SPDX-License-Identifier: Apache-2.0
//! Mock collector for development and testing.
//!
//! Produces realistic GPU samples without a real GPU.

use std::collections::HashMap;

use super::{Collector, CpuCoreSample, GpuSample, HostSample, NetSample, Snapshot};

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
            })
            .collect();

        // Per-CPU-core utilization (simulate a 64-core DGX Spark).
        let cpu_cores = (0..64)
            .map(|i| CpuCoreSample {
                index: i,
                usage_pct: 5.0 + (i as f32 % 20.0),
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
            inference: Vec::new(),
            extra: HashMap::new(),
        })
    }
}