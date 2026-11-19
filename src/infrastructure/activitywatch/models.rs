use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Payload returned by `GET /api/0/info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Hostname of the machine running the ActivityWatch server.
    pub hostname: String,
    /// ActivityWatch server semantic version (e.g. "v0.14.1").
    pub version: String,
    /// Indicates whether the server is running in testing/dev mode.
    #[serde(default)]
    pub testing: bool,
    /// Optional unique identifier of the device.
    pub device_id: Option<String>,
    /// Server profile name (usually "default").
    pub profile: Option<String>,
}

/// Metadata describing an ActivityWatch bucket returned by `GET /api/0/buckets/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketMetadata {
    /// Unique bucket identifier (e.g. "aw-watcher-window_HOSTNAME").
    pub id: String,
    /// Bucket creation timestamp.
    pub created: Option<DateTime<Utc>>,
    /// Human-readable custom name, if any.
    pub name: Option<String>,
    /// ActivityWatch event type (e.g. "currentwindow", "afkstatus", "web.tab.current").
    #[serde(rename = "type")]
    pub event_type: String,
    /// Client/watcher that writes to this bucket (e.g. "aw-watcher-window").
    pub client: String,
    /// Hostname where the watcher is running.
    pub hostname: String,
    /// Timestamp when this bucket last received an event.
    pub last_updated: Option<DateTime<Utc>>,
}

/// Map of bucket IDs to their metadata returned by `GET /api/0/buckets/`.
pub type BucketMap = HashMap<String, BucketMetadata>;

/// Individual raw event from `GET /api/0/buckets/{id}/events`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwRawEvent {
    /// Event ID (may be null/absent in unsaved heartbeats).
    pub id: Option<i64>,
    /// Start timestamp of the observation in UTC.
    pub timestamp: DateTime<Utc>,
    /// Duration of the event in seconds (can be 0.0 for instantaneous events).
    #[serde(default)]
    pub duration: f64,
    /// Freeform event data dictionary.
    #[serde(default)]
    pub data: serde_json::Value,
}

/// Window event data structure (`currentwindow`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WindowEventData {
    pub app: Option<String>,
    pub title: Option<String>,
}

/// AFK status event data structure (`afkstatus`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AfkEventData {
    pub status: Option<String>,
}

/// Web tab current event data structure (`web.tab.current`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WebEventData {
    pub url: Option<String>,
    pub title: Option<String>,
    pub audible: Option<bool>,
    pub incognito: Option<bool>,
}
