// SPDX-License-Identifier: Apache-2.0
//! dgmon — a lightweight system monitor for DGX Spark clusters.
//!
//! Two deployment modes:
//!
//!   **Push architecture** (recommended for clusters):
//!   - Each GPU node runs `dgmon push --config dgmon.json` (collector agent).
//!   - One central node runs `dgmon server` (aggregation server).
//!   - Collectors push snapshots to the server via HTTP POST `/ingest`.
//!   - Prometheus scrapes `/metrics` on the server for all nodes at once.
//!
//!   **Standalone mode** (single node):
//!   - `dgmon once`  — collect one snapshot, print to stdout.
//!   - `dgmon loop`  — collect on a loop, print to stdout.

mod api;
mod collect;
mod collector;
mod config;
mod http;
mod inference;
mod push;
mod server;
mod service;
mod storage;

use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use collector::{Collector, MockCollector, NvidiaSmiCollector};

#[derive(Parser)]
#[command(name = "dgmon", version, about = "Lightweight DGX Spark cluster monitor")]
struct Cli {
    /// Use the mock collector instead of the real GPU backend.
    #[arg(long, env = "DGMON_MOCK", global = true)]
    mock: bool,

    /// Collection interval in seconds (push, service, and loop modes).
    #[arg(long, env = "DGMON_INTERVAL", global = true, default_value = "5")]
    interval: u64,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Aggregation server: receive pushes from collector agents.
    /// Expose `/metrics`, `/snapshot`, `/nodes`, `/health`.
    Server {
        /// Listen address.
        #[arg(long, env = "DGMON_LISTEN", default_value = "0.0.0.0:9401")]
        listen: String,

        /// Data directory for time-series storage (enables history).
        /// When omitted, the server keeps only the latest snapshot per node.
        #[arg(long, env = "DGMON_DATA_DIR")]
        data_dir: Option<String>,
    },

    /// Standalone service: collect locally and expose HTTP endpoints.
    /// Use this on a single node (no push architecture).
    Service {
        /// Listen address.
        #[arg(long, env = "DGMON_LISTEN", default_value = "0.0.0.0:9401")]
        listen: String,

        /// Data directory for time-series storage (enables history).
        /// When omitted, the service keeps only the latest snapshot in memory.
        #[arg(long, env = "DGMON_DATA_DIR")]
        data_dir: Option<String>,

        /// Path to the JSON config file (inference servers, interface roles).
        #[arg(long, env = "DGMON_CONFIG")]
        config: Option<String>,
    },

    /// Push agent: collect locally and push snapshots to a remote dgmon server.
    /// Requires --config <file>.
    Push {
        /// Path to the JSON config file.
        #[arg(long, env = "DGMON_CONFIG")]
        config: String,
    },

    /// Collect once and print to stdout.
    Once,

    /// Collect on a loop and print to stdout.
    Loop,
}

fn make_collector(mock: bool) -> Arc<dyn Collector> {
    if mock {
        tracing::info!("using mock collector");
        Arc::new(MockCollector)
    } else {
        tracing::info!("using nvidia-smi collector");
        Arc::new(NvidiaSmiCollector::new())
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dgmon=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let interval = Duration::from_secs(cli.interval);

    match cli.command {
        Command::Server { listen, data_dir } => server::run(&listen, data_dir),

        Command::Service {
            listen,
            data_dir,
            config,
        } => {
            let collector = make_collector(cli.mock);
            // Load optional config for inference servers and interface roles.
            let (inference_servers, interface_role_overrides) = match config {
                Some(path) => {
                    let cfg = config::CollectorConfig::load(std::path::Path::new(&path))?;
                    (cfg.inference_servers, cfg.interface_role_overrides)
                }
                None => (Vec::new(), std::collections::HashMap::new()),
            };
            service::run(
                collector,
                &listen,
                interval,
                data_dir,
                inference_servers,
                interface_role_overrides,
            )
        }

        Command::Push { config } => {
            let cfg = config::CollectorConfig::load(std::path::Path::new(&config))?;
            let collector = make_collector(cfg.mock);
            push::run(cfg, collector)
        }

        Command::Once => {
            let collector = make_collector(cli.mock);
            collect::run_once(collector)
        }

        Command::Loop => {
            let collector = make_collector(cli.mock);
            collect::run_loop(collector, interval)
        }
    }
}
