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
}

impl AutoSourceAdapter {
    /// Creates a new `AutoSourceAdapter` with ActivityWatch as primary and Native Windows as fallback.
    pub fn new(aw_config: ActivityWatchConfig) -> Self {
        let afk_timeout = aw_config.afk_timeout_secs;
        Self {
            activitywatch: ActivityWatchAdapter::new(aw_config),
            native: NativeWindowsAdapter::new(afk_timeout),
            active_backend: ActiveBackend::ActivityWatch,
            fallback_active: false,
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
                Ok(caps)
            }
            Err(err) => {
                info!(
                    "ActivityWatch unavailable on connect ({}). Falling back to Native Windows observer.",
                    err
                );
                self.active_backend = ActiveBackend::NativeWindows;
                self.fallback_active = true;

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
                    let mut native_obs = self.native.poll()?;
                    native_obs.diagnostics.push(format!(
                        "ActivityWatch error ({}). Seamlessly operating via Native Windows observer.",
                        err
                    ));
                    Ok(native_obs)
                }
            }
        } else {
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
            Ok(report) if report.is_reachable => {
                self.active_backend = ActiveBackend::ActivityWatch;
                self.fallback_active = false;
                Ok(report)
            }
            Ok(unreachable_report) => {
                // ActivityWatch unreachable — diagnose native windows
                self.active_backend = ActiveBackend::NativeWindows;
                self.fallback_active = true;

                let mut native_report = self.native.diagnose()?;
                native_report.diagnostics.push(format!(
                    "ActivityWatch is not running at {} (connection refused or stopped). FlowGuard has seamlessly fallen back to the Native Windows observer (Win32 APIs).",
                    unreachable_report.endpoint
                ));
                Ok(native_report)
            }
            Err(err) => {
                self.active_backend = ActiveBackend::NativeWindows;
                self.fallback_active = true;

                let mut native_report = self.native.diagnose()?;
                native_report.diagnostics.push(format!(
                    "ActivityWatch diagnostic error ({}). Operating via Native Windows observer.",
                    err
                ));
                Ok(native_report)
            }
        }
    }
}
