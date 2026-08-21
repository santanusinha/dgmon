// SPDX-License-Identifier: Apache-2.0
//! Prometheus-compatible HTTP API.
//!
//! Grafana and other Prometheus clients expect the Prometheus HTTP API
//! envelope (`/api/v1/query`, `/api/v1/query_range`, `/api/v1/labels`,
//! `/api/v1/label/<name>/values`, `/api/v1/status/buildinfo`). dgmon
//! already evaluates PromQL via tsink; this module translates the results
//! into the standard Prometheus JSON format so a Prometheus datasource can
//! point directly at dgmon without a proxy.
//!
//! Time units: Prometheus uses seconds. tsink uses milliseconds. This
//! module converts on the boundary.

use std::collections::BTreeMap;
use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::storage::TsinkStore;
use tsink::promql::{PromqlValue, Sample, Series};

/// Prometheus API response envelope.
#[derive(Serialize)]
pub struct PromResponse {
    pub status: &'static str,
    pub data: PromData,
}

#[derive(Serialize)]
pub struct PromData {
    #[serde(rename = "resultType")]
    pub result_type: &'static str,
    pub result: serde_json::Value,
}

/// A single result item in a Prometheus query response.
#[derive(Serialize)]
pub struct PromResult {
    pub metric: BTreeMap<String, String>,
    /// Instant query: `[ts_sec, "value"]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Vec<serde_json::Value>>,
    /// Range query: `[[ts_sec, "value"], ...]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<Vec<serde_json::Value>>>,
}

/// Shared state for the Prometheus API handlers.
pub struct PromState {
    pub tsink: Option<Arc<TsinkStore>>,
}

/// A single query in a batch request.
#[derive(Deserialize)]
pub struct BatchQuery {
    /// Client-chosen id. Must be unique within the request.
    pub id: String,
    /// PromQL expression to evaluate.
    pub expr: String,
    /// Optional unix-seconds evaluation time. Defaults to now.
    #[serde(default)]
    pub time: Option<f64>,
    /// Optional range spec for a range query (sparklines).
    #[serde(default)]
    pub range: Option<RangeSpec>,
}

/// Range spec for a batch range query.
#[derive(Deserialize)]
pub struct RangeSpec {
    /// Start unix-seconds.
    pub start: f64,
    /// End unix-seconds.
    pub end: f64,
    /// Step in seconds.
    pub step: f64,
}

/// Request body for `POST /api/v1/query_batch`.
#[derive(Deserialize)]
pub struct BatchRequest {
    pub queries: Vec<BatchQuery>,
}

/// Build the metric label map, including `__name__`.
fn metric_map(metric: &str, labels: &[(String, String)]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("__name__".to_string(), metric.to_string());
    for (k, v) in labels {
        map.insert(k.clone(), v.clone());
    }
    map
}

/// Convert a tsink `Sample` into a Prometheus instant-vector result item.
fn sample_to_prom(s: &Sample) -> PromResult {
    let labels: Vec<(String, String)> = s
        .labels
        .iter()
        .map(|l| (l.name.clone(), l.value.clone()))
        .collect();
    PromResult {
        metric: metric_map(&s.metric, &labels),
        value: Some(vec![
            serde_json::json!(s.timestamp as f64 / 1000.0),
            serde_json::json!(s.value.to_string()),
        ]),
        values: None,
    }
}

/// Convert a tsink `Series` into a Prometheus matrix result item.
fn series_to_prom(s: &Series) -> PromResult {
    let labels: Vec<(String, String)> = s
        .labels
        .iter()
        .map(|l| (l.name.clone(), l.value.clone()))
        .collect();
    PromResult {
        metric: metric_map(&s.metric, &labels),
        value: None,
        values: Some(
            s.samples
                .iter()
                .map(|(ts, v)| {
                    vec![
                        serde_json::json!(*ts as f64 / 1000.0),
                        serde_json::json!(v.to_string()),
                    ]
                })
                .collect(),
        ),
    }
}

/// Convert a tsink `PromqlValue` into a Prometheus `PromData`.
fn promql_to_prom(val: &PromqlValue) -> PromData {
    match val {
        PromqlValue::InstantVector(samples) => PromData {
            result_type: "vector",
            result: serde_json::json!(samples.iter().map(sample_to_prom).collect::<Vec<_>>()),
        },
        PromqlValue::RangeVector(series) => PromData {
            result_type: "matrix",
            result: serde_json::json!(series.iter().map(series_to_prom).collect::<Vec<_>>()),
        },
        PromqlValue::Scalar(v, t) => PromData {
            result_type: "scalar",
            result: serde_json::json!([*t as f64 / 1000.0, v.to_string()]),
        },
        PromqlValue::String(s, t) => PromData {
            result_type: "string",
            result: serde_json::json!([*t as f64 / 1000.0, s]),
        },
    }
}

fn success(data: PromData) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/json")
        .body(serde_json::to_string(&PromResponse {
            status: "success",
            data,
        })
        .unwrap_or_default())
}

fn error_response(message: &str) -> HttpResponse {
    HttpResponse::BadRequest()
        .content_type("application/json")
        .body(
            serde_json::json!({
                "status": "error",
                "errorType": "bad_data",
                "error": message,
            })
            .to_string(),
        )
}

fn storage_unavailable() -> HttpResponse {
    HttpResponse::ServiceUnavailable()
        .content_type("application/json")
        .body(
            serde_json::json!({
                "status": "error",
                "errorType": "unavailable",
                "error": "time-series storage is disabled; start with --data-dir <path> or set DGMON_DATA_DIR",
            })
            .to_string(),
        )
}

/// GET /api/v1/query?query=<expr>&time=<unix_seconds>
pub async fn query(
    state: web::Data<PromState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let Some(ref ts) = state.tsink else {
        return storage_unavailable();
    };

    let expr = query.get("query").cloned().unwrap_or_default();
    if expr.is_empty() {
        return error_response("missing 'query' parameter");
    }

    // Prometheus time is in seconds; tsink uses milliseconds.
    let time_sec: f64 = query
        .get("time")
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp() as f64);
    let time_ms = (time_sec * 1000.0) as i64;

    match ts.promql_instant(&expr, time_ms) {
        Ok(val) => success(promql_to_prom(&val)),
        Err(e) => error_response(&format!("{e}")),
    }
}

/// GET /api/v1/query_range?query=<expr>&start=<s>&end=<s>&step=<s>
pub async fn query_range(
    state: web::Data<PromState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let Some(ref ts) = state.tsink else {
        return storage_unavailable();
    };

    let expr = query.get("query").cloned().unwrap_or_default();
    if expr.is_empty() {
        return error_response("missing 'query' parameter");
    }

    let start_sec: f64 = query
        .get("start")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let end_sec: f64 = query
        .get("end")
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp() as f64);
    let step_sec: f64 = query
        .get("step")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5.0);

    let start_ms = (start_sec * 1000.0) as i64;
    let end_ms = (end_sec * 1000.0) as i64;
    let step_ms = (step_sec * 1000.0).max(1.0) as i64;

    match ts.promql_range(&expr, start_ms, end_ms, step_ms) {
        Ok(val) => success(promql_to_prom(&val)),
        Err(e) => error_response(&format!("{e}")),
    }
}

/// GET /api/v1/labels — list all label names.
pub async fn labels(state: web::Data<PromState>) -> impl Responder {
    let Some(ref ts) = state.tsink else {
        return storage_unavailable();
    };

    let mut names = std::collections::BTreeSet::new();
    names.insert("__name__".to_string());
    match ts.list_metrics() {
        Ok(metrics) => {
            for m in &metrics {
                if let Ok(series) = ts.query_all(m, 0, chrono::Utc::now().timestamp_millis()) {
                    for (labels, _) in series {
                        for (k, _) in labels {
                            names.insert(k);
                        }
                    }
                }
            }
        }
        Err(e) => return error_response(&format!("{e}")),
    }

    HttpResponse::Ok()
        .content_type("application/json")
        .body(
            serde_json::json!({
                "status": "success",
                "data": names.into_iter().collect::<Vec<_>>(),
            })
            .to_string(),
        )
}

/// GET /api/v1/label/<name>/values — list values for one label.
pub async fn label_values(
    state: web::Data<PromState>,
    path: web::Path<String>,
) -> impl Responder {
    let Some(ref ts) = state.tsink else {
        return storage_unavailable();
    };

    let name = path.into_inner();
    let mut values = std::collections::BTreeSet::new();

    match ts.list_metrics() {
        Ok(metrics) => {
            for m in &metrics {
                if let Ok(series) = ts.query_all(m, 0, chrono::Utc::now().timestamp_millis()) {
                    for (labels, _) in series {
                        for (k, v) in &labels {
                            if k == &name {
                                values.insert(v.clone());
                            }
                        }
                    }
                }
            }
        }
        Err(e) => return error_response(&format!("{e}")),
    }

    HttpResponse::Ok()
        .content_type("application/json")
        .body(
            serde_json::json!({
                "status": "success",
                "data": values.into_iter().collect::<Vec<_>>(),
            })
            .to_string(),
        )
}

/// GET /api/v1/status/buildinfo — version info Grafana probes on connect.
pub async fn buildinfo() -> impl Responder {
    HttpResponse::Ok()
        .content_type("application/json")
        .body(
            serde_json::json!({
                "status": "success",
                "data": {
                    "version": env!("CARGO_PKG_VERSION"),
                    "revision": "",
                    "branch": "",
                    "buildUser": "dgmon",
                    "buildDate": "",
                    "goVersion": "rust",
                },
            })
            .to_string(),
        )
}

/// POST /api/v1/query_batch — evaluate many queries in one round trip.
///
/// The request body is a JSON object with a `queries` array. Each query has
/// an `id`, an `expr`, and optional `time` (unix seconds) or `range`
/// (start/end/step in seconds). The response maps each id to its
/// Prometheus-style result, or to an error object when that query fails.
/// Other queries still succeed when one fails.
pub async fn query_batch(
    state: web::Data<PromState>,
    body: web::Json<BatchRequest>,
) -> impl Responder {
    let Some(ref ts) = state.tsink else {
        return storage_unavailable();
    };

    let req = body.into_inner();
    if req.queries.is_empty() {
        return error_response("missing 'queries' array");
    }

    // Reject duplicate ids to keep the response map unambiguous.
    let mut seen = std::collections::HashSet::new();
    for q in &req.queries {
        if !seen.insert(q.id.clone()) {
            return error_response(&format!("duplicate query id '{}'", q.id));
        }
    }

    let mut data = serde_json::Map::new();
    for q in &req.queries {
        let result = if let Some(range) = &q.range {
            let start_ms = (range.start * 1000.0) as i64;
            let end_ms = (range.end * 1000.0) as i64;
            let step_ms = (range.step * 1000.0).max(1.0) as i64;
            ts.promql_range(&q.expr, start_ms, end_ms, step_ms)
                .map(|v| promql_to_prom(&v))
        } else {
            let time_sec = q.time.unwrap_or_else(|| chrono::Utc::now().timestamp() as f64);
            let time_ms = (time_sec * 1000.0) as i64;
            ts.promql_instant(&q.expr, time_ms)
                .map(|v| promql_to_prom(&v))
        };

        match result {
            Ok(prom) => {
                data.insert(q.id.clone(), serde_json::to_value(&prom).unwrap_or_default());
            }
            Err(e) => {
                data.insert(
                    q.id.clone(),
                    serde_json::json!({"error": format!("{e}")}),
                );
            }
        }
    }

    HttpResponse::Ok()
        .content_type("application/json")
        .body(
            serde_json::json!({
                "status": "success",
                "data": data,
            })
            .to_string(),
        )
}

/// Register the Prometheus-compatible API routes.
///
/// These routes are registered inside the `/api/v1` scope by `api::configure`.
/// They must NOT create their own scope, because actix-web matches the first
/// scope registered for a prefix and never falls through to a second scope
/// with the same prefix.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/query", web::get().to(query))
        .route("/query_range", web::get().to(query_range))
        .route("/query_batch", web::post().to(query_batch))
        .route("/labels", web::get().to(labels))
        .route("/label/{name}/values", web::get().to(label_values))
        .route("/status/buildinfo", web::get().to(buildinfo));
}
