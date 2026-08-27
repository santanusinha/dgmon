// SPDX-License-Identifier: Apache-2.0
//! Control plane: agent-based command mailbox.
//!
//! The server holds a mailbox: a concurrency-safe map keyed by hostname.
//! Each entry holds one pending command (or none). The push agent on each
//! node polls the mailbox, acks a pending command, then executes it locally.
//!
//! Routes (registered under `/api/v1/control/`):
//!   POST /nodes/{hostname}/restart   → queue a restart (201 / 409)
//!   POST /nodes/{hostname}/shutdown  → queue a shutdown (201 / 409)
//!   GET  /mailbox                    → poll for a command (200 / 204)
//!   POST /mailbox/ack                → ack + clear a command (200 / 404)
//!   GET  /nodes                      → list nodes + pending commands

use std::collections::HashMap;
use std::sync::RwLock;

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::api::SnapshotSource;

/// The operation a node should perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandOp {
    Restart,
    Shutdown,
}

impl CommandOp {
    /// The shell command executed on the node for this op.
    pub fn shell_command(&self) -> &'static str {
        match self {
            CommandOp::Restart => "sudo shutdown -r now",
            CommandOp::Shutdown => "sudo shutdown -h now",
        }
    }
}

/// A pending command for one node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub op: CommandOp,
    pub issued_at: chrono::DateTime<chrono::Utc>,
}

/// Concurrency-safe mailbox of pending commands keyed by hostname.
pub struct Mailbox {
    pending: RwLock<HashMap<String, Command>>,
}

impl Mailbox {
    pub fn new() -> Self {
        Self {
            pending: RwLock::new(HashMap::new()),
        }
    }

    /// Queue a command for a hostname.
    /// Returns `Ok(true)` if queued, `Ok(false)` if a command is already pending.
    pub fn post(&self, hostname: &str, op: CommandOp) -> bool {
        let mut map = self.pending.write().unwrap_or_else(|e| e.into_inner());
        if map.contains_key(hostname) {
            return false;
        }
        map.insert(
            hostname.to_string(),
            Command {
                op,
                issued_at: chrono::Utc::now(),
            },
        );
        true
    }

    /// Get the pending command for a hostname, if any.
    pub fn poll(&self, hostname: &str) -> Option<Command> {
        self.pending
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(hostname)
            .cloned()
    }

    /// Clear the pending command for a hostname.
    /// Returns `Ok(true)` if cleared, `Ok(false)` if no command was pending.
    pub fn ack(&self, hostname: &str) -> bool {
        self.pending
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(hostname)
            .is_some()
    }

    /// List all pending commands (hostname → command).
    pub fn all(&self) -> Vec<(String, Command)> {
        self.pending
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(h, c)| (h.clone(), c.clone()))
            .collect()
    }
}

impl Default for Mailbox {
    fn default() -> Self {
        Self::new()
    }
}

/// Control-plane configuration from the server config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlConfig {
    /// Master switch. Control plane is off by default.
    #[serde(default)]
    pub enabled: bool,
}

/// Shared state for control-plane handlers.
pub struct ControlState {
    pub mailbox: Mailbox,
    pub snapshots: std::sync::Arc<dyn SnapshotSource + Send + Sync>,
}

/// Parse the hostname from a `dgmon-push/<hostname>` User-Agent header.
fn hostname_from_user_agent(req: &HttpRequest) -> Option<String> {
    let ua = req.headers().get("user-agent")?.to_str().ok()?;
    let rest = ua.strip_prefix("dgmon-push/")?;
    let host = rest.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// JSON error response body.
#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

fn error_response(status: actix_web::http::StatusCode, code: &str, message: &str) -> HttpResponse {
    HttpResponse::build(status)
        .content_type("application/json")
        .body(
            serde_json::to_string(&ErrorBody {
                error: ErrorDetail {
                    code: code.into(),
                    message: message.into(),
                },
            })
            .unwrap_or_default(),
        )
}

/// POST /api/v1/control/nodes/{hostname}/restart — queue a restart.
pub async fn restart(
    state: web::Data<ControlState>,
    path: web::Path<String>,
) -> impl Responder {
    post_command(&state, &path.into_inner(), CommandOp::Restart)
}

/// POST /api/v1/control/nodes/{hostname}/shutdown — queue a shutdown.
pub async fn shutdown(
    state: web::Data<ControlState>,
    path: web::Path<String>,
) -> impl Responder {
    post_command(&state, &path.into_inner(), CommandOp::Shutdown)
}

fn post_command(state: &ControlState, hostname: &str, op: CommandOp) -> HttpResponse {
    // The hostname must be a known node.
    let known = state
        .snapshots
        .all()
        .iter()
        .any(|s| s.host.hostname == hostname);
    if !known {
        return error_response(
            actix_web::http::StatusCode::NOT_FOUND,
            "node_not_found",
            &format!("no node named '{hostname}'"),
        );
    }

    if state.mailbox.post(hostname, op) {
        tracing::info!("control: queued {op:?} for {hostname}");
        HttpResponse::Created().content_type("application/json").body(
            serde_json::json!({
                "status": "queued",
                "hostname": hostname,
                "op": op,
            })
            .to_string(),
        )
    } else {
        error_response(
            actix_web::http::StatusCode::CONFLICT,
            "command_pending",
            &format!("a command is already pending for '{hostname}'"),
        )
    }
}

/// GET /api/v1/control/mailbox — poll for a pending command (agent).
pub async fn poll(req: HttpRequest, state: web::Data<ControlState>) -> impl Responder {
    let Some(hostname) = hostname_from_user_agent(&req) else {
        return error_response(
            actix_web::http::StatusCode::BAD_REQUEST,
            "bad_user_agent",
            "User-Agent must be 'dgmon-push/<hostname>'",
        );
    };

    match state.mailbox.poll(&hostname) {
        Some(cmd) => HttpResponse::Ok()
            .content_type("application/json")
            .body(serde_json::to_string(&cmd).unwrap_or_default()),
        None => HttpResponse::NoContent().finish(),
    }
}

/// POST /api/v1/control/mailbox/ack — ack + clear a pending command (agent).
pub async fn ack(req: HttpRequest, state: web::Data<ControlState>) -> impl Responder {
    let Some(hostname) = hostname_from_user_agent(&req) else {
        return error_response(
            actix_web::http::StatusCode::BAD_REQUEST,
            "bad_user_agent",
            "User-Agent must be 'dgmon-push/<hostname>'",
        );
    };

    if state.mailbox.ack(&hostname) {
        tracing::info!("control: acked command for {hostname}");
        HttpResponse::Ok()
            .content_type("application/json")
            .body(
                serde_json::json!({
                    "status": "acked",
                    "hostname": hostname,
                })
                .to_string(),
            )
    } else {
        error_response(
            actix_web::http::StatusCode::NOT_FOUND,
            "no_command",
            &format!("no pending command for '{hostname}'"),
        )
    }
}

/// GET /api/v1/control/nodes — list nodes with pending commands.
pub async fn nodes(state: web::Data<ControlState>) -> impl Responder {
    let snaps = state.snapshots.all();
    let pending = state.mailbox.all();
    let pending_map: HashMap<String, Command> = pending.into_iter().collect();

    let list: Vec<serde_json::Value> = snaps
        .iter()
        .map(|s| {
            let hostname = &s.host.hostname;
            serde_json::json!({
                "hostname": hostname,
                "gpus": s.gpus.len(),
                "timestamp": s.timestamp,
                "pending_command": pending_map.get(hostname),
            })
        })
        .collect();

    HttpResponse::Ok()
        .content_type("application/json")
        .body(serde_json::to_string_pretty(&list).unwrap_or_default())
}

/// Register the control-plane routes.
///
/// These routes are registered inside the `/api/v1` scope by `api::configure`.
/// They must NOT create their own scope, because actix-web matches the first
/// scope registered for a prefix and never falls through to a second scope
/// with the same prefix.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/control")
            .route("/nodes", web::get().to(nodes))
            .route("/nodes/{hostname}/restart", web::post().to(restart))
            .route("/nodes/{hostname}/shutdown", web::post().to(shutdown))
            .route("/mailbox", web::get().to(poll))
            .route("/mailbox/ack", web::post().to(ack)),
    );
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_post_poll_ack() {
        let mb = Mailbox::new();
        assert!(mb.post("node1", CommandOp::Restart));
        assert!(!mb.post("node1", CommandOp::Shutdown)); // conflict
        let cmd = mb.poll("node1").expect("command should be pending");
        assert_eq!(cmd.op, CommandOp::Restart);
        assert!(mb.ack("node1"));
        assert!(!mb.ack("node1")); // already cleared
        assert!(mb.poll("node1").is_none());
    }

    #[test]
    fn mailbox_multiple_hosts() {
        let mb = Mailbox::new();
        assert!(mb.post("a", CommandOp::Restart));
        assert!(mb.post("b", CommandOp::Shutdown));
        assert_eq!(mb.poll("a").unwrap().op, CommandOp::Restart);
        assert_eq!(mb.poll("b").unwrap().op, CommandOp::Shutdown);
        assert_eq!(mb.all().len(), 2);
    }

    #[test]
    fn command_op_shell_command() {
        assert_eq!(CommandOp::Restart.shell_command(), "sudo shutdown -r now");
        assert_eq!(CommandOp::Shutdown.shell_command(), "sudo shutdown -h now");
    }

    #[test]
    fn hostname_from_user_agent_parses() {
        let req = actix_web::test::TestRequest::default()
            .insert_header(("user-agent", "dgmon-push/spark-01"))
            .to_http_request();
        assert_eq!(hostname_from_user_agent(&req).as_deref(), Some("spark-01"));
    }

    #[test]
    fn hostname_from_user_agent_rejects_bad() {
        let req = actix_web::test::TestRequest::default()
            .insert_header(("user-agent", "curl/8.0"))
            .to_http_request();
        assert!(hostname_from_user_agent(&req).is_none());

        let req = actix_web::test::TestRequest::default()
            .insert_header(("user-agent", "dgmon-push/"))
            .to_http_request();
        assert!(hostname_from_user_agent(&req).is_none());
    }
}
