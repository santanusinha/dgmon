// SPDX-License-Identifier: Apache-2.0
//! Shared actix-web HTTP handlers used by both server and service modes.
//!
//! This module contains the route handlers for the dashboard, static
//! assets, health check, and history.
//! The server and service modules provide their own data sources and
//! register these handlers on their actix-web `App`.

use std::sync::Arc;

use actix_web::{web, HttpResponse, Responder};
use serde::Serialize;

use crate::collector::Snapshot;
use crate::metric_name::sanitize_metric_name;
use crate::storage::TsinkStore;

/// JSON response for a historical query.
#[derive(Serialize)]
pub struct HistoryResponse {
    pub metric: String,
    pub hostname: String,
    pub points: Vec<HistoryPoint>,
}

#[derive(Serialize)]
pub struct HistoryPoint {
    pub timestamp: i64,
    pub value: f64,
}


/// Shared application state for actix-web handlers.
pub struct AppState {
    pub tsink: Option<Arc<TsinkStore>>,
}

/// GET / — HTML landing page with endpoint links.
pub async fn index() -> impl Responder {
    let body = r#"<!DOCTYPE html>
<html><head><meta charset='utf-8'><title>dgmon</title>
<meta http-equiv='refresh' content='5'>
<style>body{font-family:monospace;margin:2em}table{border-collapse:collapse}td,th{border:1px solid #999;padding:4px 8px}</style>
</head><body><h1>dgmon</h1>
    <p>Endpoints: <a href='/dashboard'>/dashboard</a> | <a href='/nodes'>/nodes</a> | <a href='/metrics'>/metrics</a> | <a href='/history'>/history</a> | <a href='/health'>/health</a></p>
</body></html>"#;
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(body)
}

/// GET /dashboard — embedded HTML dashboard.
pub async fn dashboard() -> impl Responder {
    let body = include_str!("../dashboard/index.html");
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(body)
}

/// GET /static/style.css
pub async fn style_css() -> impl Responder {
    let body = include_str!("../dashboard/style.css");
    HttpResponse::Ok()
        .content_type("text/css; charset=utf-8")
        .body(body)
}

/// GET /static/app.js
pub async fn app_js() -> impl Responder {
    let body = include_str!("../dashboard/app.js");
    HttpResponse::Ok()
        .content_type("application/javascript; charset=utf-8")
        .body(body)
}

/// GET /static/chart.umd.min.js
pub async fn chart_js() -> impl Responder {
    let body = include_str!("../dashboard/static/chart.umd.min.js");
    HttpResponse::Ok()
        .content_type("application/javascript; charset=utf-8")
        .body(body)
}

/// GET /health — liveness probe.
pub async fn health() -> impl Responder {
    HttpResponse::Ok().body("ok\n")
}

/// Render snapshots in Prometheus text exposition format.
/// Shared by the server (all nodes) and the service (single node).
pub fn render_prometheus(snaps: &[Snapshot]) -> String {
    let mut out = String::new();

    for snap in snaps {
        let host = escape_label_value(&snap.host.hostname);

        // Extra metadata labels merged into every metric.
        let extra_labels: String = snap
            .extra
            .iter()
            .map(|(k, v)| format!(",{k}=\"{}\"", escape_label_value(v)))
            .collect();

        out.push_str("# HELP dgmon_cpu_usage_pct CPU usage percentage\n");
        out.push_str(&format!(
            "dgmon_cpu_usage_pct{{hostname=\"{}\"{extra_labels}}} {:.1}\n",
            host, snap.host.cpu_usage_pct
        ));

        out.push_str("# HELP dgmon_memory_used_mb Memory used in MiB\n");
        out.push_str(&format!(
            "dgmon_memory_used_mb{{hostname=\"{}\"{extra_labels}}} {}\n",
            host, snap.host.memory_used_mb
        ));

        out.push_str("# HELP dgmon_memory_total_mb Total memory in MiB\n");
        out.push_str(&format!(
            "dgmon_memory_total_mb{{hostname=\"{}\"{extra_labels}}} {}\n",
            host, snap.host.memory_total_mb
        ));

        out.push_str("# HELP dgmon_uptime_seconds Host uptime in seconds\n");
        out.push_str(&format!(
            "dgmon_uptime_seconds{{hostname=\"{}\"{extra_labels}}} {}\n",
            host, snap.host.uptime_seconds
        ));

        out.push_str("# HELP dgmon_disk_used_gb Disk space used in GB\n");
        out.push_str(&format!(
            "dgmon_disk_used_gb{{hostname=\"{}\"{extra_labels}}} {:.2}\n",
            host, snap.host.disk_used_gb
        ));

        out.push_str("# HELP dgmon_disk_total_gb Total disk space in GB\n");
        out.push_str(&format!(
            "dgmon_disk_total_gb{{hostname=\"{}\"{extra_labels}}} {:.2}\n",
            host, snap.host.disk_total_gb
        ));

        out.push_str("# HELP dgmon_network_rx_bytes Network bytes received\n");
        out.push_str(&format!(
            "dgmon_network_rx_bytes{{hostname=\"{}\"{extra_labels}}} {}\n",
            host, snap.host.network_rx_bytes
        ));

        out.push_str("# HELP dgmon_network_tx_bytes Network bytes transmitted\n");
        out.push_str(&format!(
            "dgmon_network_tx_bytes{{hostname=\"{}\"{extra_labels}}} {}\n",
            host, snap.host.network_tx_bytes
        ));

        // Per-CPU-core utilization.
        for c in &snap.host.cpu_cores {
            let core = escape_label_value(&c.index.to_string());
            out.push_str("# HELP dgmon_cpu_core_usage_pct Per-CPU-core utilization percentage\n");
            out.push_str(&format!(
                "dgmon_cpu_core_usage_pct{{hostname=\"{}\",core=\"{}\"{extra_labels}}} {:.1}\n",
                host, core, c.usage_pct
            ));
            if let Some(freq) = c.freq_mhz {
                out.push_str("# HELP dgmon_cpu_core_freq_mhz Per-CPU-core frequency in MHz\n");
                out.push_str(&format!(
                    "dgmon_cpu_core_freq_mhz{{hostname=\"{}\",core=\"{}\"{extra_labels}}} {freq}\n",
                    host, core
                ));
            }
            if let Some(gov) = &c.governor {
                out.push_str("# HELP dgmon_cpu_core_governor CPU scaling governor\n");
                out.push_str(&format!(
                    "dgmon_cpu_core_governor{{hostname=\"{}\",core=\"{}\"{extra_labels}}} {gov}\n",
                    host, core
                ));
            }
            if let Some(max_freq) = c.max_freq_mhz {
                out.push_str("# HELP dgmon_cpu_core_max_freq_mhz Per-CPU-core max frequency in MHz\n");
                out.push_str(&format!(
                    "dgmon_cpu_core_max_freq_mhz{{hostname=\"{}\",core=\"{}\"{extra_labels}}} {max_freq}\n",
                    host, core
                ));
            }
        }

        // Per-interface network utilization.
        for net in &snap.host.networks {
            let net_labels = format!(
                "hostname=\"{}\",interface=\"{}\",role=\"{}\"{extra_labels}",
                host,
                escape_label_value(&net.interface),
                escape_label_value(&net.role)
            );
            out.push_str("# HELP dgmon_net_rx_bytes Per-interface bytes received\n");
            out.push_str(&format!("dgmon_net_rx_bytes{{{net_labels}}} {}\n", net.rx_bytes));
            out.push_str("# HELP dgmon_net_tx_bytes Per-interface bytes transmitted\n");
            out.push_str(&format!("dgmon_net_tx_bytes{{{net_labels}}} {}\n", net.tx_bytes));
            out.push_str("# HELP dgmon_net_rx_packets Per-interface packets received\n");
            out.push_str(&format!("dgmon_net_rx_packets{{{net_labels}}} {}\n", net.rx_packets));
            out.push_str("# HELP dgmon_net_tx_packets Per-interface packets transmitted\n");
            out.push_str(&format!("dgmon_net_tx_packets{{{net_labels}}} {}\n", net.tx_packets));
            if let Some(speed) = net.speed_mbps {
                out.push_str("# HELP dgmon_net_speed_mbps Per-interface link speed in Mbps\n");
                out.push_str(&format!("dgmon_net_speed_mbps{{{net_labels}}} {speed}\n"));
            }
            out.push_str("# HELP dgmon_net_up Per-interface link up state\n");
            out.push_str(&format!(
                "dgmon_net_up{{{net_labels}}} {}\n",
                if net.up { 1 } else { 0 }
            ));
        }

        for g in &snap.gpus {
            let labels = format!(
                "hostname=\"{}\",gpu=\"{}\",uuid=\"{}\",model=\"{}\"{extra_labels}",
                host,
                escape_label_value(&g.index.to_string()),
                escape_label_value(&g.uuid),
                escape_label_value(&g.name)
            );

            out.push_str("# HELP dgmon_gpu_utilization GPU utilization percentage\n");
            out.push_str(&format!("dgmon_gpu_utilization{{{labels}}} {}\n", g.utilization_gpu));

            out.push_str("# HELP dgmon_gpu_mem_utilization GPU memory utilization percentage\n");
            out.push_str(&format!(
                "dgmon_gpu_mem_utilization{{{labels}}} {}\n",
                g.utilization_memory
            ));

            out.push_str("# HELP dgmon_gpu_temp_c GPU temperature in Celsius\n");
            out.push_str(&format!("dgmon_gpu_temp_c{{{labels}}} {}\n", g.temperature_c));

            if let Some(p) = g.power_w {
                out.push_str("# HELP dgmon_gpu_power_w GPU power draw in watts\n");
                out.push_str(&format!("dgmon_gpu_power_w{{{labels}}} {:.1}\n", p));
            }

            if let Some(p) = g.power_limit_w {
                out.push_str("# HELP dgmon_gpu_power_limit_w GPU power limit in watts\n");
                out.push_str(&format!("dgmon_gpu_power_limit_w{{{labels}}} {:.1}\n", p));
            }

            if let Some(m) = g.memory_used_mb {
                out.push_str("# HELP dgmon_gpu_memory_used_mb GPU memory used in MiB\n");
                out.push_str(&format!("dgmon_gpu_memory_used_mb{{{labels}}} {}\n", m));
            }

            if let Some(m) = g.memory_total_mb {
                out.push_str("# HELP dgmon_gpu_memory_total_mb GPU memory total in MiB\n");
                out.push_str(&format!("dgmon_gpu_memory_total_mb{{{labels}}} {}\n", m));
            }

            if let Some(fan) = g.fan_speed_pct {
                out.push_str("# HELP dgmon_gpu_fan_speed_pct GPU fan speed percentage\n");
                out.push_str(&format!("dgmon_gpu_fan_speed_pct{{{labels}}} {fan}\n"));
            }

            // New granular GPU metrics.
            if let Some(v) = g.sm_clock_mhz {
                out.push_str("# HELP dgmon_gpu_sm_clock_mhz GPU SM clock in MHz\n");
                out.push_str(&format!("dgmon_gpu_sm_clock_mhz{{{labels}}} {v}\n"));
            }
            if let Some(v) = g.mem_clock_mhz {
                out.push_str("# HELP dgmon_gpu_mem_clock_mhz GPU memory clock in MHz\n");
                out.push_str(&format!("dgmon_gpu_mem_clock_mhz{{{labels}}} {v}\n"));
            }
            if let Some(v) = g.sm_clock_max_mhz {
                out.push_str("# HELP dgmon_gpu_sm_clock_max_mhz GPU max SM clock in MHz\n");
                out.push_str(&format!("dgmon_gpu_sm_clock_max_mhz{{{labels}}} {v}\n"));
            }
            if let Some(v) = g.mem_clock_max_mhz {
                out.push_str("# HELP dgmon_gpu_mem_clock_max_mhz GPU max memory clock in MHz\n");
                out.push_str(&format!("dgmon_gpu_mem_clock_max_mhz{{{labels}}} {v}\n"));
            }
            if let Some(v) = g.mem_temp_c {
                out.push_str("# HELP dgmon_gpu_mem_temp_c GPU memory temperature in Celsius\n");
                out.push_str(&format!("dgmon_gpu_mem_temp_c{{{labels}}} {v}\n"));
            }
            if let Some(v) = g.pcie_link_gen {
                out.push_str("# HELP dgmon_gpu_pcie_link_gen Current PCIe link generation\n");
                out.push_str(&format!("dgmon_gpu_pcie_link_gen{{{labels}}} {v}\n"));
            }
            if let Some(v) = g.pcie_link_gen_max {
                out.push_str("# HELP dgmon_gpu_pcie_link_gen_max Max PCIe link generation\n");
                out.push_str(&format!("dgmon_gpu_pcie_link_gen_max{{{labels}}} {v}\n"));
            }
            if let Some(v) = g.pcie_link_width {
                out.push_str("# HELP dgmon_gpu_pcie_link_width Current PCIe link width in lanes\n");
                out.push_str(&format!("dgmon_gpu_pcie_link_width{{{labels}}} {v}\n"));
            }
            if let Some(v) = g.pcie_link_width_max {
                out.push_str("# HELP dgmon_gpu_pcie_link_width_max Max PCIe link width in lanes\n");
                out.push_str(&format!("dgmon_gpu_pcie_link_width_max{{{labels}}} {v}\n"));
            }

            // GPU clock throttle reasons.
            out.push_str("# HELP dgmon_gpu_throttle_active GPU clock throttle reasons bitmask\n");
            out.push_str(&format!("dgmon_gpu_throttle_active{{{labels}}} {}\n", g.throttle_active));
            out.push_str("# HELP dgmon_gpu_throttle_hw_thermal HW thermal slowdown active\n");
            out.push_str(&format!("dgmon_gpu_throttle_hw_thermal{{{labels}}} {}\n", g.throttle_hw_thermal as u8));
            out.push_str("# HELP dgmon_gpu_throttle_sw_thermal SW thermal slowdown active\n");
            out.push_str(&format!("dgmon_gpu_throttle_sw_thermal{{{labels}}} {}\n", g.throttle_sw_thermal as u8));
            out.push_str("# HELP dgmon_gpu_throttle_hw_slowdown HW slowdown active\n");
            out.push_str(&format!("dgmon_gpu_throttle_hw_slowdown{{{labels}}} {}\n", g.throttle_hw_slowdown as u8));
            out.push_str("# HELP dgmon_gpu_throttle_power_brake HW power brake slowdown active\n");
            out.push_str(&format!("dgmon_gpu_throttle_power_brake{{{labels}}} {}\n", g.throttle_power_brake as u8));
        }

        // Inference metrics.
        for inf in &snap.inference {
            let inf_labels = format!(
                "hostname=\"{}\",engine=\"{}\",model_name=\"{}\"{extra_labels}",
                host,
                escape_label_value(&inf.engine),
                escape_label_value(&inf.model_name)
            );
            for (name, value) in &inf.metrics {
                let metric = sanitize_metric_name(name);
                out.push_str(&format!("{metric}{{{inf_labels}}} {value}\n"));
            }
        }
    }

    out
}

/// Escape a Prometheus label value so it is safe inside a quoted label.
/// Replaces backslash, double quote, and newline characters.
fn escape_label_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_label_value_escapes_special_chars() {
        assert_eq!(escape_label_value("bad\"host\\name"), "bad\\\"host\\\\name");
        assert_eq!(escape_label_value("line\nbreak"), "line\\nbreak");
        assert_eq!(escape_label_value("plain"), "plain");
    }
}

/// GET /history?metric=<name>&hostname=<host>&start=<ms>&end=<ms>
pub async fn history(
    state: web::Data<AppState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let Some(ref ts) = state.tsink else {
        return HttpResponse::ServiceUnavailable()
            .content_type("application/json")
            .body(r#"{"error":"history requires time-series storage; start with --data-dir <path> or set DGMON_DATA_DIR"}"#);
    };

    let metric = query.get("metric").cloned().unwrap_or_default();
    let hostname = query.get("hostname").cloned().unwrap_or_default();
    let start_ms: i64 = query
        .get("start")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let end_ms: i64 = query
        .get("end")
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

    if metric.is_empty() || hostname.is_empty() {
        return HttpResponse::BadRequest()
            .content_type("application/json")
            .body(r#"{"error":"missing 'metric' or 'hostname' query parameter"}"#);
    }

    match ts.query(&metric, &hostname, start_ms, end_ms) {
        Ok(points) => {
            let resp = HistoryResponse {
                metric,
                hostname,
                points: points
                    .iter()
                    .map(|(ts, val)| HistoryPoint {
                        timestamp: *ts,
                        value: *val,
                    })
                    .collect(),
            };
            HttpResponse::Ok()
                .content_type("application/json")
                .body(serde_json::to_string_pretty(&resp).unwrap_or_default())
        }
        Err(e) => HttpResponse::InternalServerError()
            .content_type("application/json")
            .body(format!("{{\"error\":\"{e}\"}}")),
    }
}

