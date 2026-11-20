use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

use flowguard::application::workflow_service::WorkflowService;
use flowguard::application::ActivityService;
use flowguard::config::{default_database_path, FlowGuardConfig};
use flowguard::domain::workflow::{
    CommitmentContract, RuleEffect, RuleId, RuleMatcher, RuleScope, Workflow, WorkflowId,
    WorkflowRule,
};
use flowguard::infrastructure::create_activity_source;
use flowguard::infrastructure::persistence::{
    InMemorySessionRepository, InMemoryWorkflowRepository, SqliteDb, SqliteSessionRepository,
    SqliteWorkflowRepository,
};
use flowguard::presentation::console::{
    print_doctor_report, print_observation_change, print_observation_snapshot, print_status,
};
use flowguard::presentation::session_runner::{
    format_duration_natural, print_session_summary, run_interactive_session,
};

#[derive(Parser)]
#[command(name = "flowguard")]
#[command(author = "FlowGuard Team")]
#[command(version = "0.1.0")]
#[command(
    about = "FlowGuard — Local-first Windows focus and productivity assistant",
    long_about = "FlowGuard (M1) integrates with ActivityWatch to provide normalized, reliable, and privacy-preserving activity observations."
)]
struct Cli {
    /// Optional path to a custom configuration file.
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Enable detailed output, including full window titles and URLs.
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Optional subcommand to execute. If omitted, launches the novice-friendly Desktop GUI.
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the beginner-friendly Desktop GUI in a dedicated window.
    Gui {
        /// Custom port for the local GUI server (default: 3124).
        #[arg(short, long)]
        port: Option<u16>,

        /// Run without automatically launching the desktop window/browser.
        #[arg(long)]
        no_window: bool,
    },

    /// Run diagnostic checks on the ActivityWatch connection, buckets, and watcher freshness.
    Doctor,

    /// Display current activity source health and capabilities.
    Status,

    /// Observe activity continuously (or once with --once).
    Observe {
        /// Read and print a single activity observation snapshot, then exit.
        #[arg(long)]
        once: bool,
    },

    /// Start an active focus workflow session with live policy enforcement and 30s grace alerts.
    Session {
        /// Name of the workflow (e.g. "Deep Work", "Coding", "Study").
        #[arg(short, long, default_value = "Deep Work")]
        workflow: String,

        /// Target session duration in minutes (e.g. 25, 45, 60).
        #[arg(short, long)]
        duration: Option<u32>,

        /// Commitment contract: 'standard', 'breakable' (requires written reason), or 'unbreakable' (Ulysses lock).
        #[arg(long, default_value = "standard")]
        contract: String,

        /// Minimum character length for the reflection reason when breaking an accountable workflow.
        #[arg(long, default_value_t = 10)]
        min_reason: u32,

        /// Allowed application names (comma-separated, e.g. "code.exe,windowsterminal.exe").
        #[arg(long, value_delimiter = ',')]
        allow_apps: Option<Vec<String>>,

        /// Allowed web domains (comma-separated, e.g. "github.com,doc.rust-lang.org").
        #[arg(long, value_delimiter = ',')]
        allow_domains: Option<Vec<String>>,
    },

    /// Display history of previous focus sessions and analytics.
    History {
        /// Limit number of historical sessions displayed.
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },
}

fn main() {
    let cli = Cli::parse();

    // Initialize structured logging with sensible filter
    let default_filter = if cli.verbose {
        "flowguard=debug,warn"
    } else {
        "flowguard=info,warn"
    };
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .init();

    // Load configuration
    let (config, loaded_path) = match FlowGuardConfig::load_or_default(cli.config.as_deref()) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Configuration Error: {}", e);
            std::process::exit(1);
        }
    };

    if cli.verbose {
        if let Some(path) = loaded_path {
            println!("[info] Loaded configuration from: {}", path.display());
        } else {
            println!("[info] Using default configuration settings.");
        }
    }

    match cli.command {
        None | Some(Commands::Gui { .. }) => {
            let (port, no_window) = match cli.command {
                Some(Commands::Gui { port, no_window }) => (port, no_window),
                _ => (None, false),
            };
            if let Err(e) =
                flowguard::presentation::gui::launch_desktop_gui(&config, port, no_window)
            {
                eprintln!("Error running Desktop GUI: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Doctor) => {
            let source = create_activity_source(&config);
            let mut service = ActivityService::new(source, config.clone());
            let report = service.run_doctor();
            print_doctor_report(&report, cli.verbose);
            if !report.is_reachable {
                std::process::exit(1);
            }
        }
        Some(Commands::Status) => {
            let source = create_activity_source(&config);
            let mut service = ActivityService::new(source, config.clone());
            let (health, caps) = service.status();
            let endpoint_desc =
                if config.activity_source.provider.to_lowercase() == "native_windows" {
                    "Win32 Subsystem".to_string()
                } else {
                    format!(
                        "{} (with Native Windows fallback)",
                        config.activity_source.activitywatch.base_url
                    )
                };
            print_status(health, &caps, &endpoint_desc);
        }
        Some(Commands::Observe { once }) => {
            let source = create_activity_source(&config);
            let mut service = ActivityService::new(source, config.clone());
            if once {
                match service.observe_once() {
                    Ok(obs) => {
                        print_observation_snapshot(&obs, cli.verbose);
                    }
                    Err(e) => {
                        eprintln!("Error fetching activity observation: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                let running = Arc::new(AtomicBool::new(true));
                let r = running.clone();

                if let Err(e) = ctrlc::set_handler(move || {
                    r.store(false, Ordering::SeqCst);
                }) {
                    eprintln!("Warning: Failed to set Ctrl+C signal handler: {}", e);
                }

                println!("FlowGuard Activity Observation active. Press Ctrl+C to stop.");
                println!("Streaming state changes...");
                println!();

                let verbose = cli.verbose;
                if let Err(e) = service.observe(running, move |obs| {
                    print_observation_change(obs, verbose);
                }) {
                    eprintln!("Observation stream ended with error: {}", e);
                    std::process::exit(1);
                }

                println!("\nFlowGuard observation stopped.");
            }
        }
        Some(Commands::Session {
            workflow,
            duration,
            contract,
            min_reason,
            allow_apps,
            allow_domains,
        }) => {
            use flowguard::application::ports::{SessionRepository, WorkflowRepository};

            let is_unbreakable = contract.to_lowercase().contains("unbreakable")
                || contract.to_lowercase().contains("ulysses");
            let effective_duration = if is_unbreakable {
                Some(duration.unwrap_or(25))
            } else {
                duration
            };

            let contract_enum = match contract.to_lowercase().as_str() {
                "breakable" | "accountable" => CommitmentContract::AccountableBreakable {
                    min_reason_characters: min_reason,
                },
                "unbreakable" | "ulysses" => CommitmentContract::Unbreakable {
                    target_duration_seconds: effective_duration.unwrap_or(25) as u64 * 60,
                },
                _ => CommitmentContract::Standard,
            };

            let wf_slug = workflow.to_lowercase().replace(' ', "-");
            let wf_id = WorkflowId::new(format!("cli-session-{}", wf_slug));
            let mut wf = Workflow::new(wf_id.clone(), format!("CLI: {}", workflow));
            wf.commitment_contract = contract_enum;
            wf.target_duration_minutes = effective_duration;
            wf.default_grace_period_seconds = 30;

            // Default allowed applications
            let allowed_apps = allow_apps.unwrap_or_else(|| {
                vec![
                    "code.exe".to_string(),
                    "windowsterminal.exe".to_string(),
                    "cmd.exe".to_string(),
                    "powershell.exe".to_string(),
                    "devenv.exe".to_string(),
                    "idea64.exe".to_string(),
                    "notepad.exe".to_string(),
                ]
            });
            for (idx, app) in allowed_apps.into_iter().enumerate() {
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

            // Default allowed domains
            let allowed_domains = allow_domains.unwrap_or_else(|| {
                vec![
                    "github.com".to_string(),
                    "doc.rust-lang.org".to_string(),
                    "docs.rs".to_string(),
                    "stackoverflow.com".to_string(),
                    "crates.io".to_string(),
                ]
            });
            for (idx, domain) in allowed_domains.into_iter().enumerate() {
                wf.rules.push(WorkflowRule {
                    id: RuleId::new(format!("rule-{}-dom-{}", wf_id.0, idx)),
                    workflow_id: wf_id.clone(),
                    description: format!("Allow domain {}", domain),
                    scope: RuleScope::Domain,
                    matcher: RuleMatcher::DomainPattern(domain),
                    effect: RuleEffect::Allow,
                    priority_weight: 20,
                    custom_grace_seconds: None,
                    is_enabled: true,
                    expires_at: None,
                });
            }

            let db_path = default_database_path();
            let (mut wf_repo, session_repo, session_repo_clone): (
                Box<dyn WorkflowRepository>,
                Box<dyn SessionRepository>,
                Box<dyn SessionRepository>,
            ) = match SqliteDb::open(&db_path) {
                Ok(db) => {
                    if cli.verbose {
                        println!(
                            "[info] Connected to SQLite database at: {}",
                            db_path.display()
                        );
                    }
                    let wf_r = SqliteWorkflowRepository::new(db.clone());
                    let sess_r = SqliteSessionRepository::new(db.clone());
                    let sess_clone = SqliteSessionRepository::new(db);
                    (Box::new(wf_r), Box::new(sess_r), Box::new(sess_clone))
                }
                Err(e) => {
                    eprintln!(
                        "[warn] Failed to open SQLite database at '{}': {}. Falling back to in-memory.",
                        db_path.display(),
                        e
                    );
                    let wf_r = InMemoryWorkflowRepository::new();
                    let sess_r = InMemorySessionRepository::new();
                    let sess_clone = sess_r.clone();
                    (Box::new(wf_r), Box::new(sess_r), Box::new(sess_clone))
                }
            };

            if let Err(e) = wf_repo.save(&wf) {
                eprintln!("Failed to register workflow: {}", e);
                std::process::exit(1);
            }

            let source = create_activity_source(&config);
            let mut wf_service = WorkflowService::new(
                source,
                wf_repo,
                session_repo,
                config.activity_source.activitywatch.poll_interval_ms,
            );

            let started_session = match wf_service.start_session(&wf_id) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to activate workflow session: {}", e);
                    std::process::exit(1);
                }
            };

            let running = Arc::new(AtomicBool::new(true));
            let r = running.clone();
            if let Err(e) = ctrlc::set_handler(move || {
                r.store(false, Ordering::SeqCst);
            }) {
                eprintln!("Warning: Failed to set Ctrl+C signal handler: {}", e);
            }

            let result = run_interactive_session(
                &mut wf_service,
                started_session,
                &workflow,
                effective_duration,
                running,
                cli.verbose,
            );

            match result {
                Ok(final_session) => {
                    let drifts = session_repo_clone
                        .list_drifts(&final_session.id)
                        .unwrap_or_default();
                    print_session_summary(&final_session, &drifts);
                }
                Err(e) => {
                    eprintln!("Workflow session ended with error: {}", e);
                }
            }
        }
        Some(Commands::History { limit }) => {
            use flowguard::application::ports::SessionRepository;

            let db_path = default_database_path();
            let session_repo: Box<dyn SessionRepository> = match SqliteDb::open(&db_path) {
                Ok(db) => Box::new(SqliteSessionRepository::new(db)),
                Err(e) => {
                    eprintln!(
                        "Failed to open SQLite database at '{}': {}",
                        db_path.display(),
                        e
                    );
                    std::process::exit(1);
                }
            };

            let sessions = match session_repo.list_recent(limit) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to retrieve session history: {}", e);
                    std::process::exit(1);
                }
            };

            if sessions.is_empty() {
                println!();
                println!(
                    "No historical focus sessions found in database ({})",
                    db_path.display()
                );
                println!("Start your first focus session with: flowguard session");
                println!();
                return;
            }

            println!();
            println!("=========================================================================================================");
            println!("                                   FLOWGUARD FOCUS SESSION HISTORY                                       ");
            println!("=========================================================================================================");
            println!(
                "{:<26} | {:<16} | {:<10} | {:<12} | {:<7} | Pull-Backs",
                "Session ID", "Workflow", "Status", "Focus Time", "Score"
            );
            println!("---------------------------------------------------------------------------------------------------------");
            for s in sessions {
                let total =
                    s.total_focus_seconds + s.total_grace_seconds + s.total_distraction_seconds;
                let score_str = if total > 0 {
                    format!(
                        "{:.1}%",
                        (s.total_focus_seconds as f64 / total as f64) * 100.0
                    )
                } else {
                    "100%".to_string()
                };
                let exit_note = if let Some(r) = &s.early_exit_reason {
                    format!(" (Reason: \"{}\")", r)
                } else {
                    String::new()
                };
                println!(
                    "{:<26} | {:<16} | {:<10} | {:<12} | {:<7} | {}{}",
                    s.id.0,
                    s.workflow_id.0,
                    format!("{:?}", s.state),
                    format_duration_natural(s.total_focus_seconds),
                    score_str,
                    s.pull_back_count,
                    exit_note,
                );
            }
            println!("=========================================================================================================");
            println!();
        }
    }
}
