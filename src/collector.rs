// SPDX-License-Identifier: Apache-2.0
//! GPU collector abstraction.
//!
//! Every GPU vendor supplies metrics through the same trait.
//! Today the only implementation reads from NVIDIA `nvidia-smi`.
//! Future implementations can read from `rocm-smi` (AMD) or
//! `intel-smi` (Intel) without changes to the service layer.

use std::collections::HashMap;

/// A single GPU sample.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GpuSample {
    pub index: u32,
    pub uuid: String,
    pub name: String,
    pub utilization_gpu: u32,
    pub utilization_memory: u32,
    pub temperature_c: u32,
    pub power_w: Option<f32>,
    pub power_limit_w: Option<f32>,
    pub memory_used_mb: Option<u64>,
    pub memory_total_mb: Option<u64>,
    pub fan_speed_pct: Option<u32>,
    pub pstate: String,
}

/// System-level metrics that surround the GPUs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HostSample {
    pub hostname: String,
    pub cpu_usage_pct: f32,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub disk_used_gb: f64,
    pub disk_total_gb: f64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub uptime_seconds: u64,
}

/// The complete snapshot for one collection cycle.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub host: HostSample,
    pub gpus: Vec<GpuSample>,
    /// Extra labels attached by the collector config (cluster, rack, etc.).
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

/// Every GPU vendor plug-in implements this trait.
pub trait Collector: Send + Sync {
    fn name(&self) -> &str;

    fn collect(&self) -> anyhow::Result<Snapshot>;
}

mod nvidia;
mod mock;

pub use mock::MockCollector;
pub use nvidia::NvidiaSmiCollector;