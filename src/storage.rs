// SPDX-License-Identifier: Apache-2.0
//! Time-series storage backed by tsink.
//!
//! When the server starts with `--data-dir`, every ingested snapshot
//! is written to tsink as labeled time-series data. This gives the
//! server historical query capability and automatic retention.
//!
//! The in-memory HashMap in `server.rs` still holds the latest snapshot
//! per node for fast Prometheus scraping. tsink stores the full history.

use std::sync::Arc;
use std::time::Duration;

use tsink::promql::{Engine, PromqlValue};
use tsink::{DataPoint, Label, Row, Storage, StorageBuilder, TimestampPrecision};
use crate::collector::Snapshot;

/// Wraps a tsink `Storage` instance and converts dgmon snapshots
/// into labeled time-series rows.
/// Also holds a PromQL engine for expression-based queries.
pub struct TsinkStore {
    storage: Arc<dyn Storage>,
    engine: Engine,
}

impl TsinkStore {
    /// Open or create a tsink database at the given path.
    /// Retention defaults to 30 days.
    pub fn open(data_dir: &str) -> anyhow::Result<Self> {
        let storage = StorageBuilder::new()
            .with_data_path(data_dir)
            .with_retention(Duration::from_secs(30 * 24 * 3600))
            .with_timestamp_precision(TimestampPrecision::Milliseconds)
            .build()?;
        let engine = Engine::with_precision(
            Arc::clone(&storage),
            TimestampPrecision::Milliseconds,
        );
        Ok(Self { storage, engine })
    }

    /// Write all metrics from a snapshot into tsink.
    /// Write all metrics from a snapshot into tsink.
    pub fn write_snapshot(&self, snap: &Snapshot) -> anyhow::Result<()> {
        let ts = snap.timestamp.timestamp_millis();
        let host = &snap.host.hostname;
        let mut rows = Vec::new();

        // Base labels for every row: hostname plus extra metadata labels.
        let mut base_labels = vec![Label::new("hostname", host)];
        for (k, v) in &snap.extra {
            base_labels.push(Label::new(k, v));
        }

        // Host-level metrics.
        rows.push(Row::with_labels(
            "dgmon_cpu_usage_pct",
            base_labels.clone(),
            DataPoint::new(ts, snap.host.cpu_usage_pct as f64),
        ));
        rows.push(Row::with_labels(
            "dgmon_memory_used_mb",
            base_labels.clone(),
            DataPoint::new(ts, snap.host.memory_used_mb as f64),
        ));
        rows.push(Row::with_labels(
            "dgmon_memory_total_mb",
            base_labels.clone(),
            DataPoint::new(ts, snap.host.memory_total_mb as f64),
        ));
        rows.push(Row::with_labels(
            "dgmon_uptime_seconds",
            base_labels.clone(),
            DataPoint::new(ts, snap.host.uptime_seconds as f64),
        ));
        rows.push(Row::with_labels(
            "dgmon_disk_used_gb",
            base_labels.clone(),
            DataPoint::new(ts, snap.host.disk_used_gb),
        ));
        rows.push(Row::with_labels(
            "dgmon_disk_total_gb",
            base_labels.clone(),
            DataPoint::new(ts, snap.host.disk_total_gb),
        ));
        rows.push(Row::with_labels(
            "dgmon_network_rx_bytes",
            base_labels.clone(),
            DataPoint::new(ts, snap.host.network_rx_bytes as f64),
        ));
        rows.push(Row::with_labels(
            "dgmon_network_tx_bytes",
            base_labels.clone(),
            DataPoint::new(ts, snap.host.network_tx_bytes as f64),
        ));

        // Per-CPU-core utilization.
        for core in &snap.host.cpu_cores {
            let mut labels = base_labels.clone();
            labels.push(Label::new("core", core.index.to_string()));
            rows.push(Row::with_labels(
                "dgmon_cpu_core_usage_pct",
                labels,
                DataPoint::new(ts, core.usage_pct as f64),
            ));
        }

        // Per-interface network utilization.
        for net in &snap.host.networks {
            let mut labels = base_labels.clone();
            labels.push(Label::new("interface", &net.interface));
            labels.push(Label::new("role", &net.role));
            rows.push(Row::with_labels(
                "dgmon_net_rx_bytes",
                labels.clone(),
                DataPoint::new(ts, net.rx_bytes as f64),
            ));
            rows.push(Row::with_labels(
                "dgmon_net_tx_bytes",
                labels.clone(),
                DataPoint::new(ts, net.tx_bytes as f64),
            ));
            rows.push(Row::with_labels(
                "dgmon_net_rx_packets",
                labels.clone(),
                DataPoint::new(ts, net.rx_packets as f64),
            ));
            rows.push(Row::with_labels(
                "dgmon_net_tx_packets",
                labels.clone(),
                DataPoint::new(ts, net.tx_packets as f64),
            ));
            if let Some(speed) = net.speed_mbps {
                rows.push(Row::with_labels(
                    "dgmon_net_speed_mbps",
                    labels.clone(),
                    DataPoint::new(ts, speed as f64),
                ));
            }
            rows.push(Row::with_labels(
                "dgmon_net_up",
                labels,
                DataPoint::new(ts, if net.up { 1.0 } else { 0.0 }),
            ));
        }

        // GPU-level metrics.
        for g in &snap.gpus {
            let mut labels = base_labels.clone();
            labels.push(Label::new("gpu", g.index.to_string()));
            labels.push(Label::new("uuid", &g.uuid));
            labels.push(Label::new("model", &g.name));

            rows.push(Row::with_labels(
                "dgmon_gpu_utilization",
                labels.clone(),
                DataPoint::new(ts, g.utilization_gpu as f64),
            ));
            rows.push(Row::with_labels(
                "dgmon_gpu_mem_utilization",
                labels.clone(),
                DataPoint::new(ts, g.utilization_memory as f64),
            ));
            rows.push(Row::with_labels(
                "dgmon_gpu_temp_c",
                labels.clone(),
                DataPoint::new(ts, g.temperature_c as f64),
            ));

            if let Some(p) = g.power_w {
                rows.push(Row::with_labels(
                    "dgmon_gpu_power_w",
                    labels.clone(),
                    DataPoint::new(ts, p as f64),
                ));
            }
            if let Some(p) = g.power_limit_w {
                rows.push(Row::with_labels(
                    "dgmon_gpu_power_limit_w",
                    labels.clone(),
                    DataPoint::new(ts, p as f64),
                ));
            }
            if let Some(m) = g.memory_used_mb {
                rows.push(Row::with_labels(
                    "dgmon_gpu_memory_used_mb",
                    labels.clone(),
                    DataPoint::new(ts, m as f64),
                ));
            }
            if let Some(m) = g.memory_total_mb {
                rows.push(Row::with_labels(
                    "dgmon_gpu_memory_total_mb",
                    labels.clone(),
                    DataPoint::new(ts, m as f64),
                ));
            }
            if let Some(fan) = g.fan_speed_pct {
                rows.push(Row::with_labels(
                    "dgmon_gpu_fan_speed_pct",
                    labels.clone(),
                    DataPoint::new(ts, fan as f64),
                ));
            }

            // New granular GPU metrics.
            if let Some(v) = g.sm_clock_mhz {
                rows.push(Row::with_labels(
                    "dgmon_gpu_sm_clock_mhz",
                    labels.clone(),
                    DataPoint::new(ts, v as f64),
                ));
            }
            if let Some(v) = g.mem_clock_mhz {
                rows.push(Row::with_labels(
                    "dgmon_gpu_mem_clock_mhz",
                    labels.clone(),
                    DataPoint::new(ts, v as f64),
                ));
            }
            if let Some(v) = g.sm_clock_max_mhz {
                rows.push(Row::with_labels(
                    "dgmon_gpu_sm_clock_max_mhz",
                    labels.clone(),
                    DataPoint::new(ts, v as f64),
                ));
            }
            if let Some(v) = g.mem_clock_max_mhz {
                rows.push(Row::with_labels(
                    "dgmon_gpu_mem_clock_max_mhz",
                    labels.clone(),
                    DataPoint::new(ts, v as f64),
                ));
            }
            if let Some(v) = g.mem_temp_c {
                rows.push(Row::with_labels(
                    "dgmon_gpu_mem_temp_c",
                    labels.clone(),
                    DataPoint::new(ts, v as f64),
                ));
            }
            if let Some(v) = g.pcie_link_gen {
                rows.push(Row::with_labels(
                    "dgmon_gpu_pcie_link_gen",
                    labels.clone(),
                    DataPoint::new(ts, v as f64),
                ));
            }
            if let Some(v) = g.pcie_link_gen_max {
                rows.push(Row::with_labels(
                    "dgmon_gpu_pcie_link_gen_max",
                    labels.clone(),
                    DataPoint::new(ts, v as f64),
                ));
            }
            if let Some(v) = g.pcie_link_width {
                rows.push(Row::with_labels(
                    "dgmon_gpu_pcie_link_width",
                    labels.clone(),
                    DataPoint::new(ts, v as f64),
                ));
            }
            if let Some(v) = g.pcie_link_width_max {
                rows.push(Row::with_labels(
                    "dgmon_gpu_pcie_link_width_max",
                    labels.clone(),
                    DataPoint::new(ts, v as f64),
                ));
            }
        }

        // Inference metrics.
        for inf in &snap.inference {
            let mut labels = base_labels.clone();
            labels.push(Label::new("engine", &inf.engine));
            labels.push(Label::new("model_name", &inf.model_name));
            for (name, value) in &inf.metrics {
                // Sanitize the metric name into a valid Prometheus name.
                let metric = sanitize_metric_name(name);
                rows.push(Row::with_labels(
                    &metric,
                    labels.clone(),
                    DataPoint::new(ts, *value),
                ));
            }
        }

        self.storage.insert_rows(&rows)?;
        Ok(())
    }

    /// Query historical data points for a metric within a time range.
    /// Returns (timestamp_millis, value) pairs.
    ///
    /// Uses partial label matching: only the `hostname` label is required.
    /// This matches all series for the host, regardless of their other
    /// labels (gpu, core, interface, engine, model_name, etc.).
    pub fn query(
        &self,
        metric: &str,
        hostname: &str,
        start_ms: i64,
        end_ms: i64,
    ) -> anyhow::Result<Vec<(i64, f64)>> {
        let selection = tsink::SeriesSelection::new()
            .with_metric(metric)
            .with_matcher(tsink::SeriesMatcher::equal("hostname", hostname))
            .with_time_range(start_ms, end_ms);
        let series = self.storage.select_series(&selection)?;

        let mut out = Vec::new();
        for s in &series {
            let points = self
                .storage
                .select(&s.name, &s.labels, start_ms, end_ms)?;
            for p in &points {
                if let Some(v) = p.value_as_f64() {
                    out.push((p.timestamp, v));
                }
            }
        }
        out.sort_by_key(|(ts, _)| *ts);
        Ok(out)
    }

    /// List all metric names stored in tsink.
    pub fn list_metrics(&self) -> anyhow::Result<Vec<String>> {
        let metrics = self.storage.list_metrics()?;
        let mut names: Vec<String> = metrics.iter().map(|m| m.name.clone()).collect();
        // tsink returns one entry per series (metric + labels). Deduplicate
        // to a unique set of metric names.
        names.sort();
        names.dedup();
        Ok(names)
    }

    /// Query all series for a metric within a time range.
    /// Returns (labels, points) pairs, one per label set.
    pub fn query_all(
        &self,
        metric: &str,
        start_ms: i64,
        end_ms: i64,
    ) -> anyhow::Result<Vec<(Vec<(String, String)>, Vec<(i64, f64)>)>> {
        let series = self.storage.select_all(metric, start_ms, end_ms)?;
        Ok(series
            .into_iter()
            .map(|(labels, points)| {
                let labels: Vec<(String, String)> = labels
                    .into_iter()
                    .map(|l| (l.name, l.value))
                    .collect();
                let points: Vec<(i64, f64)> = points
                    .iter()
                    .filter_map(|p| p.value_as_f64().map(|v| (p.timestamp, v)))
                    .collect();
                (labels, points)
            })
            .collect())
    }

    /// Evaluate a PromQL instant query at the given timestamp (milliseconds).
    pub fn promql_instant(&self, query: &str, time_ms: i64) -> anyhow::Result<PromqlValue> {
        Ok(self.engine.instant_query(query, time_ms)?)
    }

    /// Evaluate a PromQL range query over [start, end] with the given step (milliseconds).
    pub fn promql_range(
        &self,
        query: &str,
        start_ms: i64,
        end_ms: i64,
        step_ms: i64,
    ) -> anyhow::Result<PromqlValue> {
        Ok(self.engine.range_query(query, start_ms, end_ms, step_ms)?)
    }

    /// Flush and close the underlying tsink storage.
    /// Call this during graceful shutdown to persist in-memory data.
    pub fn close(&self) -> anyhow::Result<()> {
        self.storage.close()?;
        Ok(())
    }
}

/// Convert a Prometheus metric name into a valid tsink metric name.
/// Replaces characters that are not alphanumeric or underscore with
/// underscores, and prefixes with `dgmon_inference_` to avoid collisions.
fn sanitize_metric_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 16);
    out.push_str("dgmon_inference_");
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}
