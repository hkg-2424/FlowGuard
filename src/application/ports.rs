use thiserror::Error;

use crate::domain::{ActivityObservation, SourceCapabilities, SourceHealth};

/// High-level errors originating from an activity source implementation.
#[derive(Debug, Error)]
pub enum SourceError {
    #[error(
        "Could not connect to activity source at '{0}': connection refused or service stopped."
    )]
    ConnectionRefused(String),

    #[error(
        "Endpoint '{endpoint}' responded, but did not identify itself as ActivityWatch: {reason}"
    )]
    WrongService { endpoint: String, reason: String },

    #[error("Request to activity source timed out after {0} ms.")]
    Timeout(u64),

    #[error("Authorization required by activity source at '{0}'. Provide an API key via environment variable.")]
    AuthRequired(String),

    #[error("Ambiguous buckets detected for event type '{event_type}': found [{}]", buckets.join(", "))]
    AmbiguousBuckets {
        event_type: String,
        buckets: Vec<String>,
    },

    #[error("Required bucket '{0}' was not found in the activity source.")]
    MissingRequiredBucket(String),

    #[error("Activity source API returned HTTP status {status}: {message}")]
    ApiError { status: u16, message: String },

    #[error("Failed to parse response from activity source: {0}")]
    SerializationError(String),

    #[error("Activity source internal error: {0}")]
    Other(String),
}

/// Core boundary trait for all activity monitoring sources in FlowGuard.
///
/// # Architectural Intent & Replaceability
///
/// FlowGuard decouples the core domain and workflow engine from any specific
/// activity observation provider.
///
/// ```text
///                     +--------------------+
///                     |   ActivitySource   |  (application trait)
///                     +--------------------+
///                               |
///             +-----------------+-----------------+
///             |                                   |
///  +-----------------------+           +-----------------------+
///  | ActivityWatchAdapter  |           | NativeWindowsAdapter  |
///  |  (Milestone 1, REST)  |           | (Future M7, Win32 API)|
///  +-----------------------+           +-----------------------+
/// ```
///
/// ## How a future NativeWindowsAdapter replaces ActivityWatch:
/// 1. `NativeWindowsAdapter` implements this identical `ActivitySource` trait.
/// 2. Instead of polling ActivityWatch's local REST API (`/api/0/buckets/...`),
///    `NativeWindowsAdapter` utilizes native Windows APIs:
///    - `GetForegroundWindow` and `GetWindowTextW` for window titles.
///    - `GetWindowThreadProcessId` + `QueryFullProcessImageNameW` for application paths.
///    - `GetLastInputInfo` for user idle/AFK detection.
///    - Windows UI Automation or native accessibility APIs for browser tab URLs.
/// 3. The `NativeWindowsAdapter` produces the exact same `ActivityObservation` domain model.
/// 4. Downstream components (Workflow Engine, Analytics Engine, Intervention Engine)
///    only receive `ActivityObservation` snapshots via `ActivitySource::poll()`.
///    They have zero knowledge of ActivityWatch bucket IDs, REST URLs, or Win32 handles.
/// 5. In `config.toml`, switching the provider:
///    `provider = "native_windows"` instantiates `NativeWindowsAdapter` instead of `ActivityWatchAdapter`,
///    with zero modifications required anywhere else in the FlowGuard codebase.
pub trait ActivitySource: Send {
    /// Establishes the initial connection to the source, validates service identity,
    /// performs dynamic discovery of capabilities and buckets, and returns source capabilities.
    fn connect(&mut self) -> Result<SourceCapabilities, SourceError>;

    /// Polls the latest activity observation snapshot from the source.
    /// Normalizes raw provider data into the FlowGuard `ActivityObservation` domain model.
    fn poll(&mut self) -> Result<ActivityObservation, SourceError>;

    /// Returns the current operational health of the activity source.
    fn health(&self) -> SourceHealth;
}
