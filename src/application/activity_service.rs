use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use tracing::{info, warn};

use super::ports::{ActivitySource, SourceError};
use crate::config::FlowGuardConfig;
use crate::domain::{ActivityCapabilities, ActivityObservation, DiagnosticReport, SourceHealth};

/// Alias for provider-neutral diagnostic report for backwards compatibility.
pub type DoctorReport = DiagnosticReport;

/// Maximum duration of an individual sleep slice during polling intervals.
/// Guarantees that thread shutdown completes within at most 50 milliseconds
/// of `running` being set to `false`.
pub const SHUTDOWN_LATENCY_MAX_MS: u64 = 50;

/// Sleeps for `duration` in small slices, checking `running` repeatedly to allow
/// near-instantaneous cancellation without blocking the thread for long intervals.
fn interruptible_sleep(duration: Duration, running: &Arc<AtomicBool>) {
    let slice = Duration::from_millis(SHUTDOWN_LATENCY_MAX_MS);
    let mut remaining = duration;
    while remaining > Duration::ZERO && running.load(Ordering::SeqCst) {
        let step = slice.min(remaining);
        sleep(step);
        remaining = remaining.saturating_sub(step);
    }
}

/// Orchestrates activity monitoring and exposes domain services to presentation adapters.
pub struct ActivityService {
    source: Box<dyn ActivitySource>,
    config: FlowGuardConfig,
}

impl ActivityService {
    pub fn new(source: Box<dyn ActivitySource>, config: FlowGuardConfig) -> Self {
        Self { source, config }
    }

    /// Performs deep diagnostics on the activity source and returns a structured report.
    ///
    /// Delegates directly to the underlying `ActivitySource` implementation, preserving
    /// strict provider independence so that both ActivityWatch and future native Windows
    /// monitoring adapters produce structured, provider-neutral reports.
    pub fn run_doctor(&mut self) -> DiagnosticReport {
        match self.source.diagnose() {
            Ok(report) => report,
            Err(e) => {
                let endpoint = self.config.activity_source.activitywatch.base_url.clone();
                let health = match &e {
                    SourceError::WrongService { .. } => SourceHealth::WrongService,
                    _ => SourceHealth::Disconnected,
                };
                DiagnosticReport::unreachable(
                    "ActivityWatch",
                    endpoint,
                    health,
                    format!("Diagnostic failure: {}", e),
                )
            }
        }
    }

    /// Queries source operational status and capabilities.
    pub fn status(&mut self) -> (SourceHealth, ActivityCapabilities) {
        match self.source.poll() {
            Ok(obs) => (obs.source_health, obs.capabilities),
            Err(_) => (self.source.health(), ActivityCapabilities::none()),
        }
    }

    /// Fetches one immediate activity observation.
    pub fn observe_once(&mut self) -> Result<ActivityObservation, SourceError> {
        self.source.poll()
    }

    /// Starts a continuous observation loop.
    ///
    /// Periodically polls the activity source according to `poll_interval_ms`.
    /// When state changes are detected, `on_change` is invoked.
    /// Temporary connection failures trigger bounded backoff retries without exiting.
    /// Gracefully exits when `running` is set to false (e.g. on Ctrl+C).
    pub fn observe<F>(
        &mut self,
        running: Arc<AtomicBool>,
        mut on_change: F,
    ) -> Result<(), SourceError>
    where
        F: FnMut(&ActivityObservation),
    {
        let poll_interval =
            Duration::from_millis(self.config.activity_source.activitywatch.poll_interval_ms);
        let reconnect_interval = Duration::from_secs(
            self.config
                .activity_source
                .activitywatch
                .reconnect_interval_secs,
        );

        let mut previous_observation: Option<ActivityObservation> = None;

        info!("Starting continuous activity observation loop...");

        while running.load(Ordering::SeqCst) {
            match self.source.poll() {
                Ok(obs) => {
                    let should_notify = match &previous_observation {
                        Some(prev) => obs.has_meaningful_change(prev),
                        None => true,
                    };

                    if should_notify {
                        on_change(&obs);
                        previous_observation = Some(obs);
                    }

                    // Wait for the next poll cycle with interruptible sleep (max 50ms shutdown latency)
                    interruptible_sleep(poll_interval, &running);
                }
                Err(err) => {
                    warn!(
                        "Error during observation cycle: {}. Will retry in {}s...",
                        err,
                        reconnect_interval.as_secs()
                    );
                    let disconnected_obs = ActivityObservation::disconnected(
                        SourceHealth::Disconnected,
                        vec![format!("Observation error: {}", err)],
                    );

                    let should_notify = match &previous_observation {
                        Some(prev) => disconnected_obs.has_meaningful_change(prev),
                        None => true,
                    };

                    if should_notify {
                        on_change(&disconnected_obs);
                        previous_observation = Some(disconnected_obs);
                    }

                    // Wait for configured reconnect interval before retrying with interruptible sleep
                    interruptible_sleep(reconnect_interval, &running);
                }
            }
        }

        info!("Observation loop exited gracefully.");
        Ok(())
    }
}
