use chrono::Utc;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::application::workflow_service::{
    SessionEvaluation, WorkflowService, WorkflowServiceError,
};
use crate::domain::workflow::{
    CommitmentContract, DriftResolution, PolicyDecision, SessionError, WorkflowSession,
};
use crate::presentation::truncate::truncate_str;

/// Formats seconds into human-readable `HH:MM:SS` or `MM:SS`.
pub fn format_time_hms(total_seconds: u64) -> String {
    let hours = total_seconds / 3600;
    let mins = (total_seconds % 3600) / 60;
    let secs = total_seconds % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}

/// Formats seconds into natural phrase like `42m 15s`.
pub fn format_duration_natural(total_seconds: u64) -> String {
    let hours = total_seconds / 3600;
    let mins = (total_seconds % 3600) / 60;
    let secs = total_seconds % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, mins, secs)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs)
    } else {
        format!("{}s", secs)
    }
}

/// Displays the terminal dashboard header for a new workflow session.
pub fn print_session_start_banner(
    workflow_name: &str,
    contract: &CommitmentContract,
    target_duration_mins: Option<u32>,
) {
    println!();
    println!("================================================================================");
    println!("                    FLOWGUARD FOCUS SESSION ACTIVATED                           ");
    println!("================================================================================");
    println!("  Workflow:             {}", workflow_name);
    let contract_desc = match contract {
        CommitmentContract::Standard => "Standard (Flexible exit)".to_string(),
        CommitmentContract::AccountableBreakable {
            min_reason_characters,
        } => {
            format!(
                "Accountable Breakable (Requires >={} character written reflection to cancel)",
                min_reason_characters
            )
        }
        CommitmentContract::Unbreakable {
            target_duration_seconds,
        } => {
            format!(
                "Unbreakable / Ulysses Lock (Enforced for {})",
                format_duration_natural(*target_duration_seconds)
            )
        }
    };
    println!("  Commitment Contract:  {}", contract_desc);
    if let Some(target) = target_duration_mins {
        println!("  Target Duration:      {} minutes", target);
    }
    println!("  Grace Buffer:         30 seconds (pull-back resilience active)");
    println!("  Controls:             Press Ctrl+C to request session exit");
    println!("================================================================================");
    println!();
}

/// Renders a single evaluation tick on the terminal console.
pub fn print_session_tick(eval: &SessionEvaluation, target_seconds: Option<u64>, verbose: bool) {
    let elapsed = eval.session.duration_seconds(Utc::now());
    let elapsed_str = format_time_hms(elapsed);
    let target_str = target_seconds
        .map(format_time_hms)
        .unwrap_or_else(|| "--:--".into());

    let app = eval.observation.application.as_deref().unwrap_or("unknown");
    let title = eval.observation.window_title.as_deref().unwrap_or("");
    let app_display = truncate_str(app, 20);
    let title_display = truncate_str(title, if verbose { 50 } else { 32 });

    let grace_note;
    let (badge, color_desc): (&str, &str) = match &eval.decision {
        PolicyDecision::Allowed { .. } => ("[ALIGNED]", "On planned task"),
        PolicyDecision::WarningRequired { reason, .. } => ("[WARN]", reason.as_str()),
        PolicyDecision::GracePeriod {
            remaining_seconds, ..
        } => {
            grace_note = format!(
                "OUTSIDE WORKFLOW - Grace period: {}s remaining",
                remaining_seconds
            );
            ("[GRACE]", grace_note.as_str())
        }
        PolicyDecision::InterventionRequired { .. } => (
            "[INTERVENTION REQUIRED]",
            "Grace period expired! Return to workflow!",
        ),
        PolicyDecision::Ignored { reason } => ("[IDLE/SURFACE]", reason.as_str()),
        PolicyDecision::ObservationUnavailable { reason } => ("[UNAVAILABLE]", reason.as_str()),
        PolicyDecision::WorkflowInactive => ("[INACTIVE]", "No active workflow"),
    };

    println!(
        "[{}/{}] {:<10} | {:<16} | {:<24} | {}",
        elapsed_str, target_str, badge, app_display, title_display, color_desc
    );

    // If this tick resolved a focus pull-back, celebrate it prominently!
    if let Some(drift) = &eval.pull_back_event {
        println!();
        println!("  ******************************************************************");
        println!(
            "  *  FOCUS PULL-BACK REGISTERED! Recovered in {}s from '{}'",
            drift.duration_seconds, drift.target_name
        );
        println!(
            "  *  Self-Correction Resilience Streak: {} pull-back(s) this session! *",
            eval.session.pull_back_count
        );
        println!("  ******************************************************************");
        println!();
    }

    io::stdout().flush().ok();
}

/// Prints the comprehensive focus analytics summary upon session completion or exit.
pub fn print_session_summary(
    session: &WorkflowSession,
    drifts: &[crate::domain::workflow::DriftEvent],
) {
    let total_active = session.total_focus_seconds
        + session.total_grace_seconds
        + session.total_distraction_seconds;

    let focus_ratio = if total_active > 0 {
        (session.total_focus_seconds as f64 / total_active as f64) * 100.0
    } else {
        100.0
    };

    let total_session_time =
        total_active + session.total_afk_seconds + session.total_paused_seconds;

    println!();
    println!("================================================================================");
    println!("                      FLOWGUARD SESSION ANALYTICS CARD                          ");
    println!("================================================================================");
    println!("  Session ID:             {}", session.id);
    println!("  Workflow ID:            {}", session.workflow_id);
    println!("  Final Status:           {:?}", session.state);
    if let Some(reason) = &session.early_exit_reason {
        println!("  Written Exit Reason:    \"{}\"", reason);
    }
    println!(
        "  Total Dwell Time:       {}",
        format_duration_natural(total_session_time)
    );
    println!("--------------------------------------------------------------------------------");
    println!(
        "  Productive Focus Time:  {:<10} ({:.1}%)",
        format_duration_natural(session.total_focus_seconds),
        focus_ratio
    );
    println!(
        "  Grace Period Buffer:    {:<10}",
        format_duration_natural(session.total_grace_seconds)
    );
    println!(
        "  Distraction Duration:   {:<10}",
        format_duration_natural(session.total_distraction_seconds)
    );
    println!(
        "  AFK / Thinking Time:    {:<10}",
        format_duration_natural(session.total_afk_seconds)
    );
    if session.total_paused_seconds > 0 {
        println!(
            "  Paused Duration:        {:<10}",
            format_duration_natural(session.total_paused_seconds)
        );
    }
    println!("--------------------------------------------------------------------------------");
    println!(
        "  Focus Pull-Backs:       {} successful self-corrections",
        session.pull_back_count
    );
    println!(
        "  Interventions Fired:    {} on-screen alerts",
        session.intervention_count
    );

    if !drifts.is_empty() {
        println!();
        println!(
            "  Focus Drift History (Recent {} events):",
            drifts.len().min(5)
        );
        for (i, d) in drifts.iter().rev().take(5).enumerate() {
            let res_str = match &d.resolution {
                DriftResolution::SelfCorrected { recovery_seconds } => {
                    format!("Self-corrected in {}s", recovery_seconds)
                }
                DriftResolution::Intervened => "Prompted after 30s grace".to_string(),
            };
            println!(
                "    {}. Target: {:<20} | {}",
                i + 1,
                truncate_str(&d.target_name, 20),
                res_str
            );
        }
    }

    println!("================================================================================");
    println!();
}

/// Runs an interactive session runner on the terminal, handling live evaluations,
/// 30-second grace alerts, pull-back celebrations, and commitment contract enforcement.
pub fn run_interactive_session(
    service: &mut WorkflowService,
    session: WorkflowSession,
    workflow_name: &str,
    target_duration_mins: Option<u32>,
    running: Arc<AtomicBool>,
    verbose: bool,
) -> Result<WorkflowSession, WorkflowServiceError> {
    print_session_start_banner(
        workflow_name,
        &session.commitment_contract,
        target_duration_mins,
    );

    let target_seconds = target_duration_mins.map(|m| m as u64 * 60);
    let mut last_status_decision: Option<PolicyDecision> = None;

    let slice = Duration::from_millis(50);
    let poll_interval = Duration::from_millis(1000);

    loop {
        while running.load(Ordering::SeqCst) {
            match service.evaluate_tick() {
                Ok(eval) => {
                    // If target duration reached, complete session naturally!
                    let elapsed = eval.session.duration_seconds(Utc::now());
                    if let Some(target) = target_seconds {
                        if elapsed >= target {
                            println!();
                            println!("================================================================================");
                            println!(
                                "  CONGRATULATIONS! Target focus duration of {} minutes achieved!",
                                target / 60
                            );
                            println!("================================================================================");
                            return service.complete_session();
                        }
                    }

                    // Print tick if state changed or in grace period or every 5 seconds
                    let should_print = match (&last_status_decision, &eval.decision) {
                        (
                            Some(PolicyDecision::GracePeriod { .. }),
                            PolicyDecision::GracePeriod { .. },
                        ) => true,
                        (Some(prev), curr) => prev != curr,
                        (None, _) => true,
                    };

                    if should_print || elapsed % 5 == 0 {
                        print_session_tick(&eval, target_seconds, verbose);
                        last_status_decision = Some(eval.decision);
                    }
                }
                Err(WorkflowServiceError::Source(err)) => {
                    eprintln!("[warning] Activity observation error: {}", err);
                }
                Err(err) => return Err(err),
            }

            // Sleep in 50ms slices
            let mut remaining = poll_interval;
            while remaining > Duration::ZERO && running.load(Ordering::SeqCst) {
                let step = slice.min(remaining);
                std::thread::sleep(step);
                remaining = remaining.saturating_sub(step);
            }
        }

        // Ctrl+C was pressed! Enforce Commitment Contract rules.
        println!();
        println!(">> Session exit requested via signal.");

        match &session.commitment_contract {
            CommitmentContract::Unbreakable {
                target_duration_seconds,
            } => {
                let elapsed = service
                    .active_session()
                    .map(|s| s.duration_seconds(Utc::now()))
                    .unwrap_or(0);
                if elapsed < *target_duration_seconds {
                    println!(">> CANNOT EXIT: This session is bound by an UNBREAKABLE contract.");
                    println!(
                        ">> Target duration: {}. Elapsed so far: {}.",
                        format_duration_natural(*target_duration_seconds),
                        format_duration_natural(elapsed)
                    );
                    println!(">> Resuming workflow monitoring...");
                    running.store(true, Ordering::SeqCst);
                    continue;
                } else {
                    return service.complete_session();
                }
            }
            CommitmentContract::AccountableBreakable {
                min_reason_characters,
            } => {
                print!(
                    ">> ACCOUNTABLE COMMITMENT: Written reason required to break workflow (min {} chars): ",
                    min_reason_characters
                );
                io::stdout().flush().ok();

                let mut input = String::new();
                if io::stdin().read_line(&mut input).is_ok() && !input.trim().is_empty() {
                    let trimmed = input.trim();
                    match service.cancel_session(Some(trimmed.to_string())) {
                        Ok(cancelled) => return Ok(cancelled),
                        Err(WorkflowServiceError::Session(SessionError::InsufficientReason {
                            min_chars,
                            actual_chars,
                        })) => {
                            println!(
                                ">> Exit rejected: Reason too short ({} chars provided, minimum {} required).",
                                actual_chars, min_chars
                            );
                            println!(">> Resuming workflow monitoring...");
                            running.store(true, Ordering::SeqCst);
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                } else {
                    println!(">> Exit rejected: No reason provided.");
                    println!(">> Resuming workflow monitoring...");
                    running.store(true, Ordering::SeqCst);
                    continue;
                }
            }
            CommitmentContract::Standard => return service.cancel_session(None),
        }
    }
}
