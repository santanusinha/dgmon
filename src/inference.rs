// SPDX-License-Identifier: Apache-2.0
//! Inference server discovery and metric scraping for sglang and vLLM.
//!
//! The push agent scrapes the local inference `/metrics` endpoint and
//! tags the results with the model name from the `/v1/models` API.
//!
//! Discovery order:
//! 1. Docker containers (bollard) — inspect running containers for
//!    sglang/vLLM images and their ports. Supports both bridge and
//!    host networking modes. For host-networking, parses the command
//!    line to find `--port` and `--master-addr` (multi-node vLLM).
//! 2. Manual config targets from the collector config (override).
//! 3. Process table (sysinfo) — look for sglang/vLLM processes and
//!    their listening ports.
//! 4. netstat — shell out to find sglang/vLLM listening ports.
//!
//! If no inference server is found, the collector produces no inference
//! metrics and does not fail the snapshot.

use std::collections::HashMap;
use std::time::Duration;

use crate::collector::InferenceSample;

/// HTTP timeout for scraping inference endpoints.
const SCRAPE_TIMEOUT: Duration = Duration::from_secs(2);

/// A discovered inference server endpoint.
#[derive(Debug, Clone)]
pub struct InferenceTarget {
    /// Base URL, e.g. http://127.0.0.1:8000
    pub base_url: String,
    /// Engine name: sglang or vllm.
    pub engine: String,
}

/// Discover inference servers and scrape their metrics.
/// Returns an empty vector when no server is found.
pub async fn collect_inference(
    client: &reqwest::Client,
    manual_targets: &[String],
) -> Vec<InferenceSample> {
    let mut targets = Vec::new();

    // 1. Docker discovery via bollard (auto-detection, highest priority).
    match discover_docker().await {
        Ok(docker_targets) => targets.extend(docker_targets),
        Err(e) => tracing::debug!("docker inference discovery failed: {e:#}"),
    }

    // 2. Manual config targets (override / fallback).
    for url in manual_targets {
        targets.push(InferenceTarget {
            base_url: url.clone(),
            engine: detect_engine_from_url(url),
        });
    }

    // 3. Process table fallback when Docker found nothing.
    if targets.is_empty() {
        match discover_processes() {
            Ok(proc_targets) => targets.extend(proc_targets),
            Err(e) => tracing::debug!("process inference discovery failed: {e:#}"),
        }
    }

    // 4. netstat fallback.
    if targets.is_empty() {
        match discover_netstat() {
            Ok(net_targets) => targets.extend(net_targets),
            Err(e) => tracing::debug!("netstat inference discovery failed: {e:#}"),
        }
    }

    // Deduplicate targets by base_url.
    let mut seen = std::collections::HashSet::new();
    targets.retain(|t| seen.insert(t.base_url.clone()));

    let mut samples = Vec::new();
    for target in targets {
        match scrape_target(client, &target).await {
            Ok(sample) => samples.push(sample),
            Err(e) => tracing::debug!(
                "scrape {} failed: {e:#}",
                target.base_url
            ),
        }
    }
    samples
}

/// Scrape `/metrics` and `/v1/models` from one target.
async fn scrape_target(
    client: &reqwest::Client,
    target: &InferenceTarget,
) -> anyhow::Result<InferenceSample> {
    let metrics_url = format!("{}/metrics", target.base_url);
    let body = client
        .get(&metrics_url)
        .timeout(SCRAPE_TIMEOUT)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("GET {metrics_url}: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("GET {metrics_url}: {e}"))?
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("read {metrics_url}: {e}"))?;

    let metrics = parse_prometheus_text(&body);

    // Detect the engine from the scraped metric content. vLLM exposes
    // metrics prefixed with `vllm:`, sglang with `sglang:`. Fall back to
    // the URL-based detection when the content is ambiguous.
    let engine = detect_engine_from_content(&body, &target.engine);

    // Get the model name from the OpenAI-compatible /v1/models endpoint.
    let model_name = fetch_model_name(client, &target.base_url).await;

    Ok(InferenceSample {
        engine,
        model_name,
        metrics,
    })
}

async fn fetch_model_name(client: &reqwest::Client, base_url: &str) -> String {
    let url = format!("{base_url}/v1/models");
    match client
        .get(&url)
        .timeout(SCRAPE_TIMEOUT)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<serde_json::Value>().await {
                Ok(v) => v
                    .get("data")
                    .and_then(|d| d.as_array())
                    .and_then(|a| a.first())
                    .and_then(|m| m.get("id"))
                    .and_then(|id| id.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                Err(_) => "unknown".into(),
            }
        }
        _ => "unknown".into(),
    }
}

/// Discover inference servers via Docker (bollard).
async fn discover_docker() -> anyhow::Result<Vec<InferenceTarget>> {
    use bollard::container::ListContainersOptions;
    use bollard::Docker;

    let docker = Docker::connect_with_local_defaults()?;
    let opts = ListContainersOptions::<String> {
        all: false,
        limit: None,
        size: false,
        filters: HashMap::new(),
    };

    let containers = docker.list_containers(Some(opts)).await?;
    let mut targets = Vec::new();
    for c in containers {
        let name = c
            .names
            .as_ref()
            .and_then(|n| n.first())
            .cloned()
            .unwrap_or_default();
        let image = c.image.clone().unwrap_or_default();
        let lower = format!("{name} {image}").to_lowercase();

        // Only consider containers that look like inference servers.
        if !(lower.contains("sglang") || lower.contains("vllm")) {
            continue;
        }

        // Determine the engine from the image/name.
        let engine = if lower.contains("vllm") {
            "vllm"
        } else {
            "sglang"
        };

        // Inspect the container for port bindings and network mode.
        let id = c.id.clone().unwrap_or_default();
        let inspect = docker.inspect_container(&id, None).await?;

        // Try published ports first (bridge networking).
        if let Some(port) = extract_published_port(&inspect) {
            targets.push(InferenceTarget {
                base_url: format!("http://127.0.0.1:{port}"),
                engine: engine.to_string(),
            });
            continue;
        }

        // For host-networking containers, parse the command line to find
        // the API port and the head node address (multi-node vLLM).
        let net_mode = inspect
            .host_config
            .as_ref()
            .and_then(|h| h.network_mode.as_deref());
        if net_mode == Some("host") {
            let port = extract_port_from_command(&inspect).unwrap_or(8000);
            let host = extract_master_addr(&inspect).unwrap_or_else(|| "127.0.0.1".into());
            targets.push(InferenceTarget {
                base_url: format!("http://{host}:{port}"),
                engine: engine.to_string(),
            });
        } else {
            tracing::debug!(
                "inference container {name} has no discoverable port; skipping"
            );
        }
    }
    Ok(targets)
}

/// Extract the first published TCP port from a container inspect response.
fn extract_published_port(inspect: &bollard::models::ContainerInspectResponse) -> Option<u16> {
    let ports = inspect.network_settings.as_ref()?.ports.as_ref()?;
    for bindings in ports.values().flatten() {
        for b in bindings {
            if let Some(p) = b.host_port.as_ref().and_then(|h| h.parse::<u16>().ok()) {
                return Some(p);
            }
        }
    }
    None
}

/// Extract the API port (`--port`) from the container command line.
/// Handles both direct array args and bash -c wrapped commands.
/// Falls back to the vLLM/sglang default port 8000.
fn extract_port_from_command(inspect: &bollard::models::ContainerInspectResponse) -> Option<u16> {
    let cmd = inspect.config.as_ref()?.cmd.as_ref()?;

    // Try direct array lookup first.
    let mut i = 0;
    while i < cmd.len() {
        if cmd[i] == "--port"
            && let Some(p) = cmd.get(i + 1).and_then(|s| s.parse::<u16>().ok()) {
                return Some(p);
            }
        i += 1;
    }

    // If the command is bash -c "...", search the joined string.
    let joined = cmd.join(" ");
    if let Some(pos) = joined.find("--port ") {
        let after = &joined[pos + 7..];
        if let Some(port_str) = after.split_whitespace().next()
            && let Ok(p) = port_str.parse::<u16>() {
                return Some(p);
            }
    }

    Some(8000)
}

/// Extract the head node address (`--master-addr`) from the container command
/// line. Handles both direct array args and bash -c wrapped commands.
/// This is used in multi-node vLLM deployments. Returns None for
/// single-node setups, in which case 127.0.0.1 is the correct address.
fn extract_master_addr(inspect: &bollard::models::ContainerInspectResponse) -> Option<String> {
    let cmd = inspect.config.as_ref()?.cmd.as_ref()?;

    // Try direct array lookup first.
    let mut i = 0;
    while i < cmd.len() {
        if cmd[i] == "--master-addr" {
            return cmd.get(i + 1).cloned();
        }
        i += 1;
    }

    // If the command is bash -c "...", search the joined string.
    let joined = cmd.join(" ");
    if let Some(pos) = joined.find("--master-addr ") {
        let after = &joined[pos + 14..];
        if let Some(addr) = after.split_whitespace().next() {
            return Some(addr.to_string());
        }
    }

    None
}

/// Discover inference servers from the process table (sysinfo).
fn discover_processes() -> anyhow::Result<Vec<InferenceTarget>> {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let mut targets = Vec::new();
    for proc in sys.processes().values() {
        let name = proc.name().to_string_lossy().to_lowercase();
        let cmd = proc
            .cmd()
            .iter()
            .map(|c| c.to_string_lossy().to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        if !(name.contains("sglang")
            || name.contains("vllm")
            || cmd.contains("sglang")
            || cmd.contains("vllm"))
        {
            continue;
        }
        let engine = if name.contains("vllm") || cmd.contains("vllm") {
            "vllm"
        } else {
            "sglang"
        };
        // Try to find a listening port from the process's open ports.
        if let Some(port) = find_listening_port(proc.pid()) {
            targets.push(InferenceTarget {
                base_url: format!("http://127.0.0.1:{port}"),
                engine: engine.to_string(),
            });
        }
    }
    Ok(targets)
}

/// Find a listening TCP port for a process by reading /proc/<pid>/net/tcp.
fn find_listening_port(pid: sysinfo::Pid) -> Option<u16> {
    let path = format!("/proc/{}/net/tcp", pid.as_u32());
    let content = std::fs::read_to_string(&path).ok()?;
    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        // State 0A = LISTEN.
        if fields[3] != "0A" {
            continue;
        }
        // Local address is hex IP:hex port.
        let local = fields[1];
        let port_hex = local.rsplit(':').next()?;
        if let Ok(port) = u16::from_str_radix(port_hex, 16) {
            return Some(port);
        }
    }
    None
}

/// Discover inference servers via netstat (shell out).
fn discover_netstat() -> anyhow::Result<Vec<InferenceTarget>> {
    let out = std::process::Command::new("netstat")
        .args(["-tlnp"])
        .output()?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut targets = Vec::new();
    for line in stdout.lines().skip(2) {
        let lower = line.to_lowercase();
        if !(lower.contains("sglang") || lower.contains("vllm")) {
            continue;
        }
        // Parse the local address:port.
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        let local = fields[3];
        let port = local.rsplit(':').next().and_then(|p| p.parse::<u16>().ok());
        if let Some(port) = port {
            let engine = if lower.contains("vllm") { "vllm" } else { "sglang" };
            targets.push(InferenceTarget {
                base_url: format!("http://127.0.0.1:{port}"),
                engine: engine.to_string(),
            });
        }
    }
    Ok(targets)
}

/// Detect the engine from a manual config URL.
fn detect_engine_from_url(url: &str) -> String {
    let lower = url.to_lowercase();
    if lower.contains("vllm") {
        "vllm".into()
    } else {
        "sglang".into()
    }
}

/// Detect the engine from the scraped `/metrics` content.
///
/// vLLM exposes metrics prefixed with `vllm:`, sglang with `sglang:`.
/// When neither prefix is present, fall back to the URL-based detection.
fn detect_engine_from_content(body: &str, fallback: &str) -> String {
    let lower = body.to_lowercase();
    if lower.contains("vllm:") {
        "vllm".into()
    } else if lower.contains("sglang:") {
        "sglang".into()
    } else {
        fallback.to_string()
    }
}

/// Parse Prometheus text exposition format into (name, value) pairs.
/// Handles `# TYPE`, `# HELP`, and `# EOF` lines. Only captures the
/// last sample for each metric name (ignoring label sets for simplicity).
fn parse_prometheus_text(body: &str) -> Vec<(String, f64)> {
    let mut metrics: HashMap<String, f64> = HashMap::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Split on the last whitespace to separate value from the rest.
        let Some((name_part, value_part)) = line.rsplit_once(char::is_whitespace) else {
            continue;
        };
        let Ok(value) = value_part.parse::<f64>() else {
            continue;
        };
        // Strip any {labels} from the name part.
        let name = name_part
            .split('{')
            .next()
            .unwrap_or(name_part)
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        metrics.insert(name, value);
    }
    metrics.into_iter().collect()
}
