// SPDX-License-Identifier: Apache-2.0
//! Push-mode collector agent.
//!
//! Runs the collector on a loop and pushes each snapshot
//! to a remote dgmon server via HTTP POST.

use std::sync::Arc;
use std::time::Duration;

use crate::collector::{Collector, Snapshot};
use crate::config::CollectorConfig;

/// HTTP send timeout for each push. Keeps the agent responsive when the
/// server is slow or unreachable.
const SEND_TIMEOUT: Duration = Duration::from_secs(1);

pub fn run(config: CollectorConfig, collector: Arc<dyn Collector>) -> anyhow::Result<()> {
    let url = config.server_url.clone();
    let interval = Duration::from_secs(config.interval_secs);
    let labels = config.labels.clone();

    // Build a ureq agent with a short global timeout so a slow or
    // unreachable server does not block the collection loop.
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(SEND_TIMEOUT))
            .build(),
    );

    tracing::info!(
        "push agent started: server={}, interval={}s, collector={}",
        url,
        interval.as_secs(),
        collector.name(),
    );

    loop {
        match collector.collect() {
            Ok(mut snap) => {
                // Override hostname if configured.
                if let Some(ref h) = config.hostname {
                    snap.host.hostname = h.clone();
                }
                // Merge config labels into the snapshot extra map.
                snap.extra.extend(labels.clone());

                match push(&agent, &url, &snap) {
                    Ok(()) => tracing::debug!("pushed snapshot to {}", url),
                    Err(e) => tracing::warn!("push to {} failed: {e:#}", url),
                }
            }
            Err(e) => tracing::warn!("collection failed: {e:#}"),
        }
        std::thread::sleep(interval);
    }
}

fn push(agent: &ureq::Agent, url: &str, snap: &Snapshot) -> anyhow::Result<()> {
    let body = serde_json::to_string(snap)?;
    let resp = agent
        .post(url)
        .content_type("application/json")
        .send(body.as_bytes())?;
    if resp.status().as_u16() >= 400 {
        anyhow::bail!("server returned HTTP {}", resp.status());
    }
    Ok(())
}