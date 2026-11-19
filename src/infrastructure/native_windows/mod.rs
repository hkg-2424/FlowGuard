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
        ActivityCapabilities {
            application_tracking: true,
            window_title_tracking: true,
            idle_tracking: true,
            browser_url_tracking: false,
            incognito_detection: false,
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
        self.health = SourceHealth::Connected;
        info!("Connected to Native Windows Activity Observer (Win32 Subsystem)");

        Ok(SourceCapabilities {
            provider: "native_windows".to_string(),
            server_version: Some(api::get_os_environment_info()),
            hostname: api::get_local_hostname(),
            is_testing: false,
            capabilities: Self::native_capabilities(),
        })
    }

    fn poll(&mut self) -> Result<ActivityObservation, SourceError> {
        self.health = SourceHealth::Connected;

        let (application, window_title) = api::get_foreground_window_info();
        let (idle_state, _) = api::get_idle_state(self.afk_timeout_secs);

        let idle_freshness = if idle_state != IdleState::Unknown {
            FreshnessState::Fresh
        } else {
            FreshnessState::Missing
        };

        Ok(ActivityObservation {
            observed_at: Utc::now(),
            application,
            window_title,
            browser: None,
            idle_state,
            capabilities: Self::native_capabilities(),
            source_health: self.health,
            window_freshness: FreshnessState::Fresh,
            idle_freshness,
            browser_freshness: FreshnessState::Missing,
            diagnostics: Vec::new(),
        })
    }

    fn health(&self) -> SourceHealth {
        self.health
    }

    fn diagnose(&mut self) -> Result<DiagnosticReport, SourceError> {
        let (current_app, current_title) = api::get_foreground_window_info();
        let (idle_state, idle_secs) = api::get_idle_state(self.afk_timeout_secs);
        let idle_s = idle_secs.unwrap_or(0);

        let window_details = match (&current_app, &current_title) {
            (Some(app), Some(title)) => format!("Foreground active: [{}] \"{}\"", app, title),
            (Some(app), None) => format!("Foreground active: [{}] (no window title)", app),
            (None, _) => "No foreground window active (desktop or lock screen)".to_string(),
        };

        let idle_details = format!(
            "Tick counter active. Current idle duration: {}s (State: {:?}, AFK threshold: {}s)",
            idle_s, idle_state, self.afk_timeout_secs
        );

        let subsystems = vec![
            SubsystemDiagnostic {
                name: "Active Window Poller".to_string(),
                target: Some("GetForegroundWindow / QueryFullProcessImageNameW".to_string()),
                status: SubsystemStatus::Healthy,
                freshness: Some(FreshnessState::Fresh),
                age_seconds: Some(0),
                details: window_details,
            },
            SubsystemDiagnostic {
                name: "User Idle Detection".to_string(),
                target: Some("GetLastInputInfo".to_string()),
                status: SubsystemStatus::Healthy,
                freshness: Some(FreshnessState::Fresh),
                age_seconds: Some(idle_s as i64),
                details: idle_details,
            },
        ];

        self.health = SourceHealth::Connected;

        Ok(DiagnosticReport {
            provider_name: "NativeWindows".to_string(),
            endpoint: "Win32 Subsystem (User32 / Kernel32)".to_string(),
            is_reachable: true,
            service_identity: Some("NativeWindows".to_string()),
            server_version: Some(api::get_os_environment_info()),
            hostname: api::get_local_hostname(),
            is_testing: false,
            server_profile: None,
            subsystems,
            capabilities: Self::native_capabilities(),
            health: SourceHealth::Connected,
            diagnostics: Vec::new(),
        })
    }
}
