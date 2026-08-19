// SPDX-License-Identifier: Apache-2.0
//! NVIDIA collector.
//!
//! Reads GPU telemetry from `nvidia-smi --query-gpu ... --format=csv`.
//! This avoids a link-time dependency on `libnvidia-ml`; the binary
//! runs on any DGX system that has the NVIDIA driver installed.
//!
//! Some fields may return `[N/A]` on certain hardware (e.g. DGX Spark
//! GB10 reports no memory or power-limit values). Those fields become
//! `None` in the sample.

use std::collections::HashMap;
use std::process::Command;

use super::{Collector, CpuCoreSample, GpuSample, HostSample, NetSample, Snapshot};

pub struct NvidiaSmiCollector {
    hostname: String,
    sys: std::sync::Mutex<sysinfo::System>,
    nets: std::sync::Mutex<sysinfo::Networks>,
}

impl NvidiaSmiCollector {
    pub fn new() -> Self {
        let sys = sysinfo::System::new_all();
        let nets = sysinfo::Networks::new_with_refreshed_list();
        let hostname = sysinfo::System::host_name().unwrap_or_else(|| "unknown".into());
        Self {
            hostname,
            sys: std::sync::Mutex::new(sys),
            nets: std::sync::Mutex::new(nets),
        }
    }
}

impl Default for NvidiaSmiCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// CSV columns we request from nvidia-smi, in order.
const QUERY: &str = "index,uuid,name,utilization.gpu,utilization.memory,temperature.gpu,power.draw,power.limit,memory.used,memory.total,fan.speed,pstate,clocks.sm,clocks.mem,clocks.max.sm,clocks.max.mem,temperature.memory,pcie.link.gen.current,pcie.link.gen.max,pcie.link.width.current,pcie.link.width.max";

fn parse_u32(s: &str) -> u32 {
    s.trim().parse::<u32>().unwrap_or(0)
}

fn parse_opt_u32(s: &str) -> Option<u32> {
    let s = s.trim();
    if s == "[N/A]" || s.is_empty() {
        return None;
    }
    s.parse::<u32>().ok()
}

fn parse_opt_f32(s: &str) -> Option<f32> {
    let s = s.trim();
    if s == "[N/A]" || s.is_empty() {
        return None;
    }
    s.parse::<f32>().ok()
}

fn parse_opt_u64(s: &str) -> Option<u64> {
    let s = s.trim();
    if s == "[N/A]" || s.is_empty() {
        return None;
    }
    s.parse::<u64>().ok()
}

impl Collector for NvidiaSmiCollector {
    fn name(&self) -> &str {
        "nvidia-smi"
    }

    fn collect(&self) -> anyhow::Result<Snapshot> {
        let raw = Command::new("nvidia-smi")
            .args(["--query-gpu", QUERY, "--format=csv,nounits,noheader"])
            .output()?;

        if !raw.status.success() {
            anyhow::bail!(
                "nvidia-smi failed: {}",
                String::from_utf8_lossy(&raw.stderr)
            );
        }

        let stdout = String::from_utf8_lossy(&raw.stdout);
        let mut gpus = Vec::new();

        for line in stdout.lines() {
            let cols: Vec<&str> = line.split(", ").collect();
            if cols.len() < 20 {
                continue;
            }

            gpus.push(GpuSample {
                index: parse_u32(cols[0]),
                uuid: cols[1].trim().to_string(),
                name: cols[2].trim().to_string(),
                utilization_gpu: parse_u32(cols[3]),
                utilization_memory: parse_u32(cols[4]),
                temperature_c: parse_u32(cols[5]),
                power_w: parse_opt_f32(cols[6]),
                power_limit_w: parse_opt_f32(cols[7]),
                memory_used_mb: parse_opt_u64(cols[8]),
                memory_total_mb: parse_opt_u64(cols[9]),
                fan_speed_pct: parse_opt_u32(cols[10]),
                pstate: cols[11].trim().to_string(),
                sm_clock_mhz: parse_opt_u32(cols[12]),
                mem_clock_mhz: parse_opt_u32(cols[13]),
                sm_clock_max_mhz: parse_opt_u32(cols[14]),
                mem_clock_max_mhz: parse_opt_u32(cols[15]),
                mem_temp_c: parse_opt_u32(cols[16]),
                pcie_link_gen: parse_opt_u32(cols[17]),
                pcie_link_gen_max: parse_opt_u32(cols[18]),
                pcie_link_width: parse_opt_u32(cols[19]),
                pcie_link_width_max: parse_opt_u32(cols[20]),
            });
        }

        {
            let mut sys = self.sys.lock().unwrap();
            sys.refresh_all();
        }
        let mut nets = self.nets.lock().unwrap();
        nets.refresh();
        let host = self.build_host(&nets);

        Ok(Snapshot {
            timestamp: chrono::Utc::now(),
            host,
            gpus,
            inference: Vec::new(),
            extra: HashMap::new(),
        })
    }
}

impl NvidiaSmiCollector {
    fn build_host(&self, nets: &sysinfo::Networks) -> HostSample {
        use sysinfo::Disks;

        let sys = self.sys.lock().unwrap();
        let cpu_usage = sys.global_cpu_usage();
        let mem_used = sys.used_memory() / 1024; // bytes → KiB
        let mem_total = sys.total_memory() / 1024;

        let (disk_used, disk_total) = {
            let disks = Disks::new_with_refreshed_list();
            let total: u64 = disks.iter().map(|d| d.total_space()).sum();
            let used: u64 = disks.iter().map(|d| d.total_space() - d.available_space()).sum();
            (used as f64 / 1_000_000_000.0, total as f64 / 1_000_000_000.0)
        };

        let (rx, tx) = {
            let mut rx = 0u64;
            let mut tx = 0u64;
            for (_, data) in nets {
                rx += data.total_received();
                tx += data.total_transmitted();
            }
            (rx, tx)
        };

        // Per-CPU-core utilization.
        let cpu_cores = sys
            .cpus()
            .iter()
            .enumerate()
            .map(|(i, c)| CpuCoreSample {
                index: i as u32,
                usage_pct: c.cpu_usage(),
            })
            .collect();

        // Per-interface network utilization.
        let networks = nets
            .iter()
            .map(|(name, data)| NetSample {
                interface: name.clone(),
                role: detect_role(name).to_string(),
                rx_bytes: data.total_received(),
                tx_bytes: data.total_transmitted(),
                rx_packets: data.total_packets_received(),
                tx_packets: data.total_packets_transmitted(),
                speed_mbps: None,
                up: true,
            })
            .collect();

        HostSample {
            hostname: self.hostname.clone(),
            cpu_usage_pct: cpu_usage,
            memory_used_mb: mem_used,
            memory_total_mb: mem_total,
            disk_used_gb: disk_used,
            disk_total_gb: disk_total,
            network_rx_bytes: rx,
            network_tx_bytes: tx,
            uptime_seconds: sysinfo::System::uptime(),
            cpu_cores,
            networks,
        }
    }
}

/// Auto-detect the role of a network interface from its name.
/// DGX Spark naming patterns:
/// - cluster: ConnectX-7 interfaces like enp1s0f0np0, enP2p1s0f0np0.
/// - main: onboard ethernet like eth0, enP7s7, eno, ens.
fn detect_role(name: &str) -> &'static str {
    if name.starts_with("enp1s0f") || name.starts_with("enP2p1s0f") {
        "cluster"
    } else if name.starts_with("eth")
        || name.starts_with("enP7s7")
        || name.starts_with("eno")
        || name.starts_with("ens")
    {
        "main"
    } else {
        "other"
    }
}