use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server};
use tracing::{error, info};
use url::Url;

use crate::application::ports::{SessionRepository, WorkflowRepository};
use crate::domain::workflow::{
    CommitmentContract, DriftResolution, RuleEffect, RuleId, RuleMatcher, RuleScope, Workflow,
    WorkflowId, WorkflowRule,
};
use crate::infrastructure::persistence::sqlite::SqliteDb;
use crate::infrastructure::persistence::{SqliteSessionRepository, SqliteWorkflowRepository};

use super::state::{
    CreateWorkflowRequest, DriftEventDto, GuiState, RunningAppDto, SessionHistoryDto,
    StartSessionRequest, StopSessionRequest, WorkflowDto,
};

const INDEX_HTML: &str = include_str!("static/index.html");
const STYLE_CSS: &str = include_str!("static/style.css");
const APP_JS: &str = include_str!("static/app.js");

/// Maximum allowable request body size (64 KB) to prevent DoS via unbounded payloads (H-14).
const MAX_REQUEST_BODY_BYTES: u64 = 64 * 1024;

pub struct GuiServerContext {
    pub gui_state: Arc<Mutex<GuiState>>,
    pub db: SqliteDb,
    pub running: Arc<AtomicBool>,
    pub command_tx: std::sync::mpsc::Sender<super::engine::GuiCommand>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum OriginCheck {
    NoOrigin,
    Trusted(String),
    Untrusted,
}

/// Validates that the request Host header matches loopback (127.0.0.1 or localhost with optional port).
/// Protects against DNS rebinding attacks (C-01).
pub fn validate_host(request: &Request) -> bool {
    let host_header = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Host") || h.field.as_str().as_str().eq_ignore_ascii_case("host"));

    let host_val = match host_header {
        Some(h) => h.value.as_str().trim(),
        None => return false,
    };

    if host_val.is_empty() {
        return false;
    }

    let (hostname, port_str) = match host_val.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (host_val, None),
    };

    let host_lower = hostname.to_lowercase();
    if host_lower != "127.0.0.1" && host_lower != "localhost" {
        return false;
    }

    if let Some(port) = port_str {
        if port.parse::<u16>().is_err() {
            return false;
        }
    }

    true
}

/// Checks the Origin header against trusted local origins.
/// Untrusted cross-origin requests are rejected (C-01).
pub fn check_origin(request: &Request) -> OriginCheck {
    let origin_header = request.headers().iter().find(|h| {
        h.field.equiv("Origin") || h.field.as_str().as_str().eq_ignore_ascii_case("origin")
    });

    let origin_val = match origin_header {
        Some(h) => h.value.as_str().trim(),
        None => return OriginCheck::NoOrigin,
    };

    if origin_val.is_empty() {
        return OriginCheck::Untrusted;
    }

    if let Ok(parsed) = Url::parse(origin_val) {
        if parsed.scheme() == "http" {
            if let Some(host) = parsed.host_str() {
                let host_lower = host.to_lowercase();
                if host_lower == "127.0.0.1" || host_lower == "localhost" {
                    return OriginCheck::Trusted(origin_val.to_string());
                }
            }
        }
    }

    OriginCheck::Untrusted
}

/// Reads request body up to MAX_REQUEST_BODY_BYTES (64 KB). Returns error if exceeded (H-14).
pub fn read_bounded_body(request: &mut Request) -> Result<String, (u16, &'static str)> {
    if let Some(len) = request.body_length() {
        if len > MAX_REQUEST_BODY_BYTES as usize {
            return Err((413, "Request body exceeds 64KB limit"));
        }
    }

    let mut buf = Vec::new();
    let mut reader = request.as_reader().take(MAX_REQUEST_BODY_BYTES + 1);
    if reader.read_to_end(&mut buf).is_err() {
        return Err((400, "Failed to read request body"));
    }
    if buf.len() > MAX_REQUEST_BODY_BYTES as usize {
        return Err((413, "Request body exceeds 64KB limit"));
    }
    String::from_utf8(buf).map_err(|_| (400, "Request body is not valid UTF-8"))
}

pub fn run_server(server: Server, ctx: Arc<GuiServerContext>) {
    info!(
        "FlowGuard GUI HTTP server listening on http://{}",
        server.server_addr()
    );

    while ctx.running.load(Ordering::SeqCst) {
        match server.recv_timeout(Duration::from_millis(100)) {
            Ok(Some(request)) => {
                let ctx_clone = ctx.clone();
                std::thread::spawn(move || {
                    handle_request(request, &ctx_clone);
                });
            }
            Ok(None) => {
                // Timeout, continue loop to check running flag
            }
            Err(e) => {
                error!("HTTP server recv error: {}", e);
                break;
            }
        }
    }

    info!("FlowGuard GUI HTTP server stopped.");
}

fn handle_request(mut request: Request, ctx: &GuiServerContext) {
    // 1. Host Validation (C-01): Only allow 127.0.0.1:<port> or localhost:<port>
    if !validate_host(&request) {
        let resp = Response::from_string(r#"{"error":"Invalid or missing Host header"}"#)
            .with_status_code(400)
            .with_header(
                Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
            )
            .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap())
            .with_header(
                Header::from_bytes(&b"X-Content-Type-Options"[..], &b"nosniff"[..]).unwrap(),
            )
            .with_header(Header::from_bytes(&b"X-Frame-Options"[..], &b"DENY"[..]).unwrap());
        let _ = request.respond(resp);
        return;
    }

    // 2. Origin Validation (C-01): Reject untrusted cross-origin requests
    if let OriginCheck::Untrusted = check_origin(&request) {
        let resp = Response::from_string(
            r#"{"error":"Forbidden: Untrusted cross-origin request rejected"}"#,
        )
        .with_status_code(403)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap())
        .with_header(Header::from_bytes(&b"X-Content-Type-Options"[..], &b"nosniff"[..]).unwrap())
        .with_header(Header::from_bytes(&b"X-Frame-Options"[..], &b"DENY"[..]).unwrap());
        let _ = request.respond(resp);
        return;
    }

    let url = request.url().to_string();
    let method = request.method().clone();

    // Remove query params for routing
    let path = url.split('?').next().unwrap_or("/");

    match (&method, path) {
        (&Method::Options, _) => {
            let origin_check = check_origin(&request);
            let methods = Header::from_bytes(
                &b"Access-Control-Allow-Methods"[..],
                &b"GET, POST, OPTIONS, HEAD, DELETE"[..],
            )
            .unwrap();
            let headers = Header::from_bytes(
                &b"Access-Control-Allow-Headers"[..],
                &b"Content-Type, Authorization, X-Requested-With"[..],
            )
            .unwrap();
            let cache = Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap();
            let max_age =
                Header::from_bytes(&b"Access-Control-Max-Age"[..], &b"86400"[..]).unwrap();

            let mut resp = Response::empty(204)
                .with_header(methods)
                .with_header(headers)
                .with_header(cache)
                .with_header(max_age);

            if let OriginCheck::Trusted(ref o) = origin_check {
                if let Ok(cors) =
                    Header::from_bytes(&b"Access-Control-Allow-Origin"[..], o.as_bytes())
                {
                    resp.add_header(cors);
                }
                if let Ok(vary) = Header::from_bytes(&b"Vary"[..], &b"Origin"[..]) {
                    resp.add_header(vary);
                }
            }

            let _ = request.respond(resp);
        }
        (&Method::Get, "/api/system/open-apps") => {
            let apps = crate::infrastructure::native_windows::api::list_running_applications();
            let dtos: Vec<RunningAppDto> = apps
                .into_iter()
                .map(|a| RunningAppDto {
                    process_name: a.process_name,
                    display_name: a.display_name,
                    window_title: a.window_title,
                })
                .collect();
            let json = serde_json::to_string(&dtos).unwrap_or_else(|_| "[]".to_string());
            respond_json(request, 200, &json);
        }
        (&Method::Get | &Method::Head, "/" | "/index.html") => {
            let resp = Response::from_string(INDEX_HTML)
                .with_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                        .unwrap(),
                )
                .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-cache"[..]).unwrap())
                .with_header(
                    Header::from_bytes(&b"X-Content-Type-Options"[..], &b"nosniff"[..]).unwrap(),
                )
                .with_header(Header::from_bytes(&b"X-Frame-Options"[..], &b"DENY"[..]).unwrap());
            let _ = request.respond(resp);
        }
        (&Method::Get | &Method::Head, "/style.css") => {
            let resp = Response::from_string(STYLE_CSS)
                .with_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"text/css; charset=utf-8"[..])
                        .unwrap(),
                )
                .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-cache"[..]).unwrap())
                .with_header(
                    Header::from_bytes(&b"X-Content-Type-Options"[..], &b"nosniff"[..]).unwrap(),
                )
                .with_header(Header::from_bytes(&b"X-Frame-Options"[..], &b"DENY"[..]).unwrap());
            let _ = request.respond(resp);
        }
        (&Method::Get | &Method::Head, "/app.js") => {
            let resp = Response::from_string(APP_JS)
                .with_header(
                    Header::from_bytes(
                        &b"Content-Type"[..],
                        &b"application/javascript; charset=utf-8"[..],
                    )
                    .unwrap(),
                )
                .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-cache"[..]).unwrap())
                .with_header(
                    Header::from_bytes(&b"X-Content-Type-Options"[..], &b"nosniff"[..]).unwrap(),
                )
                .with_header(Header::from_bytes(&b"X-Frame-Options"[..], &b"DENY"[..]).unwrap());
            let _ = request.respond(resp);
        }
        (&Method::Get, "/api/session/live") => {
            let state = ctx.gui_state.lock().unwrap();
            let snap = state.snapshot();
            let json = serde_json::to_string(&snap).unwrap_or_else(|_| "{}".to_string());
            respond_json(request, 200, &json);
        }
        (&Method::Get, "/api/workflows") => {
            let repo = SqliteWorkflowRepository::new(ctx.db.clone());
            match repo.list() {
                Ok(wfs) => {
                    let dtos: Vec<WorkflowDto> = wfs.into_iter().map(workflow_to_dto).collect();
                    let json = serde_json::to_string(&dtos).unwrap_or_else(|_| "[]".to_string());
                    respond_json(request, 200, &json);
                }
                Err(e) => {
                    respond_error(request, 500, &format!("Failed to list workflows: {}", e));
                }
            }
        }
        (&Method::Post, "/api/workflows") => {
            let body = match read_bounded_body(&mut request) {
                Ok(b) => b,
                Err((status, msg)) => {
                    respond_error(request, status, msg);
                    return;
                }
            };

            let req_data: CreateWorkflowRequest = match serde_json::from_str(&body) {
                Ok(d) => d,
                Err(e) => {
                    respond_error(request, 400, &format!("Invalid JSON body: {}", e));
                    return;
                }
            };

            // H-18: Prevent empty app/site selection from creating an immediate silent deny-all workflow.
            if req_data.allowed_apps.is_empty() && req_data.allowed_domains.is_empty() {
                respond_error(
                    request,
                    400,
                    "Workflow must specify at least one allowed application or domain to avoid creating an unintended deny-all policy.",
                );
                return;
            }

            let mut repo = SqliteWorkflowRepository::new(ctx.db.clone());
            let base_slug: String = req_data
                .name
                .trim()
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect();
            let base_slug = base_slug.trim_matches('-');
            let base_slug = if base_slug.is_empty() {
                "custom"
            } else {
                base_slug
            };

            let mut id_candidate = format!("wf-{}", base_slug);
            if repo
                .get(&WorkflowId::new(&id_candidate))
                .ok()
                .flatten()
                .is_some()
            {
                id_candidate = format!(
                    "{}-{}",
                    id_candidate,
                    chrono::Utc::now().timestamp_millis() % 1_000_000
                );
            }
            let wf_id = WorkflowId::new(id_candidate);

            let mut wf = Workflow::new(wf_id.clone(), req_data.name);
            wf.description = req_data.description.unwrap_or_default();
            wf.target_duration_minutes = req_data.duration_minutes;
            wf.default_grace_period_seconds = 30;

            let contract_lower = req_data.contract_type.to_lowercase();
            let duration_mins =
                if contract_lower.contains("unbreakable") || contract_lower.contains("ulysses") {
                    Some(req_data.duration_minutes.unwrap_or(25))
                } else {
                    req_data.duration_minutes
                };

            let contract = match contract_lower.as_str() {
                "unbreakable" | "ulysses" => CommitmentContract::Unbreakable {
                    target_duration_seconds: duration_mins.unwrap_or(25) as u64 * 60,
                },
                "breakable" | "accountable" => CommitmentContract::AccountableBreakable {
                    min_reason_characters: req_data.min_reason_characters.unwrap_or(10),
                },
                "standard" => CommitmentContract::Standard,
                other => {
                    respond_error(
                        request,
                        400,
                        &format!(
                            "Invalid contract_type '{}'. Allowed: standard, accountable, unbreakable",
                            other
                        ),
                    );
                    return;
                }
            };

            wf.target_duration_minutes = duration_mins;
            wf.commitment_contract = contract;

            for (idx, app) in req_data.allowed_apps.into_iter().enumerate() {
                wf.rules.push(WorkflowRule {
                    id: RuleId::new(format!("rule-{}-app-{}", wf_id.0, idx)),
                    workflow_id: wf_id.clone(),
                    description: format!("Allow application {}", app),
                    scope: RuleScope::Application,
                    matcher: RuleMatcher::ApplicationName(app),
                    effect: RuleEffect::Allow,
                    priority_weight: 10,
                    custom_grace_seconds: None,
                    is_enabled: true,
                    expires_at: None,
                });
            }

            for (idx, dom) in req_data.allowed_domains.into_iter().enumerate() {
                wf.rules.push(WorkflowRule {
                    id: RuleId::new(format!("rule-{}-dom-{}", wf_id.0, idx)),
                    workflow_id: wf_id.clone(),
                    description: format!("Allow domain {}", dom),
                    scope: RuleScope::Domain,
                    matcher: RuleMatcher::DomainPattern(dom),
                    effect: RuleEffect::Allow,
                    priority_weight: 20,
                    custom_grace_seconds: None,
                    is_enabled: true,
                    expires_at: None,
                });
            }
            if let Err(e) = repo.save(&wf) {
                respond_error(request, 500, &format!("Failed to save workflow: {}", e));
            } else {
                respond_json(request, 201, r#"{"status":"created"}"#);
            }
        }
        (&Method::Delete, p) if p.starts_with("/api/workflows/") => {
            let id = p.trim_start_matches("/api/workflows/");
            if id.is_empty() {
                respond_error(request, 400, "Workflow ID required");
                return;
            }
            if ["wf-study", "wf-deep-work", "wf-coding", "wf-reading"].contains(&id) {
                respond_error(request, 403, "Cannot delete built-in preset workflows");
                return;
            }
            let mut repo = SqliteWorkflowRepository::new(ctx.db.clone());
            match repo.delete(&WorkflowId::new(id)) {
                Ok(true) => respond_json(request, 200, r#"{"status":"deleted"}"#),
                Ok(false) => respond_error(request, 404, "Workflow not found"),
                Err(e) => respond_error(request, 500, &format!("Failed to delete workflow: {}", e)),
            }
        }
        (&Method::Get, "/api/history") => {
            let session_repo = SqliteSessionRepository::new(ctx.db.clone());
            let wf_repo = SqliteWorkflowRepository::new(ctx.db.clone());

            match session_repo.list_recent(50) {
                Ok(sessions) => {
                    let mut history_dtos = Vec::new();
                    for s in sessions {
                        let wf_name = wf_repo
                            .get(&s.workflow_id)
                            .ok()
                            .flatten()
                            .map(|w| w.name)
                            .unwrap_or_else(|| s.workflow_id.0.clone());

                        let total_tracked = s.total_focus_seconds
                            + s.total_grace_seconds
                            + s.total_distraction_seconds;
                        let quality_score = if total_tracked > 0 {
                            (s.total_focus_seconds as f64 / total_tracked as f64) * 100.0
                        } else {
                            100.0
                        };

                        let contract_str = match &s.commitment_contract {
                            CommitmentContract::Standard => "Standard".to_string(),
                            CommitmentContract::AccountableBreakable { .. } => {
                                "AccountableBreakable".to_string()
                            }
                            CommitmentContract::Unbreakable { .. } => "Unbreakable".to_string(),
                        };

                        let drifts = session_repo
                            .list_drifts(&s.id)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|d| {
                                let res_str = match d.resolution {
                                    DriftResolution::SelfCorrected { recovery_seconds } => {
                                        format!("Self-Corrected in {}s", recovery_seconds)
                                    }
                                    DriftResolution::Intervened => "Intervened".to_string(),
                                };
                                DriftEventDto {
                                    target_name: d.target_name,
                                    duration_seconds: d.duration_seconds,
                                    resolution: res_str,
                                    timestamp: d.timestamp,
                                }
                            })
                            .collect();

                        history_dtos.push(SessionHistoryDto {
                            id: s.id.0,
                            workflow_id: s.workflow_id.0,
                            workflow_name: wf_name,
                            state: format!("{:?}", s.state),
                            contract_type: contract_str,
                            start_time: s.start_time,
                            end_time: s.end_time,
                            early_exit_reason: s.early_exit_reason,
                            total_focus_seconds: s.total_focus_seconds,
                            total_grace_seconds: s.total_grace_seconds,
                            total_distraction_seconds: s.total_distraction_seconds,
                            total_paused_seconds: s.total_paused_seconds,
                            total_afk_seconds: s.total_afk_seconds,
                            pull_back_count: s.pull_back_count,
                            intervention_count: s.intervention_count,
                            quality_score,
                            drifts,
                        });
                    }

                    let json =
                        serde_json::to_string(&history_dtos).unwrap_or_else(|_| "[]".to_string());
                    respond_json(request, 200, &json);
                }
                Err(e) => {
                    respond_error(request, 500, &format!("Failed to retrieve history: {}", e));
                }
            }
        }
        (&Method::Post, "/api/session/start") => {
            let body = match read_bounded_body(&mut request) {
                Ok(b) => b,
                Err((status, msg)) => {
                    respond_error(request, status, msg);
                    return;
                }
            };

            let req_data: StartSessionRequest = match serde_json::from_str(&body) {
                Ok(d) => d,
                Err(e) => {
                    respond_error(request, 400, &format!("Invalid JSON body: {}", e));
                    return;
                }
            };

            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            let _ = ctx
                .command_tx
                .send(super::engine::GuiCommand::start_session(
                    WorkflowId::new(req_data.workflow_id),
                    reply_tx,
                ));

            match reply_rx.recv_timeout(Duration::from_secs(3)) {
                Ok(Ok(sess)) => {
                    let json = serde_json::json!({
                        "status": "started",
                        "session_id": sess.id.0
                    })
                    .to_string();
                    respond_json(request, 200, &json);
                }
                Ok(Err(e)) => respond_error(request, 400, &e),
                Err(_) => respond_error(request, 504, "Session start timed out"),
            }
        }
        (&Method::Post, "/api/session/pause") => {
            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            let _ = ctx
                .command_tx
                .send(super::engine::GuiCommand::pause_session(reply_tx));

            match reply_rx.recv_timeout(Duration::from_secs(3)) {
                Ok(Ok(())) => respond_json(request, 200, r#"{"status":"paused"}"#),
                Ok(Err(e)) => respond_error(request, 400, &e),
                Err(_) => respond_error(request, 504, "Pause timed out"),
            }
        }
        (&Method::Post, "/api/session/resume") => {
            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            let _ = ctx
                .command_tx
                .send(super::engine::GuiCommand::resume_session(reply_tx));

            match reply_rx.recv_timeout(Duration::from_secs(3)) {
                Ok(Ok(())) => respond_json(request, 200, r#"{"status":"resumed"}"#),
                Ok(Err(e)) => respond_error(request, 400, &e),
                Err(_) => respond_error(request, 504, "Resume timed out"),
            }
        }
        (&Method::Post, "/api/session/focus-workflow-app") => {
            let (allowed_apps, allowed_titles) = {
                let st = ctx.gui_state.lock().unwrap();
                if let Some(ref wf) = st.active_workflow {
                    let mut apps = Vec::new();
                    let mut titles = Vec::new();
                    for r in &wf.rules {
                        if r.is_enabled && r.effect == crate::domain::workflow::RuleEffect::Allow {
                            match &r.matcher {
                                crate::domain::workflow::RuleMatcher::ApplicationName(app) => {
                                    apps.push(app.clone());
                                }
                                crate::domain::workflow::RuleMatcher::DomainPattern(pat) => {
                                    titles.push(pat.clone());
                                }
                                _ => {}
                            }
                        }
                    }
                    (apps, titles)
                } else {
                    (Vec::new(), Vec::new())
                }
            };

            let switched =
                crate::infrastructure::native_windows::api::force_pull_back_to_allowed_app(
                    &allowed_apps,
                    &allowed_titles,
                );

            let json = serde_json::json!({
                "status": "ok",
                "switched_to": switched
            })
            .to_string();
            respond_json(request, 200, &json);
        }
        (&Method::Post, "/api/session/stop") => {
            let body = match read_bounded_body(&mut request) {
                Ok(b) => b,
                Err((status, msg)) => {
                    respond_error(request, status, msg);
                    return;
                }
            };
            let req_data: StopSessionRequest = if body.trim().is_empty() {
                StopSessionRequest { reason: None }
            } else {
                match serde_json::from_str(&body) {
                    Ok(d) => d,
                    Err(e) => {
                        respond_error(request, 400, &format!("Invalid JSON body: {}", e));
                        return;
                    }
                }
            };

            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            let _ = ctx.command_tx.send(super::engine::GuiCommand::stop_session(
                req_data.reason,
                reply_tx,
            ));

            match reply_rx.recv_timeout(Duration::from_secs(3)) {
                Ok(Ok(sess)) => {
                    let json = serde_json::json!({
                        "status": "stopped",
                        "session_id": sess.id.0
                    })
                    .to_string();
                    respond_json(request, 200, &json);
                }
                Ok(Err(e)) => {
                    let status = if e.contains("Unbreakable") {
                        403
                    } else if e.contains("written reason") || e.contains("characters") {
                        400
                    } else {
                        500
                    };
                    respond_error(request, status, &e);
                }
                Err(_) => respond_error(request, 504, "Stop session timed out"),
            }
        }
        _ => {
            respond_error(request, 404, "Endpoint not found");
        }
    }
}

pub fn workflow_to_dto(wf: Workflow) -> WorkflowDto {
    let (contract_str, min_chars) = match wf.commitment_contract {
        CommitmentContract::Standard => ("Standard".to_string(), 0),
        CommitmentContract::AccountableBreakable {
            min_reason_characters,
        } => ("Accountable Breakable".to_string(), min_reason_characters),
        CommitmentContract::Unbreakable { .. } => ("Unbreakable".to_string(), 0),
    };

    let mut allowed_apps = Vec::new();
    let mut allowed_domains = Vec::new();

    for r in wf.rules {
        if r.is_enabled && r.effect == RuleEffect::Allow {
            match r.matcher {
                RuleMatcher::ApplicationName(app) => allowed_apps.push(app),
                RuleMatcher::DomainPattern(dom) => allowed_domains.push(dom),
                _ => {}
            }
        }
    }

    WorkflowDto {
        id: wf.id.0,
        name: wf.name,
        description: wf.description,
        target_duration_minutes: wf.target_duration_minutes,
        contract_type: contract_str,
        min_reason_characters: min_chars,
        default_grace_seconds: wf.default_grace_period_seconds,
        allowed_apps,
        allowed_domains,
    }
}

pub fn respond_json(request: Request, status: u16, json: &str) {
    let origin_check = check_origin(&request);
    let header = Header::from_bytes(
        &b"Content-Type"[..],
        &b"application/json; charset=utf-8"[..],
    )
    .unwrap();
    let cache_control = Header::from_bytes(
        &b"Cache-Control"[..],
        &b"no-store, no-cache, must-revalidate"[..],
    )
    .unwrap();
    let pragma = Header::from_bytes(&b"Pragma"[..], &b"no-cache"[..]).unwrap();
    let nosniff = Header::from_bytes(&b"X-Content-Type-Options"[..], &b"nosniff"[..]).unwrap();
    let frame_deny = Header::from_bytes(&b"X-Frame-Options"[..], &b"DENY"[..]).unwrap();

    let methods = Header::from_bytes(
        &b"Access-Control-Allow-Methods"[..],
        &b"GET, POST, OPTIONS, HEAD, DELETE"[..],
    )
    .unwrap();
    let headers = Header::from_bytes(
        &b"Access-Control-Allow-Headers"[..],
        &b"Content-Type, Authorization, X-Requested-With"[..],
    )
    .unwrap();

    let mut resp = Response::from_string(json)
        .with_status_code(status)
        .with_header(header)
        .with_header(cache_control)
        .with_header(pragma)
        .with_header(nosniff)
        .with_header(frame_deny)
        .with_header(methods)
        .with_header(headers);

    // C-01: Never send wildcard Access-Control-Allow-Origin: *. Only reflect verified trusted origins.
    if let OriginCheck::Trusted(ref o) = origin_check {
        if let Ok(cors) = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], o.as_bytes()) {
            resp.add_header(cors);
        }
        if let Ok(vary) = Header::from_bytes(&b"Vary"[..], &b"Origin"[..]) {
            resp.add_header(vary);
        }
    }

    let _ = request.respond(resp);
}

pub fn respond_error(request: Request, status: u16, message: &str) {
    let json = serde_json::json!({ "error": message }).to_string();
    respond_json(request, status, &json);
}
