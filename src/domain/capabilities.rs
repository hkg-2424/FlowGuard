use serde::{Deserialize, Serialize};

/// FlowGuard capability model.
///
/// Capabilities reflect what the active activity source can currently observe.
/// These are dynamically determined from available buckets and event data,
/// rather than assuming features exist simply because the source is connected.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActivityCapabilities {
    /// True if the active window's application/process name can be tracked.
    pub application_tracking: bool,
    /// True if the active window's title can be tracked.
    pub window_title_tracking: bool,
    /// True if user idle/AFK status can be tracked.
    pub idle_tracking: bool,
    /// True if web browser active tab URL and title can be tracked.
    pub browser_url_tracking: bool,
    /// True if incognito/private browsing state can be detected.
    pub incognito_detection: bool,
}

impl ActivityCapabilities {
    /// Returns a capabilities set with all features disabled (e.g. when disconnected).
    pub fn none() -> Self {
        Self {
            application_tracking: false,
            window_title_tracking: false,
            idle_tracking: false,
            browser_url_tracking: false,
            incognito_detection: false,
        }
    }
}

/// Metadata describing the connected source and its discovered capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceCapabilities {
    /// Source provider identifier (e.g. "activitywatch" or "native_windows").
    pub provider: String,
    /// Remote or native server version.
    pub server_version: Option<String>,
    /// Hostname where the source is running.
    pub hostname: Option<String>,
    /// Whether the server reports running in a testing configuration.
    pub is_testing: bool,
    /// Dynamic activity capabilities supported by this source session.
    pub capabilities: ActivityCapabilities,
}
