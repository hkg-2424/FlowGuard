use reqwest::blocking::Client;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_http::Server;

use flowguard::application::ports::{ActivitySource, WorkflowRepository};
use flowguard::application::workflow_service::WorkflowService;
use flowguard::domain::workflow::CommitmentContract;
use flowguard::domain::{
    ActivityCapabilities, ActivityObservation, DiagnosticReport, FreshnessState, IdleState,
    SourceCapabilities, SourceHealth,
};
use flowguard::infrastructure::persistence::sqlite::SqliteDb;
use flowguard::infrastructure::persistence::{SqliteSessionRepository, SqliteWorkflowRepository};
use flowguard::presentation::gui::engine::GuiEngine;
use flowguard::presentation::gui::seed::seed_default_workflows_if_empty;
use flowguard::presentation::gui::server::{run_server, GuiServerContext};
use flowguard::presentation::gui::state::{
    CreateWorkflowRequest, GuiState, LiveSessionSnapshot, SessionHistoryDto, StartSessionRequest,
    StopSessionRequest, WorkflowDto,
};

// Mock activity source for GUI automated testing
struct MockGuiSource {
    app: String,
    title: String,
    idle: IdleState,
}

impl ActivitySource for MockGuiSource {
    fn connect(
        &mut self,
    ) -> Result<SourceCapabilities, flowguard::application::ports::SourceError> {
        Ok(SourceCapabilities {
            provider: "mock_gui".to_string(),
            server_version: Some("1.0".to_string()),
            hostname: Some("TEST-GUI".to_string()),
            is_testing: true,
            capabilities: ActivityCapabilities {
                application_tracking: true,
                window_title_tracking: true,
                idle_tracking: true,
                browser_url_tracking: false,
                incognito_detection: false,
            },
        })
    }

    fn poll(&mut self) -> Result<ActivityObservation, flowguard::application::ports::SourceError> {
        Ok(ActivityObservation {
            observed_at: chrono::Utc::now(),
            application: Some(self.app.clone()),
            window_title: Some(self.title.clone()),
            browser: None,
            idle_state: self.idle,
            capabilities: ActivityCapabilities {
                application_tracking: true,
                window_title_tracking: true,
                idle_tracking: true,
                browser_url_tracking: false,
                incognito_detection: false,
            },
            source_health: SourceHealth::Connected,
            window_freshness: FreshnessState::Fresh,
            idle_freshness: FreshnessState::Fresh,
            browser_freshness: FreshnessState::Missing,
            diagnostics: Vec::new(),
        })
    }

    fn health(&self) -> SourceHealth {
        SourceHealth::Connected
    }

    fn diagnose(&mut self) -> Result<DiagnosticReport, flowguard::application::ports::SourceError> {
        Ok(DiagnosticReport::unreachable(
            "MockGui",
            "localhost",
            SourceHealth::Connected,
            "Healthy test mock".to_string(),
        ))
    }
}

// 1. Verify default workflows are seeded out-of-the-box for novice users
#[test]
fn test_gui_default_workflow_seeding() {
    let temp_db_path = std::env::temp_dir().join(format!(
        "flowguard_gui_seed_test_{}.db",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    let db = SqliteDb::open(&temp_db_path).expect("Opens test sqlite db");
    let mut repo = SqliteWorkflowRepository::new(db);

    // Initial empty
    assert!(repo.list().unwrap().is_empty());

    // Run seeder
    seed_default_workflows_if_empty(&mut repo);

    // Must seed 4 workflows: Study, Deep Work, Coding, Reading
    let workflows = repo.list().unwrap();
    assert_eq!(workflows.len(), 4);

    let names: Vec<String> = workflows.iter().map(|w| w.name.clone()).collect();
    assert!(names.contains(&"Study".to_string()));
    assert!(names.contains(&"Deep Work".to_string()));
    assert!(names.contains(&"Coding".to_string()));
    assert!(names.contains(&"Reading".to_string()));

    let study = workflows.iter().find(|w| w.name == "Study").unwrap();
    assert_eq!(study.target_duration_minutes, Some(45));
    assert!(matches!(
        study.commitment_contract,
        CommitmentContract::AccountableBreakable {
            min_reason_characters: 10
        }
    ));

    // Cleanup
    let _ = std::fs::remove_file(temp_db_path);
}

// 2. Verify static UI assets contain required tour, dashboard, modal, and history elements
#[test]
fn test_gui_static_assets_embedded_and_complete() {
    let index_html = include_str!("../src/presentation/gui/static/index.html");
    let style_css = include_str!("../src/presentation/gui/static/style.css");
    let app_js = include_str!("../src/presentation/gui/static/app.js");

    // Feature 1: Tour elements
    assert!(index_html.contains("Welcome to FlowGuard"));
    assert!(index_html.contains("The 30-Second Grace Window"));
    assert!(index_html.contains("Pull-Back Resilience"));
    assert!(index_html.contains("modal-tour"));
    assert!(app_js.contains("startTour"));
    assert!(app_js.contains("TOTAL_TOUR_STEPS = 5"));

    // Feature 2: Workflows grid and quick creation
    assert!(index_html.contains("workflows-container"));
    assert!(index_html.contains("modal-create-workflow"));
    assert!(app_js.contains("submitCreateWorkflow"));

    // Feature 3: Live focus session dashboard with 30s grace circle
    assert!(index_html.contains("timer-digits"));
    assert!(index_html.contains("grace-ring"));
    assert!(index_html.contains("alert-countdown"));
    assert!(index_html.contains("pullback-banner"));

    // Feature 4: Accountable exit modal
    assert!(index_html.contains("modal-accountable-exit"));
    assert!(index_html.contains("reason-char-count"));
    assert!(index_html.contains("modal-unbreakable-warning"));
    assert!(app_js.contains("updateReasonCharCount"));

    // Feature 5: History & Analytics
    assert!(index_html.contains("view-history"));
    assert!(index_html.contains("hist-total-focus"));
    assert!(index_html.contains("history-table-body"));

    // Feature 6: Status indicators
    assert!(index_html.contains("status-indicator"));
    assert!(index_html.contains("status-dot"));

    // Stylesheet rules
    assert!(style_css.contains(".radial-ring-container"));
    assert!(style_css.contains(".ring-progress"));
    assert!(style_css.contains(".alert-banner"));
}

// 3. Integration test: Full GUI HTTP server and Live Session Lifecycle
#[test]
fn test_gui_full_http_api_and_session_lifecycle() {
    let temp_db_path = std::env::temp_dir().join(format!(
        "flowguard_gui_server_test_{}.db",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    let db = SqliteDb::open(&temp_db_path).expect("Opens test sqlite db");

    let mut repo = SqliteWorkflowRepository::new(db.clone());
    seed_default_workflows_if_empty(&mut repo);

    // Bind ephemeral server
    let server = Server::http("127.0.0.1:0").expect("Binds ephemeral test server");
    let base_url = format!("http://{}", server.server_addr());

    let running = Arc::new(AtomicBool::new(true));
    let gui_state = Arc::new(Mutex::new(GuiState::new(
        "Native Windows (Win32)".to_string(),
    )));

    let mock_source = MockGuiSource {
        app: "code.exe".to_string(),
        title: "FlowGuard - Visual Studio Code".to_string(),
        idle: IdleState::Active,
    };

    let wf_repo_svc = SqliteWorkflowRepository::new(db.clone());
    let sess_repo_svc = SqliteSessionRepository::new(db.clone());
    let wf_service = WorkflowService::new(
        Box::new(mock_source),
        Box::new(wf_repo_svc),
        Box::new(sess_repo_svc),
        500,
    );

    let (command_tx, command_rx) = std::sync::mpsc::channel();
    let engine = GuiEngine::new(wf_service, gui_state.clone(), command_rx, running.clone());
    let engine_repo = SqliteWorkflowRepository::new(db.clone());
    let engine_handle = std::thread::spawn(move || {
        engine.run(Box::new(engine_repo));
    });

    let server_ctx = Arc::new(GuiServerContext {
        gui_state: gui_state.clone(),
        db: db.clone(),
        running: running.clone(),
        command_tx,
    });
    let server_handle = std::thread::spawn(move || {
        run_server(server, server_ctx);
    });

    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // Verify GET / returns HTML
    let index_res = client
        .get(format!("{}/", base_url))
        .send()
        .expect("Fetches index.html");
    assert_eq!(index_res.status().as_u16(), 200);
    let html_body = index_res.text().unwrap();
    assert!(html_body.contains("FlowGuard"));

    // Verify GET /api/workflows returns 4 default seeded workflows
    let wf_res = client
        .get(format!("{}/api/workflows", base_url))
        .send()
        .expect("Lists workflows");
    assert_eq!(wf_res.status().as_u16(), 200);
    let workflows: Vec<WorkflowDto> = wf_res.json().unwrap();
    assert_eq!(workflows.len(), 4);

    // Verify POST /api/workflows creates a new custom workflow
    let create_res = client
        .post(format!("{}/api/workflows", base_url))
        .json(&CreateWorkflowRequest {
            name: "Deep Writing".to_string(),
            description: Some("Distraction-free book drafting".to_string()),
            duration_minutes: Some(40),
            contract_type: "standard".to_string(),
            min_reason_characters: Some(10),
            allowed_apps: vec!["notepad.exe".to_string(), "word.exe".to_string()],
            allowed_domains: vec!["thesaurus.com".to_string()],
        })
        .send()
        .expect("Creates custom workflow");
    assert_eq!(create_res.status().as_u16(), 201);

    // Verify GET /api/session/live initial state
    let live_res = client
        .get(format!("{}/api/session/live", base_url))
        .send()
        .expect("Fetches live session");
    assert_eq!(live_res.status().as_u16(), 200);
    let snap: LiveSessionSnapshot = live_res.json().unwrap();
    assert_eq!(snap.state, "None");

    // Start Study workflow session (AccountableBreakable contract)
    let start_res = client
        .post(format!("{}/api/session/start", base_url))
        .json(&StartSessionRequest {
            workflow_id: "wf-study".to_string(),
            duration_minutes: Some(45),
            contract_type: None,
            min_reason_characters: None,
        })
        .send()
        .expect("Starts session");
    assert_eq!(start_res.status().as_u16(), 200);

    // Allow engine thread to perform a tick
    std::thread::sleep(Duration::from_millis(300));

    // Verify session is now active
    let live_active_res = client
        .get(format!("{}/api/session/live", base_url))
        .send()
        .expect("Fetches active session");
    let active_snap: LiveSessionSnapshot = live_active_res.json().unwrap();
    assert_eq!(active_snap.state, "Active");
    assert_eq!(active_snap.contract_type, "AccountableBreakable");
    assert_eq!(active_snap.workflow_name, Some("Study".to_string()));

    // Pause session
    let pause_res = client
        .post(format!("{}/api/session/pause", base_url))
        .send()
        .expect("Pauses session");
    assert_eq!(pause_res.status().as_u16(), 200);

    // Resume session
    let resume_res = client
        .post(format!("{}/api/session/resume", base_url))
        .send()
        .expect("Resumes session");
    assert_eq!(resume_res.status().as_u16(), 200);

    // Attempt early exit with reason TOO SHORT (min 10 characters required)
    let short_exit_res = client
        .post(format!("{}/api/session/stop", base_url))
        .json(&StopSessionRequest {
            reason: Some("bored".to_string()), // only 5 chars
        })
        .send()
        .expect("Sends short reason exit");
    assert_eq!(
        short_exit_res.status().as_u16(),
        400,
        "Must reject reason shorter than 10 characters"
    );

    // End session with VALID written reflection reason
    let valid_exit_res = client
        .post(format!("{}/api/session/stop", base_url))
        .json(&StopSessionRequest {
            reason: Some("Completed assigned research module ahead of schedule.".to_string()),
        })
        .send()
        .expect("Sends valid exit reason");
    assert_eq!(valid_exit_res.status().as_u16(), 200);

    // Verify GET /api/history returns the completed session
    let hist_res = client
        .get(format!("{}/api/history", base_url))
        .send()
        .expect("Fetches history");
    assert_eq!(hist_res.status().as_u16(), 200);
    let history: Vec<SessionHistoryDto> = hist_res.json().unwrap();
    assert!(!history.is_empty());
    let last = &history[0];
    assert_eq!(last.workflow_name, "Study");
    assert_eq!(
        last.early_exit_reason.as_deref(),
        Some("Completed assigned research module ahead of schedule.")
    );

    // Shutdown test server
    running.store(false, Ordering::SeqCst);
    let _ = engine_handle.join();
    let _ = server_handle.join();
    let _ = std::fs::remove_file(temp_db_path);
}
