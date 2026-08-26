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
mod metric_name;
mod promapi;
mod store;
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
    /// Expose `/metrics`, `/nodes`, `/health`.
    Server {
        /// Listen address. Overrides the config file value.
        #[arg(long, env = "DGMON_LISTEN")]
        listen: Option<String>,

        /// Data directory for time-series storage. Overrides the config file value.
        #[arg(long, env = "DGMON_DATA_DIR")]
        data_dir: Option<String>,

        /// Path to the JSON config file. When omitted, --data-dir is required.
        #[arg(long, env = "DGMON_CONFIG")]
        config: Option<String>,
    },

    /// Standalone service: collect locally and expose HTTP endpoints.
    /// Use this on a single node (no push architecture).
    Service {
        /// Listen address. Overrides the config file value.
        #[arg(long, env = "DGMON_LISTEN")]
        listen: Option<String>,

        /// Data directory for time-series storage. Overrides the config file value.
        #[arg(long, env = "DGMON_DATA_DIR")]
        data_dir: Option<String>,

        /// Path to the JSON config file. When omitted, --data-dir is required.
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

/// Load the config file, or use defaults when no path is given.
fn load_config(path: Option<&str>) -> anyhow::Result<config::CollectorConfig> {
    match path {
        Some(p) => config::CollectorConfig::load(std::path::Path::new(p)),
        None => Ok(config::CollectorConfig::default()),
    }
}

/// Resolve the data directory. CLI overrides config; otherwise error.
fn resolve_data_dir(cli: Option<String>, cfg: Option<&str>) -> anyhow::Result<String> {
    if let Some(dir) = cli {
        return Ok(dir);
    }
    if let Some(dir) = cfg {
        return Ok(dir.to_string());
    }
    anyhow::bail!(
        "no data directory given; pass --data-dir <path> or set data_dir in the config file"
    )
}

/// Resolve the listen address. CLI overrides config; default otherwise.
fn resolve_listen(cli: Option<String>, cfg: Option<&str>) -> String {
    cli.or_else(|| cfg.map(str::to_string))
        .unwrap_or_else(|| "0.0.0.0:9401".to_string())
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
        Command::Server {
            listen,
            data_dir,
            config,
        } => {
            let cfg = load_config(config.as_deref())?;
            let data_dir = resolve_data_dir(data_dir, cfg.data_dir.as_deref())?;
            let listen = resolve_listen(listen, cfg.listen.as_deref());
            server::run(&listen, &data_dir, cfg)
        }

        Command::Service {
            listen,
            data_dir,
            config,
        } => {
            let collector = make_collector(cli.mock);
            let cfg = load_config(config.as_deref())?;
            let data_dir = resolve_data_dir(data_dir, cfg.data_dir.as_deref())?;
            let listen = resolve_listen(listen, cfg.listen.as_deref());
            service::run(
                collector,
                &listen,
                interval,
                &data_dir,
                cfg.inference_servers,
                cfg.interface_role_overrides,
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
