// SPDX-License-Identifier: Apache-2.0
//! Configuration for the collector agent.
//!
//! The collector reads a JSON config file that tells it where
//! to push snapshots. The file has this shape:
//!
//! ```json
//! {
//!   "server_url": "http://10.0.0.1:9401/ingest",
//!   "interval_secs": 5,
//!   "mock": false,
//!   "hostname": "node1",
//!   "labels": {
//!     "cluster": "dgx-spark-prod",
//!     "rack": "r1"
//!   }
//! }
//! ```

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Collector agent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorConfig {
    /// URL of the dgmon server ingest endpoint.
    #[serde(default)]
    pub server_url: String,

    /// Push interval in seconds.
    #[serde(default = "default_interval")]
    pub interval_secs: u64,

    /// Use the mock collector instead of nvidia-smi.
    #[serde(default)]
    pub mock: bool,

    /// Override the hostname reported with each snapshot.
    /// When omitted, the collector's default hostname is used.
    #[serde(default)]
    pub hostname: Option<String>,

    /// Extra labels attached to every snapshot from this node.
    #[serde(default)]
    pub labels: HashMap<String, String>,

    /// Manual inference server targets. Each entry is a base URL like
    /// `http://127.0.0.1:8000`. When set, discovery is skipped for these.
    #[serde(default)]
    pub inference_servers: Vec<String>,

    /// Optional per-interface role overrides. Keys are interface names,
    /// values are roles (main, cluster, other).
    #[serde(default)]
    pub interface_role_overrides: HashMap<String, String>,

    /// Data directory for time-series storage (server and service modes).
    #[serde(default)]
    pub data_dir: Option<String>,

    /// Listen address for the HTTP server (server and service modes).
    #[serde(default)]
    pub listen: Option<String>,
}

fn default_interval() -> u64 {
    5
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            interval_secs: 5,
            mock: false,
            hostname: None,
            labels: HashMap::new(),
            inference_servers: Vec::new(),
            interface_role_overrides: HashMap::new(),
            data_dir: None,
            listen: None,
        }
    }
}

impl CollectorConfig {
    /// Load from a JSON file. Missing keys use defaults.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("cannot read config {}: {e}", path.display()))?;
        let cfg: Self = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("cannot parse config {}: {e}", path.display()))?;
        Ok(cfg)
    }
}
