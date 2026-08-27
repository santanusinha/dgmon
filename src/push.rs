// SPDX-License-Identifier: Apache-2.0
//! Push-mode collector agent.
//!
//! Runs the collector on a loop and pushes each snapshot
//! to a remote dgmon server via HTTP POST.
//!
//! The sender is async (tokio + reqwest) to keep system load low.
//! Collection itself is blocking (nvidia-smi, sysinfo), so it runs
//! in a blocking task on the tokio runtime.

use std::sync::Arc;
use std::time::Duration;

use crate::collector::{Collector, Snapshot};
use crate::config::CollectorConfig;
use crate::control::CommandOp;
use crate::inference::collect_inference;

/// HTTP send timeout for each push. Keeps the agent responsive when the
/// server is slow or unreachable.
const SEND_TIMEOUT: Duration = Duration::from_secs(1);

pub fn run(config: CollectorConfig, collector: Arc<dyn Collector>) -> anyhow::Result<()> {
    let url = config.server_url.clone();
    let interval = Duration::from_secs(config.interval_secs);
    let hostname_override = config.hostname.clone();
    let labels = config.labels.clone();
    let inference_servers = config.inference_servers.clone();
    let interface_role_overrides = config.interface_role_overrides.clone();
    // Derive the hostname used for control-plane identification. Prefer the
    // config override; otherwise use the collector's default hostname.
    let hostname = hostname_override.clone().unwrap_or_else(|| {
        collector
            .collect()
            .map(|s| s.host.hostname)
            .unwrap_or_else(|_| "unknown".to_string())
    });
    tracing::info!(
        "push agent started: server={}, interval={}s, collector={}",
        url,
        interval.as_secs(),
        collector.name(),
    );

    // Build an async reqwest client with a short timeout so a slow or
    // unreachable server does not block the collection loop.
    let client = reqwest::Client::builder()
        .timeout(SEND_TIMEOUT)
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {e}"))?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build tokio runtime: {e}"))?;

    rt.block_on(async move {
        let mut interval_timer = tokio::time::interval(interval);
        loop {
            interval_timer.tick().await;

            // Collection is blocking; run it on a blocking thread.
            let collector = Arc::clone(&collector);
            let hostname_override = hostname_override.clone();
            let labels = labels.clone();
            let mut snap = match tokio::task::spawn_blocking(move || {
                let mut snap = collector.collect()?;
                if let Some(ref h) = hostname_override {
                    snap.host.hostname = h.clone();
                }
                snap.extra.extend(labels.clone());
                Ok::<_, anyhow::Error>(snap)
            })
            .await
            {
                Ok(Ok(snap)) => snap,
                Ok(Err(e)) => {
                    tracing::warn!("collection failed: {e:#}");
                    continue;
                }
                Err(e) => {
                    tracing::warn!("collection task failed: {e}");
                    continue;
                }
            };

            // Apply per-interface role overrides from config.
            for net in &mut snap.host.networks {
                if let Some(role) = interface_role_overrides.get(&net.interface) {
                    net.role = role.clone();
                }
            }

            // Scrape inference metrics asynchronously (does not block the loop).
            let inference_servers = inference_servers.clone();
            snap.inference = collect_inference(&client, &inference_servers).await;

            match push(&client, &url, &snap, &hostname).await {
                Ok(()) => tracing::debug!("pushed snapshot to {}", url),
                Err(e) => tracing::warn!("push to {} failed: {e:#}", url),
            }

            // Poll the control-plane mailbox for a pending command.
            poll_and_execute(&client, &url, &hostname).await;
        }
    });

    Ok(())
}

async fn push(
    client: &reqwest::Client,
    url: &str,
    snap: &Snapshot,
    hostname: &str,
) -> anyhow::Result<()> {
    let resp = client
        .post(url)
        .header("user-agent", format!("dgmon-push/{hostname}"))
        .json(snap)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("send failed: {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("server returned HTTP {}", resp.status());
    }
    Ok(())
}

/// Poll the control-plane mailbox for a pending command, ack it, and
/// execute it locally. Failures are logged but do not stop the loop.
async fn poll_and_execute(client: &reqwest::Client, base_url: &str, hostname: &str) {
    // Derive the control base URL from the ingest URL. The ingest endpoint
    // is `<server>/ingest`; the control routes live under
    // `<server>/api/v1/control/`.
    let control_base = match base_url.strip_suffix("/ingest") {
        Some(prefix) => format!("{prefix}/api/v1/control"),
        None => {
            tracing::warn!("cannot derive control base URL from {base_url}");
            return;
        }
    };

    let poll_url = format!("{control_base}/mailbox");
    let resp = match client
        .get(&poll_url)
        .header("user-agent", format!("dgmon-push/{hostname}"))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("control poll failed: {e:#}");
            return;
        }
    };

    // 204 means no pending command.
    if resp.status() == reqwest::StatusCode::NO_CONTENT {
        return;
    }
    if !resp.status().is_success() {
        tracing::debug!("control poll returned HTTP {}", resp.status());
        return;
    }

    let cmd: crate::control::Command = match resp.json().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("control poll parse failed: {e:#}");
            return;
        }
    };

    // Ack before executing so the mailbox is cleared and a new command can
    // be queued even if execution fails.
    let ack_url = format!("{control_base}/mailbox/ack");
    if let Err(e) = client
        .post(&ack_url)
        .header("user-agent", format!("dgmon-push/{hostname}"))
        .send()
        .await
    {
        tracing::warn!("control ack failed: {e:#}");
    }

    tracing::info!("control: executing {cmd:?} on {hostname}");
    execute_command(cmd.op);
}

/// Execute a control command locally via the shell.
fn execute_command(op: CommandOp) {
    let cmd = op.shell_command();
    tracing::info!("control: running '{cmd}'");
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status();
    match status {
        Ok(s) if s.success() => tracing::info!("control: command succeeded"),
        Ok(s) => tracing::warn!("control: command exited with {s}"),
        Err(e) => tracing::warn!("control: failed to run command: {e}"),
    }
}
