// SPDX-License-Identifier: Apache-2.0
//! Standalone service mode: collect locally and expose HTTP endpoints.
//!
//! Use this on a single node that does both collection and serving.
//! For multi-node clusters, use `dgmon server` + `dgmon push` instead.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};

use crate::api::{self, ApiState, SnapshotSource};
use crate::collector::{Collector, Snapshot};
use crate::http::{self, AppState};
use crate::promapi::PromState;
use crate::inference::collect_inference;
use crate::storage::TsinkStore;

/// Shared state for the service actix-web app.
struct ServiceState {
    store: Arc<Store>,
    http: AppState,
}

/// GET /snapshot — latest snapshot as JSON.
async fn json(state: web::Data<ServiceState>) -> impl Responder {
    let snap = state.store.latest();
    match serde_json::to_string_pretty(&snap) {
        Ok(json) => HttpResponse::Ok()
            .content_type("application/json")
            .body(json),
        Err(e) => HttpResponse::InternalServerError()
            .content_type("application/json")
            .body(format!("{{\"error\":\"{e}\"}}")),
    }
}

/// GET /nodes — list of registered nodes (single node in service mode).
async fn nodes(state: web::Data<ServiceState>) -> impl Responder {
    let snap = state.store.latest();
    let info = serde_json::json!([{
        "hostname": snap.host.hostname,
        "gpus": snap.gpus.len() as u32,
        "timestamp": snap.timestamp,
    }]);
    HttpResponse::Ok()
        .content_type("application/json")
        .body(serde_json::to_string_pretty(&info).unwrap_or_default())
}

/// GET /metrics — Prometheus text format for the local node.
/// GET /metrics — Prometheus text format for the local node.
async fn metrics(state: web::Data<ServiceState>) -> impl Responder {
    let snap = state.store.latest();
    let body = http::render_prometheus(std::slice::from_ref(&snap));
    HttpResponse::Ok()
        .content_type("text/plain; version=0.0.4")
        .body(body)
}

/// Fallback 404 handler.
async fn not_found() -> impl Responder {
    HttpResponse::NotFound().finish()
}

pub fn run(
    collector: Arc<dyn Collector>,
    addr: &str,
    interval: Duration,
    data_dir: Option<String>,
    inference_servers: Vec<String>,
    interface_role_overrides: HashMap<String, String>,
) -> anyhow::Result<()> {
    let store = Arc::new(Store::new(collector, interval, interface_role_overrides));

    // Open tsink time-series storage if a data directory is provided.
    let tsink: Option<Arc<TsinkStore>> = match data_dir {
        Some(dir) => {
            tracing::info!("time-series storage enabled: {dir}");
            let ts = Arc::new(TsinkStore::open(&dir)?);
            Some(ts)
        }
        None => {
            tracing::info!("time-series storage disabled (no --data-dir)");
            None
        }
    };

    // Background collection loop: collect and write to tsink.
    let store_bg = Arc::clone(&store);
    let tsink_bg = tsink.clone();
    let inference_servers_bg = inference_servers.clone();
    std::thread::spawn(move || {
        store_bg.collect_loop(tsink_bg, inference_servers_bg)
    });

    let service_state = web::Data::new(ServiceState {
        store: Arc::clone(&store),
        http: AppState { tsink },
    });

    // REST API state: expose the local store and tsink to /api/v1 handlers.
    let api_state = web::Data::new(ApiState {
        snapshots: store.clone() as Arc<dyn SnapshotSource + Send + Sync>,
        tsink: service_state.http.tsink.clone(),
    });

    let addr_owned = addr.to_string();
    let sys = actix_rt::System::new();

    // Clone tsink before the closure so it is available after the server stops.
    let tsink_shutdown = service_state.http.tsink.clone();

    sys.block_on(async move {
        let server = HttpServer::new(move || {
            let cors = Cors::permissive();
            App::new()
                .wrap(cors)
                .app_data(service_state.clone())
                .app_data(api_state.clone())
                .app_data(web::Data::new(http::AppState {
                    tsink: service_state.http.tsink.clone(),
                }))
                .configure(api::configure)
                .app_data(web::Data::new(PromState {
                    tsink: service_state.http.tsink.clone(),
                }))
                .route("/", web::get().to(http::index))
                .route("/dashboard", web::get().to(http::dashboard))
                .route("/static/style.css", web::get().to(http::style_css))
                .route("/static/app.js", web::get().to(http::app_js))
                .route("/static/chart.umd.min.js", web::get().to(http::chart_js))
                .route("/health", web::get().to(http::health))
                .route("/nodes", web::get().to(nodes))
                .route("/snapshot", web::get().to(json))
                .route("/metrics", web::get().to(metrics))
                .route("/history", web::get().to(http::history))
                .route("/metrics/list", web::get().to(http::metrics_list))
                .route("/query", web::get().to(http::query))
                .default_service(web::route().to(not_found))
        })
        .bind(&addr_owned)
        .map_err(|e| anyhow::anyhow!("bind {addr_owned} failed: {e}"))?
        .run();

        let handle = server.handle();

        // Spawn a task that waits for SIGINT or SIGTERM, then stops the
        // server gracefully so in-flight requests finish and tsink flushes.
        actix_rt::spawn(async move {
            use actix_rt::signal::unix::{signal, SignalKind};

            let mut sigterm =
                signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
            let mut sigint =
                signal(SignalKind::interrupt()).expect("failed to install SIGINT handler");

            tokio::select! {
                _ = sigterm.recv() => tracing::info!("received SIGTERM, shutting down"),
                _ = sigint.recv() => tracing::info!("received SIGINT, shutting down"),
            }

            handle.stop(true).await;
        });

        tracing::info!("dgmon service listening on http://{addr_owned}");
        server.await.map_err(|e| anyhow::anyhow!("server error: {e}"))?;

        // Server has stopped. Flush tsink so no in-memory data is lost.
        if let Some(ref ts) = tsink_shutdown {
            if let Err(e) = ts.close() {
                tracing::warn!("tsink close failed: {e:#}");
            } else {
                tracing::info!("tsink storage flushed and closed");
            }
        }

        Ok(())
    })
}

impl SnapshotSource for Store {
    fn all(&self) -> Vec<Snapshot> {
        vec![self.latest()]
    }
}

struct Store {
    collector: Arc<dyn Collector>,
    interval: Duration,
    interface_role_overrides: HashMap<String, String>,
    latest: std::sync::RwLock<Option<Snapshot>>,
}

impl Store {
    fn new(
        collector: Arc<dyn Collector>,
        interval: Duration,
        interface_role_overrides: HashMap<String, String>,
    ) -> Self {
        Self {
            collector,
            interval,
            interface_role_overrides,
            latest: std::sync::RwLock::new(None),
        }
    }

    fn latest(&self) -> Snapshot {
        self.latest.read().unwrap().clone().unwrap_or_else(|| {
            Snapshot {
                timestamp: chrono::Utc::now(),
                host: crate::collector::HostSample {
                    hostname: "pending".into(),
                    cpu_usage_pct: 0.0,
                    memory_used_mb: 0,
                    memory_total_mb: 0,
                    disk_used_gb: 0.0,
                    disk_total_gb: 0.0,
                    network_rx_bytes: 0,
                    network_tx_bytes: 0,
                    uptime_seconds: 0,
                    cpu_cores: vec![],
                    networks: vec![],
                },
                gpus: vec![],
                inference: vec![],
                extra: std::collections::HashMap::new(),
            }
        })
    }

    fn collect_loop(&self, tsink: Option<Arc<TsinkStore>>, inference_servers: Vec<String>) {
        // Build an async reqwest client for inference scraping.
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("failed to build HTTP client: {e}");
                return;
            }
        };

        // Build a tokio runtime for async inference collection.
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("failed to build tokio runtime: {e}");
                return;
            }
        };

        loop {
            let mut snap = match self.collector.collect() {
                Ok(snap) => snap,
                Err(e) => {
                    tracing::warn!("collection failed: {e:#}");
                    std::thread::sleep(self.interval);
                    continue;
                }
            };

            // Apply per-interface role overrides from config.
            for net in &mut snap.host.networks {
                if let Some(role) = self.interface_role_overrides.get(&net.interface) {
                    net.role = role.clone();
                }
            }

            // Scrape inference metrics asynchronously.
            let inference_servers = inference_servers.clone();
            snap.inference = rt.block_on(collect_inference(&client, &inference_servers));

            let n = snap.gpus.len();
            tracing::debug!("collected snapshot: {n} GPUs");

            // Write to tsink for historical storage.
            if let Some(ref ts) = tsink
                && let Err(e) = ts.write_snapshot(&snap)
            {
                tracing::warn!("tsink write failed: {e:#}");
            }

            *self.latest.write().unwrap() = Some(snap);
            std::thread::sleep(self.interval);
        }
    }
}
