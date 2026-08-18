// SPDX-License-Identifier: Apache-2.0
//! Mock collector for development and testing.
//!
//! Produces realistic GPU samples without a real GPU.

use std::collections::HashMap;

use super::{Collector, GpuSample, HostSample, Snapshot};

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
                utilization_gpu: 40 + i as u32 * 5,
                utilization_memory: 50,
                temperature_c: 55 + i as u32,
                power_w: Some(350.0 + i as f32 * 10.0),
                power_limit_w: Some(700.0),
                memory_used_mb: Some(40_000 + i as u64 * 5_000),
                memory_total_mb: Some(140_000),
                fan_speed_pct: Some(60),
                pstate: format!("P{i}"),
            })
            .collect();

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
            },
            gpus,
            extra: HashMap::new(),
        })
    }
}