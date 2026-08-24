// SPDX-License-Identifier: Apache-2.0
//! Server mode: an HTTP server that receives pushes from collector
//! agents and exposes aggregated snapshots as JSON and Prometheus metrics.
//!
//! In a cluster deployment:
//!   - Each GPU node runs `dgmon push` (the collector agent).
//!   - One central node runs `dgmon server` (this module).
//!   - The server stores the latest snapshot from every node.
//!   - Prometheus scrapes `/metrics` on the server to get all nodes at once.

use std::sync::Arc;

use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};

use crate::api::{self, ApiState, SnapshotSource};
use crate::collector::Snapshot;
use crate::http::{self, AppState};
use crate::promapi::PromState;
use crate::storage::TsinkStore;
use crate::store::NodeStore;

/// Shared state for the server actix-web app.
struct ServerState {
    store: Arc<NodeStore>,
    http: AppState,
}

/// GET /nodes — list of registered nodes.
async fn nodes(state: web::Data<ServerState>) -> impl Responder {
    let nodes = state.store.node_list();
    HttpResponse::Ok()
        .content_type("application/json")
        .body(serde_json::to_string_pretty(&nodes).unwrap_or_default())
}

/// GET /metrics — Prometheus text format for all nodes.
async fn metrics(state: web::Data<ServerState>) -> impl Responder {
    let snaps = state.store.all();
    let body = http::render_prometheus(&snaps);
    HttpResponse::Ok()
        .content_type("text/plain; version=0.0.4")
        .body(body)
}

/// POST /ingest — receive a snapshot push from a collector agent.
async fn ingest(
    state: web::Data<ServerState>,
    body: web::Json<Snapshot>,
) -> impl Responder {
    let snap = body.into_inner();
    let hostname = snap.host.hostname.clone();
    let n_gpus = snap.gpus.len();

    // Write to tsink for historical storage.
    if let Some(ref ts) = state.http.tsink
        && let Err(e) = ts.write_snapshot(&snap)
    {
        tracing::warn!("tsink write failed for {hostname}: {e:#}");
    }

    state.store.put(hostname.clone(), snap);
    tracing::debug!("ingested snapshot from {hostname}: {n_gpus} GPUs");
    HttpResponse::Ok().body("ok\n")
}

/// Fallback 404 handler.
async fn not_found() -> impl Responder {
    HttpResponse::NotFound().finish()
}

pub fn run(addr: &str, data_dir: Option<String>) -> anyhow::Result<()> {
    let store = Arc::new(NodeStore::new());

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

    let server_state = web::Data::new(ServerState {
        store: Arc::clone(&store),
        http: AppState { tsink },
    });

    // REST API state: expose the node store and tsink to /api/v1 handlers.
    let api_state = web::Data::new(ApiState {
        snapshots: store.clone() as Arc<dyn SnapshotSource + Send + Sync>,
        tsink: server_state.http.tsink.clone(),
    });

    let addr_owned = addr.to_string();
    let sys = actix_rt::System::new();

    // Clone tsink before the closure so it is available after the server stops.
    let tsink_shutdown = server_state.http.tsink.clone();

    sys.block_on(async move {
        let server = HttpServer::new(move || {
            let cors = Cors::permissive();
            App::new()
                .wrap(cors)
                .app_data(server_state.clone())
                .app_data(api_state.clone())
                .app_data(web::Data::new(http::AppState {
                    tsink: server_state.http.tsink.clone(),
                }))
                .configure(api::configure)
                .app_data(web::Data::new(PromState {
                    tsink: server_state.http.tsink.clone(),
                }))
                .route("/", web::get().to(http::index))
                .route("/dashboard", web::get().to(http::dashboard))
                .route("/static/style.css", web::get().to(http::style_css))
                .route("/static/app.js", web::get().to(http::app_js))
                .route("/static/chart.umd.min.js", web::get().to(http::chart_js))
                .route("/health", web::get().to(http::health))
                .route("/nodes", web::get().to(nodes))
                .route("/metrics", web::get().to(metrics))
                .route("/ingest", web::post().to(ingest))
                .route("/history", web::get().to(http::history))
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

        tracing::info!("dgmon server listening on http://{addr_owned}");
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
