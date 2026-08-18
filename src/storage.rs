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
    pub fn write_snapshot(&self, snap: &Snapshot) -> anyhow::Result<()> {
        let ts = snap.timestamp.timestamp_millis();
        let host = &snap.host.hostname;
        let mut rows = Vec::new();

        // Host-level metrics.
        rows.push(Row::with_labels(
            "dgmon_cpu_usage_pct",
            vec![Label::new("hostname", host)],
            DataPoint::new(ts, snap.host.cpu_usage_pct as f64),
        ));
        rows.push(Row::with_labels(
            "dgmon_memory_used_mb",
            vec![Label::new("hostname", host)],
            DataPoint::new(ts, snap.host.memory_used_mb as f64),
        ));
        rows.push(Row::with_labels(
            "dgmon_memory_total_mb",
            vec![Label::new("hostname", host)],
            DataPoint::new(ts, snap.host.memory_total_mb as f64),
        ));
        rows.push(Row::with_labels(
            "dgmon_uptime_seconds",
            vec![Label::new("hostname", host)],
            DataPoint::new(ts, snap.host.uptime_seconds as f64),
        ));
        rows.push(Row::with_labels(
            "dgmon_disk_used_gb",
            vec![Label::new("hostname", host)],
            DataPoint::new(ts, snap.host.disk_used_gb),
        ));
        rows.push(Row::with_labels(
            "dgmon_disk_total_gb",
            vec![Label::new("hostname", host)],
            DataPoint::new(ts, snap.host.disk_total_gb),
        ));
        rows.push(Row::with_labels(
            "dgmon_network_rx_bytes",
            vec![Label::new("hostname", host)],
            DataPoint::new(ts, snap.host.network_rx_bytes as f64),
        ));
        rows.push(Row::with_labels(
            "dgmon_network_tx_bytes",
            vec![Label::new("hostname", host)],
            DataPoint::new(ts, snap.host.network_tx_bytes as f64),
        ));

        // GPU-level metrics.
        for g in &snap.gpus {
            let labels = vec![
                Label::new("hostname", host),
                Label::new("gpu", &g.index.to_string()),
                Label::new("uuid", &g.uuid),
                Label::new("model", &g.name),
            ];

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
        }

        self.storage.insert_rows(&rows)?;
        Ok(())
    }

    /// Query historical data points for a metric within a time range.
    /// Returns (timestamp_millis, value) pairs.
    pub fn query(
        &self,
        metric: &str,
        hostname: &str,
        start_ms: i64,
        end_ms: i64,
    ) -> anyhow::Result<Vec<(i64, f64)>> {
        let labels = vec![Label::new("hostname", hostname)];
        let points = self
            .storage
            .select(metric, &labels, start_ms, end_ms)?;
        Ok(points
            .iter()
            .filter_map(|p| p.value_as_f64().map(|v| (p.timestamp, v)))
            .collect())
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
}
