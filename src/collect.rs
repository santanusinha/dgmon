// SPDX-License-Identifier: Apache-2.0
//! Collector mode: run one collection cycle and print the result.

use std::sync::Arc;
use std::time::Duration;

use crate::collector::{Collector, Snapshot};

pub fn run_once(collector: Arc<dyn Collector>) -> anyhow::Result<()> {
    let snap = collector.collect()?;
    print_snapshot(&snap);
    Ok(())
}

pub fn run_loop(collector: Arc<dyn Collector>, interval: Duration) -> anyhow::Result<()> {
    loop {
        match collector.collect() {
            Ok(snap) => print_snapshot(&snap),
            Err(e) => tracing::error!("collection failed: {e:#}"),
        }
        std::thread::sleep(interval);
    }
}

fn fmt_opt<T: std::fmt::Display>(opt: Option<T>) -> String {
    match opt {
        Some(v) => v.to_string(),
        None => "N/A".to_string(),
    }
}

fn print_snapshot(snap: &Snapshot) {
    let ts = snap.timestamp.format("%Y-%m-%dT%H:%M:%SZ");
    println!("\n=== dgmon snapshot {ts} ===");
    println!("host: {} (uptime {}s)", snap.host.hostname, snap.host.uptime_seconds);
    println!(
        "cpu: {:.1}% | mem: {}/{} MiB | disk: {:.1}/{:.1} GB",
        snap.host.cpu_usage_pct,
        snap.host.memory_used_mb,
        snap.host.memory_total_mb,
        snap.host.disk_used_gb,
        snap.host.disk_total_gb,
    );

    if snap.gpus.is_empty() {
        println!("gpus: (none detected)");
        return;
    }

    println!(
        "{:>4} {:>36} {:>5} {:>5} {:>4} {:>7} {:>8} {:>12}",
        "idx", "uuid", "util%", "mem%", "temp", "power", "mem_used", "mem_total"
    );
    for g in &snap.gpus {
        println!(
            "{:>4} {:>36} {:>5} {:>5} {:>4} {:>6} {:>7} {:>10}",
            g.index,
            g.uuid,
            g.utilization_gpu,
            g.utilization_memory,
            g.temperature_c,
            format_opt_f32(g.power_w),
            fmt_opt(g.memory_used_mb),
            fmt_opt(g.memory_total_mb),
        );
    }
}

fn format_opt_f32(opt: Option<f32>) -> String {
    match opt {
        Some(v) => format!("{v:.1}W"),
        None => "N/A".to_string(),
    }
}