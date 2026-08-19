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
    /// SM (compute) clock in MHz.
    pub sm_clock_mhz: Option<u32>,
    /// Memory clock in MHz.
    pub mem_clock_mhz: Option<u32>,
    /// Maximum SM clock in MHz.
    pub sm_clock_max_mhz: Option<u32>,
    /// Maximum memory clock in MHz.
    pub mem_clock_max_mhz: Option<u32>,
    /// Memory temperature in Celsius.
    pub mem_temp_c: Option<u32>,
    /// Current PCIe link generation.
    pub pcie_link_gen: Option<u32>,
    /// Maximum PCIe link generation.
    pub pcie_link_gen_max: Option<u32>,
    /// Current PCIe link width in lanes.
    pub pcie_link_width: Option<u32>,
    /// Maximum PCIe link width in lanes.
    pub pcie_link_width_max: Option<u32>,
}

/// Per-CPU-core utilization sample.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CpuCoreSample {
    pub index: u32,
    pub usage_pct: f32,
}

/// Per-interface network sample.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetSample {
    pub interface: String,
    /// Role auto-detected from the interface name: main, cluster, or other.
    pub role: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub speed_mbps: Option<u64>,
    pub up: bool,
}

/// Inference server sample (sglang or vLLM).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InferenceSample {
    /// Engine name: sglang or vllm.
    pub engine: String,
    /// Model name served by the engine.
    pub model_name: String,
    /// Scraped Prometheus metrics as (name, value) pairs.
    pub metrics: Vec<(String, f64)>,
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
    /// Per-CPU-core utilization.
    #[serde(default)]
    pub cpu_cores: Vec<CpuCoreSample>,
    /// Per-interface network utilization.
    #[serde(default)]
    pub networks: Vec<NetSample>,
}

/// The complete snapshot for one collection cycle.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub host: HostSample,
    pub gpus: Vec<GpuSample>,
    /// Inference metrics scraped from local sglang/vLLM servers.
    #[serde(default)]
    pub inference: Vec<InferenceSample>,
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