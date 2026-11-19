pub mod engine;
pub mod seed;
pub mod server;
pub mod state;

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{info, warn};

use crate::application::workflow_service::WorkflowService;
use crate::config::{default_database_path, FlowGuardConfig};
use crate::infrastructure::create_activity_source;
use crate::infrastructure::persistence::sqlite::SqliteDb;
use crate::infrastructure::persistence::{SqliteSessionRepository, SqliteWorkflowRepository};

use engine::GuiEngine;
use server::GuiServerContext;
use state::GuiState;

/// Launches the beginner-friendly Desktop GUI with interactive onboarding tour,
/// workflow selector, live focus dashboard, accountable exit modal, and history analytics.
pub fn launch_desktop_gui(
    config: &FlowGuardConfig,
    custom_port: Option<u16>,
    no_window: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let port = custom_port.unwrap_or(3124);
    let server = match tiny_http::Server::http(format!("127.0.0.1:{}", port)) {
        Ok(s) => s,
        Err(e) => {
            warn!(
                "Port {} unavailable ({}). Binding to dynamic available port...",
                port, e
            );
            tiny_http::Server::http("127.0.0.1:0")
                .map_err(|err| std::io::Error::other(err.to_string()))?
        }
    };

    let app_url = format!("http://{}", server.server_addr());

    // Initialize SQLite persistence & seed initial novice-ready workflows
    let db_path = default_database_path();
    let db = SqliteDb::open(&db_path)?;
    let mut wf_repo = SqliteWorkflowRepository::new(db.clone());
    seed::seed_default_workflows_if_empty(&mut wf_repo);

    // Clean up any dangling unclosed sessions from previous unclean shutdowns
    if let Ok(conn) = db.lock() {
        let _ = conn.execute(
            "UPDATE sessions SET state = 'Interrupted', end_time = datetime('now') WHERE state IN ('Active', 'Paused')",
            [],
        );
    }

    // Instantiate activity observation source
    let source = create_activity_source(config);
    let source_name = if config.activity_source.provider.to_lowercase() == "native_windows" {
        "Native Windows (Win32)".to_string()
    } else {
        "Native Windows (Auto Fallback)".to_string()
    };

    let gui_state = Arc::new(Mutex::new(GuiState::new(source_name)));
    let running = Arc::new(AtomicBool::new(true));

    let wf_repo_for_service = SqliteWorkflowRepository::new(db.clone());
    let session_repo = SqliteSessionRepository::new(db.clone());
    let wf_service = WorkflowService::new(
        source,
        Box::new(wf_repo_for_service),
        Box::new(session_repo),
        config.activity_source.activitywatch.poll_interval_ms,
    );

    // Create command channel between server and engine
    let (command_tx, command_rx) = std::sync::mpsc::channel();

    // Spawn focus orchestration engine thread
    let engine = GuiEngine::new(wf_service, gui_state.clone(), command_rx, running.clone());
    let engine_repo = SqliteWorkflowRepository::new(db.clone());
    let engine_handle = std::thread::spawn(move || {
        engine.run(Box::new(engine_repo));
    });

    // Spawn HTTP API & Web UI server thread
    let server_ctx = Arc::new(GuiServerContext {
        gui_state: gui_state.clone(),
        db: db.clone(),
        running: running.clone(),
        command_tx,
    });
    let server_handle = std::thread::spawn(move || {
        server::run_server(server, server_ctx);
    });

    println!();
    println!("==========================================================================");
    println!("                  FlowGuard — Focus & Productivity Assistant              ");
    println!("==========================================================================");
    println!("  Desktop GUI active at: {}", app_url);
    println!("  Features loaded:");
    println!("    * Interactive step-by-step Onboarding Tour");
    println!("    * 1-Click Workflow Selector (Study, Deep Work, Coding, Reading)");
    println!("    * Live Focus Dashboard with animated 30s Grace Circle & Pull-Back Tracking");
    println!("    * Accountable Reflection Exit Guard & Ulysses Locks");
    println!("    * History & Analytics View with Focus Quality Scores");
    println!("  Press Ctrl+C in this console to stop FlowGuard.");
    println!("==========================================================================");
    println!();

    if !no_window {
        launch_desktop_window(&app_url);
    }

    // Set up graceful Ctrl+C handler
    let r = running.clone();
    if let Err(e) = ctrlc::set_handler(move || {
        println!("\nShutdown signal received. Closing FlowGuard Desktop GUI...");
        r.store(false, Ordering::SeqCst);
    }) {
        warn!("Failed to install Ctrl+C handler: {}", e);
    }

    // Main thread waits until running flag is cleared
    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }

    let _ = engine_handle.join();
    let _ = server_handle.join();

    println!("FlowGuard Desktop GUI gracefully stopped.");
    Ok(())
}

/// Opens the Desktop GUI in a dedicated borderless application window using Edge or Chrome's
/// native app mode, seamlessly falling back to the system default browser.
fn launch_desktop_window(url: &str) {
    let mut candidates = vec![
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".to_string(),
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe".to_string(),
        r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string(),
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe".to_string(),
        r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe".to_string(),
    ];

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        candidates.push(format!(
            r"{}\Microsoft\Edge\Application\msedge.exe",
            local_app_data
        ));
        candidates.push(format!(
            r"{}\Google\Chrome\Application\chrome.exe",
            local_app_data
        ));
        candidates.push(format!(
            r"{}\BraveSoftware\Brave-Browser\Application\brave.exe",
            local_app_data
        ));
    }

    let mut launched = false;
    for exe_path in &candidates {
        if Path::new(exe_path).exists() {
            let app_arg = format!("--app={}", url);
            let size_arg = "--window-size=1200,840";
            if let Ok(_child) = Command::new(exe_path).args([&app_arg, size_arg]).spawn() {
                info!("Launched dedicated Desktop Webview window via {}", exe_path);
                launched = true;
                break;
            }
        }
    }

    if !launched {
        info!("Launching Desktop GUI via system default browser");
        if let Err(e) = open::that(url) {
            warn!("Failed to open system browser: {}", e);
        }
    }
}
