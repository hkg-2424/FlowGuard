pub mod api;

use chrono::Utc;
use tracing::info;

use crate::application::ports::{ActivitySource, SourceError};
use crate::domain::{
    ActivityCapabilities, ActivityObservation, DiagnosticReport, FreshnessState, IdleState,
    SourceCapabilities, SourceHealth, SubsystemDiagnostic, SubsystemStatus,
};

/// Default AFK inactivity timeout in seconds (180s = 3 minutes).
pub const DEFAULT_AFK_TIMEOUT_SECS: u64 = 180;

/// Native Windows activity observer implementing the `ActivitySource` trait.
///
/// Directly queries Win32 subsystem APIs (`GetForegroundWindow`, `GetWindowTextW`,
/// `GetWindowThreadProcessId`, `QueryFullProcessImageNameW`, and `GetLastInputInfo`).
///
/// Requires zero external services, background daemons, or browser extensions,
/// allowing novice users to run FlowGuard immediately out of the box.
pub struct NativeWindowsAdapter {
    afk_timeout_secs: u64,
    health: SourceHealth,
}

impl NativeWindowsAdapter {
    /// Creates a new `NativeWindowsAdapter` with a configured AFK timeout in seconds.
    pub fn new(afk_timeout_secs: u64) -> Self {
        Self {
            afk_timeout_secs: if afk_timeout_secs == 0 {
                DEFAULT_AFK_TIMEOUT_SECS
            } else {
                afk_timeout_secs
            },
            health: SourceHealth::Connecting,
        }
    }

    /// Creates an adapter with the default 180s (3 minute) AFK timeout.
    pub fn default_adapter() -> Self {
        Self::new(DEFAULT_AFK_TIMEOUT_SECS)
    }

    /// Returns the active capabilities supported by native Windows observation.
    pub fn native_capabilities() -> ActivityCapabilities {
        #[cfg(target_os = "windows")]
        {
            ActivityCapabilities {
                application_tracking: true,
                window_title_tracking: true,
                idle_tracking: true,
                browser_url_tracking: false,
                incognito_detection: false,
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            ActivityCapabilities {
                application_tracking: false,
                window_title_tracking: false,
                idle_tracking: false,
                browser_url_tracking: false,
                incognito_detection: false,
            }
        }
    }
}

impl Default for NativeWindowsAdapter {
    fn default() -> Self {
        Self::default_adapter()
    }
}

impl ActivitySource for NativeWindowsAdapter {
    fn connect(&mut self) -> Result<SourceCapabilities, SourceError> {
        #[cfg(not(target_os = "windows"))]
        {
            self.health = SourceHealth::Disconnected;
            return Err(SourceError::Other(
                "NativeWindowsAdapter is only supported on Windows operating systems".to_string(),
            ));
        }

        #[cfg(target_os = "windows")]
        {
            let (app, title) = api::get_foreground_window_info();
            let (idle_state, _) = api::get_idle_state(self.afk_timeout_secs);

            let idle_ok = idle_state != IdleState::Unknown;
            let window_ok =
                app.is_some() || title.is_some() || api::is_window_subsystem_available();

            if !idle_ok && !window_ok {
                self.health = SourceHealth::Disconnected;
                return Err(SourceError::Other(
                    "Failed to probe Win32 subsystem APIs (neither window nor idle observation available)".to_string(),
                ));
            }

            self.health = if idle_ok && (app.is_some() || title.is_some() || window_ok) {
                SourceHealth::Connected
            } else {
                SourceHealth::Degraded
            };

            info!(
                "Connected to Native Windows Activity Observer (Win32 Subsystem, health: {:?})",
                self.health
            );

            let caps = ActivityCapabilities {
                application_tracking: window_ok,
                window_title_tracking: window_ok,
                idle_tracking: idle_ok,
                browser_url_tracking: false,
                incognito_detection: false,
            };

            Ok(SourceCapabilities {
                provider: "native_windows".to_string(),
                server_version: Some(api::get_os_environment_info()),
                hostname: api::get_local_hostname(),
                is_testing: false,
                capabilities: caps,
            })
        }
    }

    fn poll(&mut self) -> Result<ActivityObservation, SourceError> {
        #[cfg(not(target_os = "windows"))]
        {
            self.health = SourceHealth::Disconnected;
            return Ok(ActivityObservation::unreachable(
                "native_windows",
                "NativeWindowsAdapter is only supported on Windows".to_string(),
            ));
        }

        #[cfg(target_os = "windows")]
        {
            let window_ok = api::is_window_subsystem_available();
            let (application, window_title) = api::get_foreground_window_info();
            let (idle_state, _) = api::get_idle_state(self.afk_timeout_secs);

            let idle_ok = idle_state != IdleState::Unknown;

            let window_freshness = if window_ok {
                FreshnessState::Fresh
            } else {
                FreshnessState::Missing
            };

            let idle_freshness = if idle_ok {
                FreshnessState::Fresh
            } else {
                FreshnessState::Missing
            };

            let health = match (window_freshness, idle_freshness) {
                (FreshnessState::Fresh, FreshnessState::Fresh) => SourceHealth::Connected,
                (FreshnessState::Fresh, _) | (_, FreshnessState::Fresh) => SourceHealth::Degraded,
                _ => SourceHealth::Disconnected,
            };
            self.health = health;

            let caps = ActivityCapabilities {
                application_tracking: window_ok,
                window_title_tracking: window_ok,
                idle_tracking: idle_ok,
                browser_url_tracking: false,
                incognito_detection: false,
            };

            let mut diagnostics = Vec::new();
            if !window_ok {
                diagnostics.push("Win32 window observation subsystem unavailable.".to_string());
            } else if application.is_none() && window_title.is_none() {
                diagnostics.push("No active foreground window observed (desktop, lock screen, or protected window).".to_string());
            }
            if !idle_ok {
                diagnostics.push("User idle acquisition failed via GetLastInputInfo.".to_string());
            }

            Ok(ActivityObservation {
                observed_at: Utc::now(),
                application,
                window_title,
                browser: None,
                idle_state,
                capabilities: caps,
                source_health: self.health,
                window_freshness,
                idle_freshness,
                browser_freshness: FreshnessState::Missing,
                diagnostics,
            })
        }
    }

    fn health(&self) -> SourceHealth {
        self.health
    }

    fn diagnose(&mut self) -> Result<DiagnosticReport, SourceError> {
        #[cfg(not(target_os = "windows"))]
        {
            self.health = SourceHealth::Disconnected;
            return Ok(DiagnosticReport::unreachable(
                "NativeWindows",
                "Win32 Subsystem",
                SourceHealth::Disconnected,
                "NativeWindows adapter is only supported on Windows operating systems.".to_string(),
            ));
        }

        #[cfg(target_os = "windows")]
        {
            let window_ok = api::is_window_subsystem_available();
            let (current_app, current_title) = api::get_foreground_window_info();
            let (idle_state, idle_secs) = api::get_idle_state(self.afk_timeout_secs);
            let idle_s = idle_secs.unwrap_or(0);
            let idle_ok = idle_state != IdleState::Unknown;

            let (window_status, window_freshness, window_details) = if !window_ok {
                (
                    SubsystemStatus::Unavailable,
                    FreshnessState::Missing,
                    "Win32 window observation subsystem unavailable".to_string(),
                )
            } else {
                match (&current_app, &current_title) {
                    (Some(app), Some(title)) => (
                        SubsystemStatus::Healthy,
                        FreshnessState::Fresh,
                        format!("Foreground active: [{}] \"{}\"", app, title),
                    ),
                    (Some(app), None) => (
                        SubsystemStatus::Healthy,
                        FreshnessState::Fresh,
                        format!("Foreground active: [{}] (no window title)", app),
                    ),
                    (None, _) => (
                        SubsystemStatus::Healthy,
                        FreshnessState::Fresh,
                        "Foreground window idle or absent (desktop, lock screen, or protected window)".to_string(),
                    ),
                }
            };

            let (idle_status, idle_freshness, idle_details) = if idle_ok {
                (
                    SubsystemStatus::Healthy,
                    FreshnessState::Fresh,
                    format!(
                        "Tick counter active. Current idle duration: {}s (State: {:?}, AFK threshold: {}s)",
                        idle_s, idle_state, self.afk_timeout_secs
                    ),
                )
            } else {
                (
                    SubsystemStatus::Degraded,
                    FreshnessState::Missing,
                    "GetLastInputInfo failed or unavailable".to_string(),
                )
            };

            let subsystems = vec![
                SubsystemDiagnostic {
                    name: "Active Window Poller".to_string(),
                    target: Some("GetForegroundWindow / QueryFullProcessImageNameW".to_string()),
                    status: window_status,
                    freshness: Some(window_freshness),
                    age_seconds: Some(0),
                    details: window_details,
                },
                SubsystemDiagnostic {
                    name: "User Idle Detection".to_string(),
                    target: Some("GetLastInputInfo".to_string()),
                    status: idle_status,
                    freshness: Some(idle_freshness),
                    age_seconds: Some(idle_s as i64),
                    details: idle_details,
                },
            ];

            let overall_health = match (window_status, idle_status) {
                (SubsystemStatus::Healthy, SubsystemStatus::Healthy) => SourceHealth::Connected,
                (SubsystemStatus::Healthy, _) | (_, SubsystemStatus::Healthy) => {
                    SourceHealth::Degraded
                }
                _ => SourceHealth::Disconnected,
            };

            self.health = overall_health;

            let derived_caps = ActivityCapabilities {
                application_tracking: window_ok,
                window_title_tracking: window_ok,
                idle_tracking: idle_ok,
                browser_url_tracking: false,
                incognito_detection: false,
            };

            let mut diagnostics = Vec::new();
            if !window_ok {
                diagnostics.push("Window watcher: subsystem unavailable.".to_string());
            } else if current_app.is_none() && current_title.is_none() {
                diagnostics
                    .push("Window watcher: foreground window absent or protected.".to_string());
            }
            if !idle_ok {
                diagnostics.push("Idle watcher: GetLastInputInfo unavailable.".to_string());
            }

            Ok(DiagnosticReport {
                provider_name: "NativeWindows".to_string(),
                endpoint: "Win32 Subsystem (User32 / Kernel32)".to_string(),
                is_reachable: window_ok || idle_ok,
                service_identity: Some("NativeWindows".to_string()),
                server_version: Some(api::get_os_environment_info()),
                hostname: api::get_local_hostname(),
                is_testing: false,
                server_profile: None,
                subsystems,
                capabilities: derived_caps,
                health: overall_health,
                diagnostics,
            })
        }
    }
}
