use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::capabilities::ActivityCapabilities;
use super::source_health::SourceHealth;

/// Indicates whether data from an observation source is fresh, stale, or missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreshnessState {
    /// Observation was recorded within the configured freshness threshold.
    Fresh,
    /// Observation exists, but its timestamp exceeds the staleness threshold.
    Stale,
    /// Observation is missing because the corresponding watcher or bucket does not exist.
    Missing,
}

impl FreshnessState {
    pub fn is_usable(&self) -> bool {
        matches!(self, FreshnessState::Fresh)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            FreshnessState::Fresh => "Fresh",
            FreshnessState::Stale => "Stale",
            FreshnessState::Missing => "Missing",
        }
    }
}

/// Normalized user idle/activity state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdleState {
    /// User is actively interacting with the machine (keyboard/mouse).
    Active,
    /// User is away from keyboard / inactive.
    Afk,
    /// Idle status cannot be determined (watcher missing, stopped, or disconnected).
    Unknown,
}

impl IdleState {
    pub fn as_str(&self) -> &'static str {
        match self {
            IdleState::Active => "Active",
            IdleState::Afk => "AFK",
            IdleState::Unknown => "Unknown",
        }
    }
}

/// Normalized browser context provided by a browser extension watcher.
///
/// If detailed browser tracking is unavailable, this context will be `None`,
/// and FlowGuard will fall back to application and window title observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserContext {
    /// Full or normalized URL of the active browser tab.
    pub url: Option<String>,
    /// Web page title reported by the browser watcher.
    pub title: Option<String>,
    /// Whether audio is currently playing in this tab.
    pub audible: Option<bool>,
    /// Whether this tab is in an incognito / private browsing window.
    pub incognito: Option<bool>,
}

/// Normalized snapshot of user activity across all observed sources.
///
/// FlowGuard interacts exclusively with this domain model, insulating all downstream
/// modules (workflow engine, analytics, interventions) from external source schemas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityObservation {
    /// Timestamp when this snapshot was normalized by FlowGuard.
    pub observed_at: DateTime<Utc>,
    /// Name of the active process / application executable (e.g. "chrome.exe", "code.exe").
    pub application: Option<String>,
    /// Title of the active desktop window.
    pub window_title: Option<String>,
    /// Browser-specific tab context (only populated when a web watcher is active and fresh).
    pub browser: Option<BrowserContext>,
    /// Normalized user idle state.
    pub idle_state: IdleState,
    /// Capabilities dynamically available when this observation was produced.
    pub capabilities: ActivityCapabilities,
    /// Source operational health at the time of observation.
    pub source_health: SourceHealth,
    /// Freshness of window watcher data.
    pub window_freshness: FreshnessState,
    /// Freshness of AFK watcher data.
    pub idle_freshness: FreshnessState,
    /// Freshness of web browser watcher data.
    pub browser_freshness: FreshnessState,
    /// Informational warnings or diagnostics for this polling cycle.
    pub diagnostics: Vec<String>,
}

impl ActivityObservation {
    /// Produces a disconnected fallback observation.
    pub fn disconnected(health: SourceHealth, diagnostics: Vec<String>) -> Self {
        Self {
            observed_at: Utc::now(),
            application: None,
            window_title: None,
            browser: None,
            idle_state: IdleState::Unknown,
            capabilities: ActivityCapabilities::none(),
            source_health: health,
            window_freshness: FreshnessState::Missing,
            idle_freshness: FreshnessState::Missing,
            browser_freshness: FreshnessState::Missing,
            diagnostics,
        }
    }

    /// Determines if there is a meaningful state change between this observation
    /// and a previous observation. This prevents spamming terminal output or downstream
    /// event buses when nothing has changed.
    ///
    /// # Checked Properties:
    /// - Active application name
    /// - Active window title
    /// - Idle status (Active, Afk, Unknown)
    /// - Operational source health (Healthy, Degraded, Disconnected)
    /// - Freshness of window, AFK/idle, and browser watcher signals
    /// - Structural capabilities
    /// - Browser context (URL, tab title, audio, incognito)
    /// - Active diagnostic messages
    pub fn has_meaningful_change(&self, prev: &ActivityObservation) -> bool {
        self.application != prev.application
            || self.window_title != prev.window_title
            || self.idle_state != prev.idle_state
            || self.source_health != prev.source_health
            || self.window_freshness != prev.window_freshness
            || self.idle_freshness != prev.idle_freshness
            || self.browser_freshness != prev.browser_freshness
            || self.capabilities != prev.capabilities
            || self.browser != prev.browser
            || self.diagnostics != prev.diagnostics
    }
}
