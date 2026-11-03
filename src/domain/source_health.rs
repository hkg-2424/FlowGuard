use serde::{Deserialize, Serialize};
use std::fmt;

/// Connection and operational health of an activity source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceHealth {
    /// In the process of connecting or reconnecting to the activity source.
    Connecting,
    /// Connected to the source and all essential and optional watchers are healthy and fresh.
    Connected,
    /// Source is reachable, but one or more watchers are missing or producing stale data.
    Degraded,
    /// Cannot reach the source (connection refused, network error, or service stopped).
    Disconnected,
    /// An HTTP endpoint responded, but did not identify itself as the expected activity source.
    WrongService,
}

impl SourceHealth {
    /// True if the source is connected, even if degraded.
    pub fn is_operational(&self) -> bool {
        matches!(self, SourceHealth::Connected | SourceHealth::Degraded)
    }

    /// Short human-readable status badge.
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceHealth::Connecting => "Connecting",
            SourceHealth::Connected => "Connected",
            SourceHealth::Degraded => "Degraded",
            SourceHealth::Disconnected => "Disconnected",
            SourceHealth::WrongService => "Wrong Service",
        }
    }
}

impl fmt::Display for SourceHealth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
