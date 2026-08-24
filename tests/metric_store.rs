// SPDX-License-Identifier: Apache-2.0
//! High-level integration tests for the metric store (TsinkStore).
//!
//! These tests exercise real-life use cases: ingestion, querying,
//! concurrency, and recovery. They use a temp directory for persistent
//! storage so recovery tests can close and reopen the store.

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

use chrono::{TimeZone, Utc};
use dgmon::collector::{CpuCoreSample, GpuSample, HostSample, InferenceSample, NetSample, Snapshot};
use dgmon::storage::TsinkStore;
use tempfile::TempDir;

/// Build a Snapshot with a controllable timestamp and hostname.
fn snapshot(ts_ms: i64, hostname: &str, gpu_count: u32) -> Snapshot {
    let gpus = (0..gpu_count)
        .map(|i| GpuSample {
            index: i,
            uuid: format!("GPU-{hostname}-{i}"),
            name: "NVIDIA DGX H200".into(),
            utilization_gpu: 40 + i,
            utilization_memory: 50,
            temperature_c: 55 + i,
            power_w: Some(350.0 + i as f32 * 10.0),
            power_limit_w: Some(700.0),
            memory_used_mb: Some(40_000 + i as u64 * 5_000),
            memory_total_mb: Some(140_000),
            fan_speed_pct: Some(60),
            pstate: "P0".into(),
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

    let host = HostSample {
        hostname: hostname.into(),
        cpu_usage_pct: 12.5,
        memory_used_mb: 64_000,
        memory_total_mb: 128_000,
        disk_used_gb: 300.0,
        disk_total_gb: 1000.0,
        network_rx_bytes: 1_000_000,
        network_tx_bytes: 2_000_000,
        uptime_seconds: 86_400,
        cpu_cores: vec![
            CpuCoreSample {
                index: 0,
                usage_pct: 10.0,
                freq_mhz: Some(2800),
                governor: Some("performance".into()),
                max_freq_mhz: Some(3900),
            },
            CpuCoreSample {
                index: 1,
                usage_pct: 20.0,
                freq_mhz: Some(2800),
                governor: Some("performance".into()),
                max_freq_mhz: Some(3900),
            },
        ],
        networks: vec![
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
        ],
    };

    Snapshot {
        timestamp: Utc.timestamp_millis_opt(ts_ms).unwrap(),
        host,
        gpus,
        inference: vec![InferenceSample {
            engine: "vllm".into(),
            model_name: "llama-3.1-8b-instruct".into(),
            metrics: vec![
                ("vllm:num_requests_running".into(), 3.0),
                ("vllm:kv_cache_usage_perc".into(), 0.45),
            ],
        }],
        extra: HashMap::from([("cluster".into(), "dgx-lab".into())]),
    }
}

/// Open a store at a temp dir path.
fn open_store(dir: &TempDir) -> TsinkStore {
    TsinkStore::open(dir.path().to_str().unwrap()).unwrap()
}

/// Return a timestamp (ms) near now, inside the retention window.
fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

// ---------------------------------------------------------------------
// Ingestion
// ---------------------------------------------------------------------

#[test]
fn write_snapshot_ingests_all_metric_types() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir);
    let snap = snapshot(now_ms(), "host1", 2);

    store.write_snapshot(&snap).unwrap();

    let metrics = store.list_metrics().unwrap();
    assert!(metrics.contains(&"dgmon_cpu_usage_pct".to_string()));
    assert!(metrics.contains(&"dgmon_memory_used_mb".to_string()));
    assert!(metrics.contains(&"dgmon_net_rx_bytes".to_string()));
    assert!(metrics.contains(&"dgmon_gpu_utilization".to_string()));
    assert!(metrics.contains(&"dgmon_gpu_power_w".to_string()));
    assert!(metrics.contains(&"dgmon_inference_num_requests_running".to_string()));
    assert!(metrics.contains(&"dgmon_inference_kv_cache_usage_perc".to_string()));
    store.close().unwrap();
}

#[test]
fn write_snapshot_round_trips_values() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir);
    let ts = now_ms();
    let snap = snapshot(ts, "host1", 1);

    store.write_snapshot(&snap).unwrap();

    let points = store
        .query("dgmon_cpu_usage_pct", "host1", ts - 1000, ts + 1000)
        .unwrap();
    assert_eq!(points.len(), 1);
    assert!((points[0].1 - 12.5).abs() < 1e-6);

    let gpu_points = store
        .query("dgmon_gpu_utilization", "host1", ts - 1000, ts + 1000)
        .unwrap();
    assert_eq!(gpu_points.len(), 1);
    assert!((gpu_points[0].1 - 40.0).abs() < 1e-6);
    store.close().unwrap();
}

#[test]
fn multiple_snapshots_accumulate_history() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir);
    let base = now_ms();

    for i in 0..5 {
        let mut snap = snapshot(base + i * 60_000, "host1", 1);
        snap.host.cpu_usage_pct = 10.0 + i as f32;
        store.write_snapshot(&snap).unwrap();
    }

    let points = store
        .query("dgmon_cpu_usage_pct", "host1", base - 1000, base + 5 * 60_000 + 1000)
        .unwrap();
    assert_eq!(points.len(), 5);
    store.close().unwrap();
}

// ---------------------------------------------------------------------
// Querying
// ---------------------------------------------------------------------

#[test]
fn query_returns_points_in_time_range() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir);
    let base = now_ms();

    for i in 0..10 {
        let mut snap = snapshot(base + i * 60_000, "host1", 1);
        snap.host.cpu_usage_pct = i as f32;
        store.write_snapshot(&snap).unwrap();
    }

    // Only points in [base+2min, base+5min] should be returned.
    // Only points in [base+2min, base+5min) should be returned.
    // tsink uses an exclusive end bound.
    let points = store
        .query(
            "dgmon_cpu_usage_pct",
            "host1",
            base + 2 * 60_000,
            base + 5 * 60_000,
        )
        .unwrap();
    assert_eq!(points.len(), 3);
    assert_eq!(points[2].0, base + 4 * 60_000);
    store.close().unwrap();
}

#[test]
fn query_filters_by_hostname() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir);
    let ts = now_ms();

    store.write_snapshot(&snapshot(ts, "hostA", 1)).unwrap();
    store.write_snapshot(&snapshot(ts, "hostB", 1)).unwrap();

    let a = store
        .query("dgmon_cpu_usage_pct", "hostA", ts - 1000, ts + 1000)
        .unwrap();
    let b = store
        .query("dgmon_cpu_usage_pct", "hostB", ts - 1000, ts + 1000)
        .unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    store.close().unwrap();
}

#[test]
fn list_metrics_deduplicates() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir);
    let ts = now_ms();

    // Two hosts produce the same metric names. list_metrics must return
    // each name once.
    store.write_snapshot(&snapshot(ts, "hostA", 2)).unwrap();
    store.write_snapshot(&snapshot(ts, "hostB", 2)).unwrap();

    let metrics = store.list_metrics().unwrap();
    let count = metrics.iter().filter(|m| *m == "dgmon_gpu_utilization").count();
    assert_eq!(count, 1, "metric names must be unique");
    store.close().unwrap();
}

#[test]
fn query_all_returns_labeled_series() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir);
    let ts = now_ms();

    store.write_snapshot(&snapshot(ts, "host1", 2)).unwrap();

    let series = store
        .query_all("dgmon_gpu_utilization", ts - 1000, ts + 1000)
        .unwrap();
    assert_eq!(series.len(), 2, "one series per GPU");

    for (labels, points) in &series {
        let gpu = labels.iter().find(|(k, _)| k == "gpu").unwrap().1.clone();
        assert!(!gpu.is_empty(), "series must carry a gpu label");
        assert_eq!(points.len(), 1);
    }
    store.close().unwrap();
}

#[test]
fn promql_instant_query() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir);
    let ts = now_ms();

    store.write_snapshot(&snapshot(ts, "host1", 1)).unwrap();

    let value = store.promql_instant("dgmon_cpu_usage_pct", ts).unwrap();
    let samples = value.as_instant_vector().unwrap();
    assert_eq!(samples.len(), 1);
    assert!((samples[0].value - 12.5).abs() < 1e-6);
    store.close().unwrap();
}

#[test]
fn promql_range_query() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir);
    let base = now_ms();

    for i in 0..5 {
        let mut snap = snapshot(base + i * 60_000, "host1", 1);
        snap.host.cpu_usage_pct = 10.0 + i as f32;
        store.write_snapshot(&snap).unwrap();
    }

    let value = store
        .promql_range("dgmon_cpu_usage_pct", base, base + 4 * 60_000, 60_000)
        .unwrap();
    let series = value.as_range_vector().unwrap();
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].samples.len(), 5);
    store.close().unwrap();
}

#[test]
fn promql_aggregation() {
    let dir = TempDir::new().unwrap();
    let store = open_store(&dir);
    let ts = now_ms();

    // Two GPUs on the same host with different utilization values.
    store.write_snapshot(&snapshot(ts, "host1", 2)).unwrap();

    let value = store.promql_instant("avg(dgmon_gpu_utilization)", ts).unwrap();
    let samples = value.as_instant_vector().unwrap();
    assert_eq!(samples.len(), 1);
    // GPU 0 = 40, GPU 1 = 41 => avg = 40.5
    assert!((samples[0].value - 40.5).abs() < 1e-6);
    store.close().unwrap();
}

// ---------------------------------------------------------------------
// Concurrency
// ---------------------------------------------------------------------

#[test]
fn concurrent_writes_are_consistent() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(open_store(&dir));
    let base = now_ms();
    let threads = 8;
    let snaps_per_thread = 25;

    let mut handles = Vec::new();
    for t in 0..threads {
        let store = Arc::clone(&store);
        handles.push(thread::spawn(move || {
            for i in 0..snaps_per_thread {
                let ts = base + (t * snaps_per_thread + i) as i64 * 1000;
                let mut snap = snapshot(ts, "host1", 1);
                snap.host.cpu_usage_pct = t as f32 * 100.0 + i as f32;
                store.write_snapshot(&snap).unwrap();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    let points = store
        .query("dgmon_cpu_usage_pct", "host1", base, base + threads as i64 * snaps_per_thread as i64 * 1000)
        .unwrap();
    assert_eq!(points.len(), threads * snaps_per_thread, "all writes must be present");
    store.close().unwrap();
}

#[test]
fn concurrent_reads_and_writes() {
    let dir = TempDir::new().unwrap();
    let store = Arc::new(open_store(&dir));
    let base = now_ms();
    let writer_threads = 4;
    let snaps_per_thread = 20;

    // Writers.
    let mut writer_handles = Vec::new();
    for t in 0..writer_threads {
        let store = Arc::clone(&store);
        writer_handles.push(thread::spawn(move || {
            for i in 0..snaps_per_thread {
                let ts = base + (t * snaps_per_thread + i) as i64 * 1000;
                let mut snap = snapshot(ts, "host1", 1);
                snap.host.cpu_usage_pct = t as f32 * 100.0 + i as f32;
                store.write_snapshot(&snap).unwrap();
            }
        }));
    }

    // Readers run concurrently with writers. They must not panic and must
    // eventually see all data.
    let reader_store = Arc::clone(&store);
    let reader = thread::spawn(move || {
        for _ in 0..50 {
            let _ = reader_store
                .query("dgmon_cpu_usage_pct", "host1", base, base + 1_000_000)
                .unwrap();
        }
    });

    for h in writer_handles {
        h.join().unwrap();
    }
    reader.join().unwrap();

    let points = store
        .query("dgmon_cpu_usage_pct", "host1", base, base + writer_threads as i64 * snaps_per_thread as i64 * 1000)
        .unwrap();
    assert_eq!(points.len(), writer_threads * snaps_per_thread);
    store.close().unwrap();
}

// ---------------------------------------------------------------------
// Recovery
// ---------------------------------------------------------------------

#[test]
fn data_survives_reopen() {
    let dir = TempDir::new().unwrap();
    let ts = now_ms();

    {
        let store = open_store(&dir);
        store.write_snapshot(&snapshot(ts, "host1", 1)).unwrap();
        store.close().unwrap();
    }

    {
        let store = open_store(&dir);
        let points = store
            .query("dgmon_cpu_usage_pct", "host1", ts - 1000, ts + 1000)
            .unwrap();
        assert_eq!(points.len(), 1);
        assert!((points[0].1 - 12.5).abs() < 1e-6);
        store.close().unwrap();
    }
}

#[test]
fn close_then_reopen_preserves_history() {
    let dir = TempDir::new().unwrap();
    let base = now_ms();

    {
        let store = open_store(&dir);
        for i in 0..5 {
            let mut snap = snapshot(base + i * 60_000, "host1", 1);
            snap.host.cpu_usage_pct = 10.0 + i as f32;
            store.write_snapshot(&snap).unwrap();
        }
        store.close().unwrap();
    }

    {
        let store = open_store(&dir);
        let points = store
            .query("dgmon_cpu_usage_pct", "host1", base - 1000, base + 5 * 60_000 + 1000)
            .unwrap();
        assert_eq!(points.len(), 5);
        store.close().unwrap();
    }
}
