use serde::{Deserialize, Serialize};

use super::activity::FreshnessState;
use super::capabilities::ActivityCapabilities;
use super::source_health::SourceHealth;

/// Operational status of an individual observation subsystem or watcher channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubsystemStatus {
    /// Subsystem is active, communicating, and producing fresh observations.
    Healthy,
    /// Subsystem is reachable, but observations are stale, degraded, or timing out.
    Degraded,
    /// Subsystem is not installed, bucket not found, or feature unavailable.
    Unavailable,
}

impl SubsystemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SubsystemStatus::Healthy => "Healthy / Fresh",
            SubsystemStatus::Degraded => "Degraded / Stale",
            SubsystemStatus::Unavailable => "Unavailable",
        }
    }
}

/// Diagnostic metadata for an individual observation subsystem or watcher channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubsystemDiagnostic {
    /// Logical name of the subsystem (e.g., "Active Window", "AFK / Idle Detection", "Browser Extension").
    pub name: String,
    /// Concrete target or identifier (e.g., bucket ID or Win32 hook descriptor).
    pub target: Option<String>,
    /// Operational status of this subsystem channel.
    pub status: SubsystemStatus,
    /// Observation freshness if applicable.
    pub freshness: Option<FreshnessState>,
    /// Observation age in seconds if applicable.
    pub age_seconds: Option<i64>,
    /// Human-readable explanation or error details.
    pub details: String,
}

/// Provider-neutral diagnostic report produced by `ActivitySource::diagnose()`
/// and consumed by the doctor command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticReport {
    /// Identifier of the activity provider (e.g., "ActivityWatch", "NativeWindows").
    pub provider_name: String,
    /// Connection endpoint or target (e.g., "http://127.0.0.1:5600", "Win32 Subsystem").
    pub endpoint: String,
    /// Whether the underlying service or subsystem is reachable.
    pub is_reachable: bool,
    /// Provider/service identity string if verified (e.g., "ActivityWatch").
    pub service_identity: Option<String>,
    /// Remote or native host server version (if available).
    pub server_version: Option<String>,
    /// Hostname where the source is running (if available).
    pub hostname: Option<String>,
    /// Whether the server is in testing/development mode.
    pub is_testing: bool,
    /// Server profile name, if reported.
    pub server_profile: Option<String>,
    /// Diagnostic details for each observation channel/subsystem.
    pub subsystems: Vec<SubsystemDiagnostic>,
    /// Capabilities verified for this provider session.
    pub capabilities: ActivityCapabilities,
    /// Overall health classification of the source.
    pub health: SourceHealth,
    /// Actionable diagnostics, troubleshooting advice, or ambiguity warnings.
    pub diagnostics: Vec<String>,
}

impl DiagnosticReport {
    /// Creates a fallback diagnostic report when an activity source is completely unreachable.
    pub fn unreachable(
        provider_name: impl Into<String>,
        endpoint: impl Into<String>,
        health: SourceHealth,
        error_message: String,
    ) -> Self {
        Self {
            provider_name: provider_name.into(),
            endpoint: endpoint.into(),
            is_reachable: false,
            service_identity: None,
            server_version: None,
            hostname: None,
            is_testing: false,
            server_profile: None,
            subsystems: Vec::new(),
            capabilities: ActivityCapabilities::none(),
            health,
            diagnostics: vec![error_message],
        }
    }
}
