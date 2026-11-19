use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server};
use tracing::{error, info};

use crate::application::ports::{SessionRepository, WorkflowRepository};
use crate::domain::workflow::{
    CommitmentContract, DriftResolution, RuleEffect, RuleId, RuleMatcher, RuleScope, Workflow,
    WorkflowId, WorkflowRule,
};
use crate::infrastructure::persistence::sqlite::SqliteDb;
use crate::infrastructure::persistence::{SqliteSessionRepository, SqliteWorkflowRepository};

use super::state::{
    CreateWorkflowRequest, DriftEventDto, GuiState, SessionHistoryDto, StartSessionRequest,
    StopSessionRequest, WorkflowDto,
};

const INDEX_HTML: &str = include_str!("static/index.html");
const STYLE_CSS: &str = include_str!("static/style.css");
const APP_JS: &str = include_str!("static/app.js");

pub struct GuiServerContext {
    pub gui_state: Arc<Mutex<GuiState>>,
    pub db: SqliteDb,
    pub running: Arc<AtomicBool>,
    pub command_tx: std::sync::mpsc::Sender<super::engine::GuiCommand>,
}

pub fn run_server(server: Server, ctx: Arc<GuiServerContext>) {
    info!(
        "FlowGuard GUI HTTP server listening on http://{}",
        server.server_addr()
    );

    while ctx.running.load(Ordering::SeqCst) {
        match server.recv_timeout(Duration::from_millis(100)) {
            Ok(Some(request)) => {
                handle_request(request, &ctx);
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
    let url = request.url().to_string();
    let method = request.method().clone();

    // Remove query params for routing
    let path = url.split('?').next().unwrap_or("/");

    match (&method, path) {
        (&Method::Get | &Method::Head, "/" | "/index.html") => {
            let resp = Response::from_string(INDEX_HTML).with_header(
                Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap(),
            );
            let _ = request.respond(resp);
        }
        (&Method::Get | &Method::Head, "/style.css") => {
            let resp = Response::from_string(STYLE_CSS).with_header(
                Header::from_bytes(&b"Content-Type"[..], &b"text/css; charset=utf-8"[..]).unwrap(),
            );
            let _ = request.respond(resp);
        }
        (&Method::Get | &Method::Head, "/app.js") => {
            let resp = Response::from_string(APP_JS).with_header(
                Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/javascript; charset=utf-8"[..],
                )
                .unwrap(),
            );
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
            let mut body = String::new();
            if let Err(e) = request.as_reader().read_to_string(&mut body) {
                respond_error(request, 400, &format!("Failed to read request body: {}", e));
                return;
            }

            let req_data: CreateWorkflowRequest = match serde_json::from_str(&body) {
                Ok(d) => d,
                Err(e) => {
                    respond_error(request, 400, &format!("Invalid JSON body: {}", e));
                    return;
                }
            };

            let wf_id = WorkflowId::new(format!(
                "wf-{}",
                req_data.name.to_lowercase().replace(' ', "-")
            ));
            let mut wf = Workflow::new(wf_id.clone(), req_data.name);
            wf.description = req_data.description.unwrap_or_default();
            wf.target_duration_minutes = req_data.duration_minutes;
            wf.default_grace_period_seconds = 30;

            wf.commitment_contract = match req_data.contract_type.to_lowercase().as_str() {
                "unbreakable" | "ulysses" => CommitmentContract::Unbreakable {
                    target_duration_seconds: req_data.duration_minutes.unwrap_or(25) as u64 * 60,
                },
                "breakable" | "accountable" => CommitmentContract::AccountableBreakable {
                    min_reason_characters: req_data.min_reason_characters.unwrap_or(10),
                },
                _ => CommitmentContract::Standard,
            };

            for (idx, app) in req_data.allowed_apps.into_iter().enumerate() {
                wf.rules.push(WorkflowRule {
                    id: RuleId::new(format!("rule-app-{}", idx)),
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
                    id: RuleId::new(format!("rule-dom-{}", idx)),
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

            let mut repo = SqliteWorkflowRepository::new(ctx.db.clone());
            if let Err(e) = repo.save(&wf) {
                respond_error(request, 500, &format!("Failed to save workflow: {}", e));
            } else {
                respond_json(request, 201, r#"{"status":"created"}"#);
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
            let mut body = String::new();
            if let Err(e) = request.as_reader().read_to_string(&mut body) {
                respond_error(request, 400, &format!("Failed to read request body: {}", e));
                return;
            }

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
                .send(super::engine::GuiCommand::StartSession {
                    workflow_id: WorkflowId::new(req_data.workflow_id),
                    reply: reply_tx,
                });

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
                .send(super::engine::GuiCommand::PauseSession { reply: reply_tx });

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
                .send(super::engine::GuiCommand::ResumeSession { reply: reply_tx });

            match reply_rx.recv_timeout(Duration::from_secs(3)) {
                Ok(Ok(())) => respond_json(request, 200, r#"{"status":"resumed"}"#),
                Ok(Err(e)) => respond_error(request, 400, &e),
                Err(_) => respond_error(request, 504, "Resume timed out"),
            }
        }
        (&Method::Post, "/api/session/stop") => {
            let mut body = String::new();
            let _ = request.as_reader().read_to_string(&mut body);
            let req_data: StopSessionRequest =
                serde_json::from_str(&body).unwrap_or(StopSessionRequest { reason: None });

            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
            let _ = ctx.command_tx.send(super::engine::GuiCommand::StopSession {
                reason: req_data.reason,
                reply: reply_tx,
            });

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
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    let resp = Response::from_string(json)
        .with_status_code(status)
        .with_header(header);
    let _ = request.respond(resp);
}

pub fn respond_error(request: Request, status: u16, message: &str) {
    let json = serde_json::json!({ "error": message }).to_string();
    respond_json(request, status, &json);
}
