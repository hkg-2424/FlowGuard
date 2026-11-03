use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

use flowguard::application::ActivityService;
use flowguard::config::FlowGuardConfig;
use flowguard::infrastructure::activitywatch::ActivityWatchAdapter;
use flowguard::presentation::console::{
    print_doctor_report, print_observation_change, print_observation_snapshot, print_status,
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

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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

    // Build ActivitySource adapter (ActivityWatch for M1)
    let adapter = ActivityWatchAdapter::new(config.activity_source.activitywatch.clone());
    let mut service = ActivityService::new(Box::new(adapter), config.clone());

    match cli.command {
        Commands::Doctor => {
            let report = service.run_doctor();
            print_doctor_report(&report, cli.verbose);
            if !report.is_reachable {
                std::process::exit(1);
            }
        }
        Commands::Status => {
            let (health, caps) = service.status();
            print_status(
                health,
                &caps,
                &config.activity_source.activitywatch.base_url,
            );
        }
        Commands::Observe { once } => {
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
    }
}
