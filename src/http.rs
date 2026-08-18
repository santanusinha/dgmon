// SPDX-License-Identifier: Apache-2.0
//! Shared actix-web HTTP handlers used by both server and service modes.
//!
//! This module contains the route handlers for the dashboard, static
//! assets, health check, history, PromQL query, and metrics listing.
//! The server and service modules provide their own data sources and
//! register these handlers on their actix-web `App`.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Serialize;

use crate::collector::Snapshot;
use crate::storage::TsinkStore;
use tsink::promql::{PromqlValue, Sample, Series};

/// JSON response for a historical query.
#[derive(Serialize)]
pub struct HistoryResponse {
    pub metric: String,
    pub hostname: String,
    pub points: Vec<HistoryPoint>,
}

#[derive(Serialize)]
pub struct HistoryPoint {
    pub timestamp: i64,
    pub value: f64,
}

/// JSON-serializable wrapper for a PromQL query result.
#[derive(Serialize)]
pub struct PromqlResponse {
    pub result_type: String,
    pub result: PromqlResult,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum PromqlResult {
    Scalar { value: f64, timestamp: i64 },
    String { value: String, timestamp: i64 },
    InstantVector(Vec<SampleJson>),
    RangeVector(Vec<SeriesJson>),
}

#[derive(Serialize)]
pub struct SampleJson {
    pub metric: String,
    pub labels: Vec<(String, String)>,
    pub timestamp: i64,
    pub value: f64,
}

#[derive(Serialize)]
pub struct SeriesJson {
    pub metric: String,
    pub labels: Vec<(String, String)>,
    pub samples: Vec<(i64, f64)>,
}

/// Convert a `tsink::promql::PromqlValue` into a JSON-serializable response.
pub fn promql_to_json(val: &PromqlValue) -> PromqlResponse {
    match val {
        PromqlValue::Scalar(v, t) => PromqlResponse {
            result_type: "scalar".into(),
            result: PromqlResult::Scalar { value: *v, timestamp: *t },
        },
        PromqlValue::String(s, t) => PromqlResponse {
            result_type: "string".into(),
            result: PromqlResult::String { value: s.clone(), timestamp: *t },
        },
        PromqlValue::InstantVector(samples) => PromqlResponse {
            result_type: "vector".into(),
            result: PromqlResult::InstantVector(
                samples.iter().map(sample_to_json).collect(),
            ),
        },
        PromqlValue::RangeVector(series) => PromqlResponse {
            result_type: "matrix".into(),
            result: PromqlResult::RangeVector(
                series.iter().map(series_to_json).collect(),
            ),
        },
    }
}

fn sample_to_json(s: &Sample) -> SampleJson {
    SampleJson {
        metric: s.metric.clone(),
        labels: s.labels.iter().map(|l| (l.name.clone(), l.value.clone())).collect(),
        timestamp: s.timestamp,
        value: s.value,
    }
}

fn series_to_json(s: &Series) -> SeriesJson {
    SeriesJson {
        metric: s.metric.clone(),
        labels: s.labels.iter().map(|l| (l.name.clone(), l.value.clone())).collect(),
        samples: s.samples.clone(),
    }
}

/// Shared application state for actix-web handlers.
pub struct AppState {
    pub tsink: Option<Arc<TsinkStore>>,
}

/// GET / — HTML landing page with endpoint links.
pub async fn index() -> impl Responder {
    let body = r#"<!DOCTYPE html>
<html><head><meta charset='utf-8'><title>dgmon</title>
<meta http-equiv='refresh' content='5'>
<style>body{font-family:monospace;margin:2em}table{border-collapse:collapse}td,th{border:1px solid #999;padding:4px 8px}</style>
</head><body><h1>dgmon</h1>
    <p>Endpoints: <a href='/dashboard'>/dashboard</a> | <a href='/nodes'>/nodes</a> | <a href='/snapshot'>/snapshot</a> | <a href='/metrics'>/metrics</a> | <a href='/history'>/history</a> | <a href='/query'>/query</a> | <a href='/metrics/list'>/metrics/list</a> | <a href='/health'>/health</a></p>
</body></html>"#;
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(body)
}

/// GET /dashboard — embedded HTML dashboard.
pub async fn dashboard() -> impl Responder {
    let body = include_str!("../dashboard/index.html");
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(body)
}

/// GET /static/style.css
pub async fn style_css() -> impl Responder {
    let body = include_str!("../dashboard/style.css");
    HttpResponse::Ok()
        .content_type("text/css; charset=utf-8")
        .body(body)
}

/// GET /static/app.js
pub async fn app_js() -> impl Responder {
    let body = include_str!("../dashboard/app.js");
    HttpResponse::Ok()
        .content_type("application/javascript; charset=utf-8")
        .body(body)
}

/// GET /static/chart.umd.min.js
pub async fn chart_js() -> impl Responder {
    let body = include_str!("../dashboard/static/chart.umd.min.js");
    HttpResponse::Ok()
        .content_type("application/javascript; charset=utf-8")
        .body(body)
}

/// GET /health — liveness probe.
/// GET /health — liveness probe.
pub async fn health() -> impl Responder {
    HttpResponse::Ok().body("ok\n")
}

/// Render snapshots in Prometheus text exposition format.
/// Shared by the server (all nodes) and the service (single node).
pub fn render_prometheus(snaps: &[Snapshot]) -> String {
    let mut out = String::new();

    for snap in snaps {
        let host = &snap.host.hostname;

        out.push_str("# HELP dgmon_cpu_usage_pct CPU usage percentage\n");
        out.push_str(&format!(
            "dgmon_cpu_usage_pct{{hostname=\"{}\"}} {:.1}\n",
            host, snap.host.cpu_usage_pct
        ));

        out.push_str("# HELP dgmon_memory_used_mb Memory used in MiB\n");
        out.push_str(&format!(
            "dgmon_memory_used_mb{{hostname=\"{}\"}} {}\n",
            host, snap.host.memory_used_mb
        ));

        out.push_str("# HELP dgmon_memory_total_mb Total memory in MiB\n");
        out.push_str(&format!(
            "dgmon_memory_total_mb{{hostname=\"{}\"}} {}\n",
            host, snap.host.memory_total_mb
        ));

        out.push_str("# HELP dgmon_uptime_seconds Host uptime in seconds\n");
        out.push_str(&format!(
            "dgmon_uptime_seconds{{hostname=\"{}\"}} {}\n",
            host, snap.host.uptime_seconds
        ));

        out.push_str("# HELP dgmon_disk_used_gb Disk space used in GB\n");
        out.push_str(&format!(
            "dgmon_disk_used_gb{{hostname=\"{}\"}} {:.2}\n",
            host, snap.host.disk_used_gb
        ));

        out.push_str("# HELP dgmon_disk_total_gb Total disk space in GB\n");
        out.push_str(&format!(
            "dgmon_disk_total_gb{{hostname=\"{}\"}} {:.2}\n",
            host, snap.host.disk_total_gb
        ));

        out.push_str("# HELP dgmon_network_rx_bytes Network bytes received\n");
        out.push_str(&format!(
            "dgmon_network_rx_bytes{{hostname=\"{}\"}} {}\n",
            host, snap.host.network_rx_bytes
        ));

        out.push_str("# HELP dgmon_network_tx_bytes Network bytes transmitted\n");
        out.push_str(&format!(
            "dgmon_network_tx_bytes{{hostname=\"{}\"}} {}\n",
            host, snap.host.network_tx_bytes
        ));

        for g in &snap.gpus {
            let labels = format!(
                "hostname=\"{}\",gpu=\"{}\",uuid=\"{}\",model=\"{}\"",
                host, g.index, g.uuid, g.name
            );

            out.push_str("# HELP dgmon_gpu_utilization GPU utilization percentage\n");
            out.push_str(&format!("dgmon_gpu_utilization{{{labels}}} {}\n", g.utilization_gpu));

            out.push_str("# HELP dgmon_gpu_mem_utilization GPU memory utilization percentage\n");
            out.push_str(&format!(
                "dgmon_gpu_mem_utilization{{{labels}}} {}\n",
                g.utilization_memory
            ));

            out.push_str("# HELP dgmon_gpu_temp_c GPU temperature in Celsius\n");
            out.push_str(&format!("dgmon_gpu_temp_c{{{labels}}} {}\n", g.temperature_c));

            if let Some(p) = g.power_w {
                out.push_str("# HELP dgmon_gpu_power_w GPU power draw in watts\n");
                out.push_str(&format!("dgmon_gpu_power_w{{{labels}}} {:.1}\n", p));
            }

            if let Some(p) = g.power_limit_w {
                out.push_str("# HELP dgmon_gpu_power_limit_w GPU power limit in watts\n");
                out.push_str(&format!("dgmon_gpu_power_limit_w{{{labels}}} {:.1}\n", p));
            }

            if let Some(m) = g.memory_used_mb {
                out.push_str("# HELP dgmon_gpu_memory_used_mb GPU memory used in MiB\n");
                out.push_str(&format!("dgmon_gpu_memory_used_mb{{{labels}}} {}\n", m));
            }

            if let Some(m) = g.memory_total_mb {
                out.push_str("# HELP dgmon_gpu_memory_total_mb GPU memory total in MiB\n");
                out.push_str(&format!("dgmon_gpu_memory_total_mb{{{labels}}} {}\n", m));
            }

            if let Some(fan) = g.fan_speed_pct {
                out.push_str("# HELP dgmon_gpu_fan_speed_pct GPU fan speed percentage\n");
                out.push_str(&format!("dgmon_gpu_fan_speed_pct{{{labels}}} {fan}\n"));
            }
        }
    }

    out
}

/// GET /history?metric=<name>&hostname=<host>&start=<ms>&end=<ms>
pub async fn history(
    state: web::Data<AppState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let Some(ref ts) = state.tsink else {
        return HttpResponse::ServiceUnavailable()
            .content_type("application/json")
            .body(r#"{"error":"history requires time-series storage; start with --data-dir <path> or set DGMON_DATA_DIR"}"#);
    };

    let metric = query.get("metric").cloned().unwrap_or_default();
    let hostname = query.get("hostname").cloned().unwrap_or_default();
    let start_ms: i64 = query
        .get("start")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let end_ms: i64 = query
        .get("end")
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

    if metric.is_empty() || hostname.is_empty() {
        return HttpResponse::BadRequest()
            .content_type("application/json")
            .body(r#"{"error":"missing 'metric' or 'hostname' query parameter"}"#);
    }

    match ts.query(&metric, &hostname, start_ms, end_ms) {
        Ok(points) => {
            let resp = HistoryResponse {
                metric,
                hostname,
                points: points
                    .iter()
                    .map(|(ts, val)| HistoryPoint {
                        timestamp: *ts,
                        value: *val,
                    })
                    .collect(),
            };
            HttpResponse::Ok()
                .content_type("application/json")
                .body(serde_json::to_string_pretty(&resp).unwrap_or_default())
        }
        Err(e) => HttpResponse::InternalServerError()
            .content_type("application/json")
            .body(format!("{{\"error\":\"{e}\"}}")),
    }
}

/// GET /metrics/list — JSON array of stored metric names.
pub async fn metrics_list(state: web::Data<AppState>) -> impl Responder {
    let Some(ref ts) = state.tsink else {
        return HttpResponse::ServiceUnavailable()
            .content_type("application/json")
            .body(r#"{"error":"metrics list requires time-series storage; start with --data-dir <path> or set DGMON_DATA_DIR"}"#);
    };

    match ts.list_metrics() {
        Ok(metrics) => HttpResponse::Ok()
            .content_type("application/json")
            .body(serde_json::to_string_pretty(&metrics).unwrap_or_default()),
        Err(e) => HttpResponse::InternalServerError()
            .content_type("application/json")
            .body(format!("{{\"error\":\"{e}\"}}")),
    }
}

/// GET /query?q=<promql>&time=<ms>  — instant query
/// GET /query?q=<promql>&start=<ms>&end=<ms>&step=<ms>  — range query
pub async fn query(
    state: web::Data<AppState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let Some(ref ts) = state.tsink else {
        return HttpResponse::ServiceUnavailable()
            .content_type("application/json")
            .body(r#"{"error":"query requires time-series storage; start with --data-dir <path> or set DGMON_DATA_DIR"}"#);
    };

    let q = query.get("q").cloned().unwrap_or_default();
    let time_ms: i64 = query
        .get("time")
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let start_ms: i64 = query
        .get("start")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let end_ms: i64 = query
        .get("end")
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let step_ms: i64 = query
        .get("step")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5000);

    if q.is_empty() {
        return HttpResponse::BadRequest()
            .content_type("application/json")
            .body(r#"{"error":"missing 'q' query parameter"}"#);
    }

    let result = if query.contains_key("start") || query.contains_key("end") {
        // Range query.
        ts.promql_range(&q, start_ms, end_ms, step_ms)
    } else {
        // Instant query.
        ts.promql_instant(&q, time_ms)
    };

    match result {
        Ok(val) => {
            let resp = promql_to_json(&val);
            HttpResponse::Ok()
                .content_type("application/json")
                .body(serde_json::to_string_pretty(&resp).unwrap_or_default())
        }
        Err(e) => HttpResponse::InternalServerError()
            .content_type("application/json")
            .body(format!("{{\"error\":\"{e}\"}}")),
    }
}
