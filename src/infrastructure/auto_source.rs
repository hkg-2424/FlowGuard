use tracing::{debug, info};

use super::activitywatch::ActivityWatchAdapter;
use super::native_windows::NativeWindowsAdapter;
use crate::application::ports::{ActivitySource, SourceError};
use crate::config::ActivityWatchConfig;
use crate::domain::{ActivityObservation, DiagnosticReport, SourceCapabilities, SourceHealth};

/// Active observation backend currently used by `AutoSourceAdapter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveBackend {
    ActivityWatch,
    NativeWindows,
}

/// Automatic activity source adapter that seamlessly falls back to native Windows APIs.
///
/// Tries ActivityWatch first to provide advanced capability detection (e.g. browser URLs
/// when `aw-watcher-web` is installed). If ActivityWatch is unreachable, stopped, uninstalled,
/// or returns connection errors, `AutoSourceAdapter` automatically falls back to
/// `NativeWindowsAdapter` so FlowGuard works immediately out of the box with zero configuration.
pub struct AutoSourceAdapter {
    activitywatch: ActivityWatchAdapter,
    native: NativeWindowsAdapter,
    active_backend: ActiveBackend,
    fallback_active: bool,
    reconnect_interval: std::time::Duration,
    last_recovery_attempt: Option<std::time::Instant>,
}

impl AutoSourceAdapter {
    /// Creates a new `AutoSourceAdapter` with ActivityWatch as primary and Native Windows as fallback.
    pub fn new(aw_config: ActivityWatchConfig) -> Self {
        let reconnect_secs = aw_config.reconnect_interval_secs;
        let afk_timeout = aw_config.afk_timeout_secs;
        Self {
            activitywatch: ActivityWatchAdapter::new(aw_config),
            native: NativeWindowsAdapter::new(afk_timeout),
            active_backend: ActiveBackend::ActivityWatch,
            fallback_active: false,
            reconnect_interval: std::time::Duration::from_secs(reconnect_secs),
            last_recovery_attempt: None,
        }
    }

    /// Returns which backend is currently actively monitoring activity.
    pub fn active_backend(&self) -> ActiveBackend {
        self.active_backend
    }

    /// Returns true if fallback to Native Windows has taken place.
    pub fn is_fallback_active(&self) -> bool {
        self.fallback_active
    }

    /// Access reference to underlying ActivityWatch adapter.
    pub fn activitywatch_adapter(&self) -> &ActivityWatchAdapter {
        &self.activitywatch
    }

    /// Access reference to underlying Native Windows adapter.
    pub fn native_adapter(&self) -> &NativeWindowsAdapter {
        &self.native
    }
}

impl ActivitySource for AutoSourceAdapter {
    fn connect(&mut self) -> Result<SourceCapabilities, SourceError> {
        match self.activitywatch.connect() {
            Ok(caps) => {
                self.active_backend = ActiveBackend::ActivityWatch;
                self.fallback_active = false;
                self.last_recovery_attempt = None;
                Ok(caps)
            }
            Err(err) => {
                // If the error is an authentication error or wrong service, do not silently swallow (H-05)
                match &err {
                    SourceError::AuthRequired(_) | SourceError::WrongService { .. } => {
                        return Err(err);
                    }
                    _ => {}
                }

                info!(
                    "ActivityWatch unavailable on connect ({}). Falling back to Native Windows observer.",
                    err
                );
                self.active_backend = ActiveBackend::NativeWindows;
                self.fallback_active = true;
                self.last_recovery_attempt = Some(std::time::Instant::now());

                let mut caps = self.native.connect()?;
                // Preserve note of fallback in provider string if helpful, or keep clean
                caps.provider = "native_windows (fallback)".to_string();
                Ok(caps)
            }
        }
    }

    fn poll(&mut self) -> Result<ActivityObservation, SourceError> {
        if self.active_backend == ActiveBackend::ActivityWatch {
            match self.activitywatch.poll() {
                Ok(obs) => {
                    if obs.source_health == SourceHealth::Disconnected {
                        // ActivityWatch server reported disconnected
                        debug!("ActivityWatch disconnected during poll. Falling back to Native Windows.");
                        self.active_backend = ActiveBackend::NativeWindows;
                        self.fallback_active = true;
                        self.last_recovery_attempt = Some(std::time::Instant::now());
                        let mut native_obs = self.native.poll()?;
                        native_obs.diagnostics.push(
                            "ActivityWatch disconnected; automatically switched to Native Windows observer."
                                .to_string(),
                        );
                        return Ok(native_obs);
                    }
                    Ok(obs)
                }
                Err(err) => {
                    debug!(
                        "ActivityWatch poll failed ({}). Falling back to Native Windows observer.",
                        err
                    );
                    self.active_backend = ActiveBackend::NativeWindows;
                    self.fallback_active = true;
                    self.last_recovery_attempt = Some(std::time::Instant::now());
                    let mut native_obs = self.native.poll()?;
                    native_obs.diagnostics.push(format!(
                        "ActivityWatch error ({}). Seamlessly operating via Native Windows observer.",
                        err
                    ));
                    Ok(native_obs)
                }
            }
        } else {
            // Periodic recovery check back to ActivityWatch when in fallback mode (H-04)
            if self.fallback_active {
                let should_probe = self
                    .last_recovery_attempt
                    .map(|last| last.elapsed() >= self.reconnect_interval)
                    .unwrap_or(true);

                if should_probe {
                    self.last_recovery_attempt = Some(std::time::Instant::now());
                    match self.activitywatch.poll() {
                        Ok(aw_obs) if aw_obs.source_health.is_operational() => {
                            info!("ActivityWatch has recovered and is operational. Switching active backend back to ActivityWatch.");
                            self.active_backend = ActiveBackend::ActivityWatch;
                            self.fallback_active = false;
                            return Ok(aw_obs);
                        }
                        _ => {
                            debug!("ActivityWatch recovery probe: server still not operational, continuing on Native Windows.");
                        }
                    }
                }
            }

            self.native.poll()
        }
    }

    fn health(&self) -> SourceHealth {
        match self.active_backend {
            ActiveBackend::ActivityWatch => self.activitywatch.health(),
            ActiveBackend::NativeWindows => self.native.health(),
        }
    }

    fn diagnose(&mut self) -> Result<DiagnosticReport, SourceError> {
        // Check if ActivityWatch is reachable
        let aw_report = self.activitywatch.diagnose();

        match aw_report {
            Ok(report) if report.is_reachable && report.health.is_operational() => {
                self.active_backend = ActiveBackend::ActivityWatch;
                self.fallback_active = false;
                Ok(report)
            }
            Ok(unreachable_report) => {
                // ActivityWatch unreachable or wrong service — diagnose native windows
                self.active_backend = ActiveBackend::NativeWindows;
                self.fallback_active = true;

                let mut native_report = self.native.diagnose()?;
                if unreachable_report.health == SourceHealth::WrongService {
                    native_report.diagnostics.push(format!(
                        "ActivityWatch port collision at {}: another service is occupying this port. Operating via Native Windows observer.",
                        unreachable_report.endpoint
                    ));
                } else {
                    native_report.diagnostics.push(format!(
                        "ActivityWatch is not running at {} (connection refused or stopped). FlowGuard has seamlessly fallen back to the Native Windows observer (Win32 APIs).",
                        unreachable_report.endpoint
                    ));
                }
                for d in unreachable_report.diagnostics {
                    if !native_report.diagnostics.contains(&d) {
                        native_report.diagnostics.push(d);
                    }
                }
                Ok(native_report)
            }
            Err(err) => {
                self.active_backend = ActiveBackend::NativeWindows;
                self.fallback_active = true;

                let mut native_report = self.native.diagnose()?;
                match &err {
                    SourceError::AuthRequired(endpoint) => {
                        native_report.diagnostics.push(format!(
                            "ActivityWatch at '{}' requires authentication: {}. Operating via Native Windows observer.",
                            endpoint, err
                        ));
                    }
                    SourceError::WrongService { endpoint, reason } => {
                        native_report.diagnostics.push(format!(
                            "ActivityWatch at '{}' encountered wrong service: {}. Operating via Native Windows observer.",
                            endpoint, reason
                        ));
                    }
                    _ => {
                        native_report.diagnostics.push(format!(
                            "ActivityWatch diagnostic error ({}). Operating via Native Windows observer.",
                            err
                        ));
                    }
                }
                Ok(native_report)
            }
        }
    }
}
