// SPDX-License-Identifier: Apache-2.0
//! Versioned REST API under `/api/v1/`.
//!
//! Provides clean, resource-oriented endpoints for querying cluster data
//! from any client (browser, script, mobile app). The existing endpoints
//! (`/nodes`, `/metrics`) remain for backward compatibility.
//!
//! Routes:
//!   GET /api/v1/nodes                          → list nodes
//!   GET /api/v1/nodes/{hostname}               → latest snapshot for one node
//!   GET /api/v1/nodes/{hostname}/host          → host metrics only
//!   GET /api/v1/nodes/{hostname}/gpus          → GPU list for one node
//!   GET /api/v1/nodes/{hostname}/gpus/{index}  → one GPU
//!   GET /api/v1/metrics                        → list available metric names
//!   GET /api/v1/metrics/{name}                 → latest value(s) for a metric
//!   GET /api/v1/metrics/{name}/history         → time-series for a metric

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Serialize;

use crate::collector::{GpuSample, Snapshot};
use crate::promapi;
use crate::storage::TsinkStore;

/// A node summary for the `/api/v1/nodes` listing.
#[derive(Serialize)]
pub struct NodeSummary {
    pub hostname: String,
    pub gpus: u32,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// A GPU summary for `/api/v1/nodes/{hostname}/gpus`.
#[derive(Serialize)]
pub struct GpuSummary {
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

/// A metric value at a point in time.
#[derive(Serialize)]
pub struct MetricPoint {
    pub timestamp: i64,
    pub value: f64,
    pub labels: Vec<(String, String)>,
}

/// A metric series (one per label set).
#[derive(Serialize)]
pub struct MetricSeries {
    pub metric: String,
    pub labels: Vec<(String, String)>,
    pub points: Vec<MetricPoint>,
}

/// Error response body.
#[derive(Serialize)]
pub struct ApiError {
    pub error: ApiErrorDetail,
}

#[derive(Serialize)]
pub struct ApiErrorDetail {
    pub code: String,
    pub message: String,
}

fn not_found(code: &str, message: &str) -> HttpResponse {
    HttpResponse::NotFound()
        .content_type("application/json")
        .body(serde_json::to_string(&ApiError {
            error: ApiErrorDetail {
                code: code.into(),
                message: message.into(),
            },
        })
        .unwrap_or_default())
}


/// Shared state for the REST API handlers.
pub struct ApiState {
    /// Latest snapshot per hostname (server mode) or single snapshot (service mode).
    pub snapshots: Arc<dyn SnapshotSource + Send + Sync>,
    pub tsink: Arc<TsinkStore>,
}

/// Abstraction over the snapshot store so the API works in both server and
/// service modes.
pub trait SnapshotSource {
    fn all(&self) -> Vec<Snapshot>;
}

/// GET /api/v1/nodes — list all nodes.
pub async fn nodes(state: web::Data<ApiState>) -> impl Responder {
    let snaps = state.snapshots.all();
    let list: Vec<NodeSummary> = snaps
        .iter()
        .map(|s| NodeSummary {
            hostname: s.host.hostname.clone(),
            gpus: s.gpus.len() as u32,
            timestamp: s.timestamp,
        })
        .collect();
    HttpResponse::Ok()
        .content_type("application/json")
        .body(serde_json::to_string_pretty(&list).unwrap_or_default())
}

/// GET /api/v1/nodes/{hostname} — latest snapshot for one node.
pub async fn node_snapshot(
    state: web::Data<ApiState>,
    path: web::Path<String>,
) -> impl Responder {
    let hostname = path.into_inner();
    match find_snapshot(&state, &hostname) {
        Some(snap) => HttpResponse::Ok()
            .content_type("application/json")
            .body(serde_json::to_string_pretty(&snap).unwrap_or_default()),
        None => not_found("node_not_found", &format!("no node named '{hostname}'")),
    }
}

/// GET /api/v1/nodes/{hostname}/host — host metrics only.
pub async fn node_host(
    state: web::Data<ApiState>,
    path: web::Path<String>,
) -> impl Responder {
    let hostname = path.into_inner();
    match find_snapshot(&state, &hostname) {
        Some(snap) => HttpResponse::Ok()
            .content_type("application/json")
            .body(serde_json::to_string_pretty(&snap.host).unwrap_or_default()),
        None => not_found("node_not_found", &format!("no node named '{hostname}'")),
    }
}

/// GET /api/v1/nodes/{hostname}/gpus — GPU list for one node.
pub async fn node_gpus(
    state: web::Data<ApiState>,
    path: web::Path<String>,
) -> impl Responder {
    let hostname = path.into_inner();
    match find_snapshot(&state, &hostname) {
        Some(snap) => {
            let gpus: Vec<GpuSummary> = snap.gpus.iter().map(gpu_summary).collect();
            HttpResponse::Ok()
                .content_type("application/json")
                .body(serde_json::to_string_pretty(&gpus).unwrap_or_default())
        }
        None => not_found("node_not_found", &format!("no node named '{hostname}'")),
    }
}

/// GET /api/v1/nodes/{hostname}/gpus/{index} — one GPU.
pub async fn node_gpu(
    state: web::Data<ApiState>,
    path: web::Path<(String, u32)>,
) -> impl Responder {
    let (hostname, index) = path.into_inner();
    match find_snapshot(&state, &hostname) {
        Some(snap) => match snap.gpus.iter().find(|g| g.index == index) {
            Some(g) => HttpResponse::Ok()
                .content_type("application/json")
                .body(serde_json::to_string_pretty(&gpu_summary(g)).unwrap_or_default()),
            None => not_found(
                "gpu_not_found",
                &format!("node '{hostname}' has no GPU with index {index}"),
            ),
        },
        None => not_found("node_not_found", &format!("no node named '{hostname}'")),
    }
}

/// GET /api/v1/metrics — list available metric names.
pub async fn metrics(state: web::Data<ApiState>) -> impl Responder {
    let ts = &state.tsink;
    match ts.list_metrics() {
        Ok(names) => HttpResponse::Ok()
            .content_type("application/json")
            .body(serde_json::to_string_pretty(&names).unwrap_or_default()),
        Err(e) => HttpResponse::InternalServerError()
            .content_type("application/json")
            .body(format!("{{\"error\":{{\"code\":\"storage_error\",\"message\":\"{e}\"}}}}")),
    }
}

/// GET /api/v1/metrics/{name} — latest value(s) for a metric.
pub async fn metric_latest(
    state: web::Data<ApiState>,
    path: web::Path<String>,
) -> impl Responder {
    let ts = &state.tsink;
    let name = path.into_inner();
    let now = chrono::Utc::now().timestamp_millis();
    // Look back 1 hour for the latest value.
    let start = now - 3_600_000;
    match ts.query_all(&name, start, now) {
        Ok(series) => {
            let latest: Vec<MetricSeries> = series
                .into_iter()
                .filter_map(|(labels, points)| {
                    let last = points.last()?;
                    Some(MetricSeries {
                        metric: name.clone(),
                        labels,
                        points: vec![MetricPoint {
                            timestamp: last.0,
                            value: last.1,
                            labels: vec![],
                        }],
                    })
                })
                .collect();
            HttpResponse::Ok()
                .content_type("application/json")
                .body(serde_json::to_string_pretty(&latest).unwrap_or_default())
        }
        Err(e) => HttpResponse::InternalServerError()
            .content_type("application/json")
            .body(format!("{{\"error\":{{\"code\":\"storage_error\",\"message\":\"{e}\"}}}}")),
    }
}

/// GET /api/v1/metrics/{name}/history?start=<ms>&end=<ms> — time-series for a metric.
pub async fn metric_history(
    state: web::Data<ApiState>,
    path: web::Path<String>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let ts = &state.tsink;
    let name = path.into_inner();
    let start_ms: i64 = query
        .get("start")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let end_ms: i64 = query
        .get("end")
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

    match ts.query_all(&name, start_ms, end_ms) {
        Ok(series) => {
            let out: Vec<MetricSeries> = series
                .into_iter()
                .map(|(labels, points)| MetricSeries {
                    metric: name.clone(),
                    labels,
                    points: points
                        .into_iter()
                        .map(|(ts, v)| MetricPoint {
                            timestamp: ts,
                            value: v,
                            labels: vec![],
                        })
                        .collect(),
                })
                .collect();
            HttpResponse::Ok()
                .content_type("application/json")
                .body(serde_json::to_string_pretty(&out).unwrap_or_default())
        }
        Err(e) => HttpResponse::InternalServerError()
            .content_type("application/json")
            .body(format!("{{\"error\":{{\"code\":\"storage_error\",\"message\":\"{e}\"}}}}")),
    }
}

fn find_snapshot(state: &web::Data<ApiState>, hostname: &str) -> Option<Snapshot> {
    state
        .snapshots
        .all()
        .into_iter()
        .find(|s| s.host.hostname == hostname)
}

fn gpu_summary(g: &GpuSample) -> GpuSummary {
    GpuSummary {
        index: g.index,
        uuid: g.uuid.clone(),
        name: g.name.clone(),
        utilization_gpu: g.utilization_gpu,
        utilization_memory: g.utilization_memory,
        temperature_c: g.temperature_c,
        power_w: g.power_w,
        power_limit_w: g.power_limit_w,
        memory_used_mb: g.memory_used_mb,
        memory_total_mb: g.memory_total_mb,
        fan_speed_pct: g.fan_speed_pct,
        pstate: g.pstate.clone(),
    }
}

/// Register the REST API routes on an actix-web `App`.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/v1")
            .route("/nodes", web::get().to(nodes))
            .route("/nodes/{hostname}", web::get().to(node_snapshot))
            .route("/nodes/{hostname}/host", web::get().to(node_host))
            .route("/nodes/{hostname}/gpus", web::get().to(node_gpus))
            .route("/nodes/{hostname}/gpus/{index}", web::get().to(node_gpu))
            .route("/metrics", web::get().to(metrics))
            .route("/metrics/{name}", web::get().to(metric_latest))
            .route("/metrics/{name}/history", web::get().to(metric_history))
            // Prometheus-compatible API routes share the same /api/v1 scope.
            .configure(promapi::configure),
    );
}
