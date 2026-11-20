use thiserror::Error;

use crate::domain::{ActivityObservation, DiagnosticReport, SourceCapabilities, SourceHealth};

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

    /// Performs deep provider-specific diagnostics and returns a structured,
    /// provider-neutral diagnostic report for the `flowguard doctor` command.
    fn diagnose(&mut self) -> Result<DiagnosticReport, SourceError>;
}

/// High-level errors originating from repository operations.
#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("Entity not found: {0}")]
    NotFound(String),

    #[error("Persistence error: {0}")]
    Storage(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Corrupt data in storage: {0}")]
    CorruptData(String),
}

/// Port for persisting and loading workflow definitions and their policy rules.
pub trait WorkflowRepository: Send + Sync {
    /// Retrieves a workflow definition by its unique identifier.
    fn get(
        &self,
        id: &crate::domain::WorkflowId,
    ) -> Result<Option<crate::domain::Workflow>, RepositoryError>;

    /// Saves or updates a workflow definition.
    fn save(&mut self, workflow: &crate::domain::Workflow) -> Result<(), RepositoryError>;

    /// Lists all stored workflows.
    fn list(&self) -> Result<Vec<crate::domain::Workflow>, RepositoryError>;

    /// Deletes a workflow definition by ID, returning true if it existed.
    fn delete(&mut self, id: &crate::domain::WorkflowId) -> Result<bool, RepositoryError>;
}

/// Port for persisting session states, accounting metrics, and drift resolution logs.
pub trait SessionRepository: Send + Sync {
    /// Retrieves a session record by its unique identifier.
    fn get(
        &self,
        id: &crate::domain::SessionId,
    ) -> Result<Option<crate::domain::WorkflowSession>, RepositoryError>;

    /// Saves or updates an active or completed workflow session.
    fn save(&mut self, session: &crate::domain::WorkflowSession) -> Result<(), RepositoryError>;

    /// Logs an individual drift episode (both sub-30s self-corrections and post-30s interventions).
    fn record_drift(&mut self, drift: &crate::domain::DriftEvent) -> Result<(), RepositoryError>;

    /// Retrieves all drift events recorded for a given session.
    fn list_drifts(
        &self,
        session_id: &crate::domain::SessionId,
    ) -> Result<Vec<crate::domain::DriftEvent>, RepositoryError>;

    /// Lists recent workflow sessions ordered by start time descending.
    fn list_recent(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::domain::WorkflowSession>, RepositoryError>;

    /// Reconciles unclosed active/paused sessions after an unclean shutdown, marking them Interrupted with RFC3339 timestamps.
    fn recover_interrupted_sessions(&mut self) -> Result<usize, RepositoryError> {
        Ok(0)
    }
}
