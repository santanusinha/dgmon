// SPDX-License-Identifier: Apache-2.0
//! Shared multi-node in-memory store of the latest snapshot per host.
//!
//! Used by both `server` and `service` modes. Holds the latest snapshot
//! per hostname for fast Prometheus scraping and JSON listing.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::api::SnapshotSource;
use crate::collector::Snapshot;

/// Multi-node in-memory store of the latest snapshot per host.
pub struct NodeStore {
    nodes: RwLock<HashMap<String, Snapshot>>,
}

impl NodeStore {
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
        }
    }

    pub fn put(&self, hostname: String, snap: Snapshot) {
        self.nodes
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(hostname, snap);
    }

    pub fn all(&self) -> Vec<Snapshot> {
        self.nodes
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    pub fn node_list(&self) -> Vec<NodeInfo> {
        self.nodes
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(host, snap)| NodeInfo {
                hostname: host.clone(),
                gpus: snap.gpus.len() as u32,
                timestamp: snap.timestamp,
            })
            .collect()
    }
}

impl SnapshotSource for NodeStore {
    fn all(&self) -> Vec<Snapshot> {
        self.all()
    }
}

#[derive(serde::Serialize)]
pub struct NodeInfo {
    pub hostname: String,
    pub gpus: u32,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
