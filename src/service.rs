// SPDX-License-Identifier: Apache-2.0
//! Standalone service mode: collect locally and expose HTTP endpoints.
//! Also accepts pushes from remote nodes via POST /ingest.
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
use crate::store::NodeStore;

/// Shared state for the service actix-web app.
struct ServiceState {
    store: Arc<NodeStore>,
    http: AppState,
}


/// GET /nodes — list of registered nodes.
async fn nodes(state: web::Data<ServiceState>) -> impl Responder {
    let nodes = state.store.node_list();
    HttpResponse::Ok()
        .content_type("application/json")
        .body(serde_json::to_string_pretty(&nodes).unwrap_or_default())
}

/// GET /metrics — Prometheus text format for all nodes.
async fn metrics(state: web::Data<ServiceState>) -> impl Responder {
    let snaps = state.store.all();
    let body = http::render_prometheus(&snaps);
    HttpResponse::Ok()
        .content_type("text/plain; version=0.0.4")
        .body(body)
}

/// POST /ingest — receive a snapshot push from a remote collector agent.
async fn ingest(
    state: web::Data<ServiceState>,
    body: web::Json<Snapshot>,
) -> impl Responder {
    let snap = body.into_inner();
    let hostname = snap.host.hostname.clone();
    let n_gpus = snap.gpus.len();

    // Write to tsink for historical storage.
    if let Err(e) = state.http.tsink.write_snapshot(&snap) {
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

pub fn run(
    collector: Arc<dyn Collector>,
    addr: &str,
    interval: Duration,
    data_dir: &str,
    inference_servers: Vec<String>,
    interface_role_overrides: HashMap<String, String>,
) -> anyhow::Result<()> {
    let store = Arc::new(NodeStore::new());

    // Open tsink time-series storage.
    tracing::info!("time-series storage enabled: {data_dir}");
    let tsink = Arc::new(TsinkStore::open(data_dir)?);

    let service_state = web::Data::new(ServiceState {
        store: Arc::clone(&store),
        http: AppState { tsink: Arc::clone(&tsink) },
    });

    // REST API state: expose the node store and tsink to /api/v1 handlers.
    let api_state = web::Data::new(ApiState {
        snapshots: store.clone() as Arc<dyn SnapshotSource + Send + Sync>,
        tsink: Arc::clone(&tsink),
    });

    let addr_owned = addr.to_string();
    let sys = actix_rt::System::new();

    // Clone tsink before the closure so it is available after the server stops.
    let tsink_shutdown = Arc::clone(&tsink);

    sys.block_on(async move {
        // Build an async reqwest client for inference scraping.
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("failed to build HTTP client: {e}");
                return Err(anyhow::anyhow!("failed to build HTTP client: {e}"));
            }
        };

        // Background collection loop, spawned on the same runtime as the
        // HTTP server. Uses tokio::time::interval for pacing and
        // spawn_blocking for the blocking collector call.
        let store_bg = Arc::clone(&store);
        let tsink_bg = Arc::clone(&tsink);
        let inference_servers_bg = inference_servers.clone();
        let interface_role_overrides_bg = interface_role_overrides.clone();
        actix_rt::spawn(collect_loop(
            store_bg,
            tsink_bg,
            collector,
            interval,
            inference_servers_bg,
            interface_role_overrides_bg,
            client,
        ));

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
                    control_enabled: false,
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

        tracing::info!("dgmon service listening on http://{addr_owned}");
        server.await.map_err(|e| anyhow::anyhow!("server error: {e}"))?;

        // Server has stopped. Flush tsink so no in-memory data is lost.
        if let Err(e) = tsink_shutdown.close() {
            tracing::warn!("tsink close failed: {e:#}");
        } else {
            tracing::info!("tsink storage flushed and closed");
        }

        Ok(())
    })
}

/// Background collection loop: collect locally and store in the node store.
async fn collect_loop(
    store: Arc<NodeStore>,
    tsink: Arc<TsinkStore>,
    collector: Arc<dyn Collector>,
    interval: Duration,
    inference_servers: Vec<String>,
    interface_role_overrides: HashMap<String, String>,
    client: reqwest::Client,
) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;

        // The collector call is blocking. Run it on a blocking thread so
        // it does not stall the async runtime.
        let collector = Arc::clone(&collector);
        let snap = tokio::task::spawn_blocking(move || collector.collect())
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("collection task join failed: {e}")));

        let mut snap = match snap {
            Ok(snap) => snap,
            Err(e) => {
                tracing::warn!("collection failed: {e:#}");
                continue;
            }
        };

        // Apply per-interface role overrides from config.
        for net in &mut snap.host.networks {
            if let Some(role) = interface_role_overrides.get(&net.interface) {
                net.role = role.clone();
            }
        }

        // Scrape inference metrics asynchronously.
        // Only scrape when the collector did not already provide inference
        // data (e.g. the mock collector supplies synthetic inference samples).
        if snap.inference.is_empty() {
            let inference_servers = inference_servers.clone();
            snap.inference = collect_inference(&client, &inference_servers).await;
        }

        let n = snap.gpus.len();
        tracing::debug!("collected snapshot: {n} GPUs");

        // Write to tsink for historical storage.
        if let Err(e) = tsink.write_snapshot(&snap) {
            tracing::warn!("tsink write failed: {e:#}");
        }

        let hostname = snap.host.hostname.clone();
        store.put(hostname, snap);
    }
}
