use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::entities::{CommitmentContract, WorkflowId};

/// Type-safe newtype wrapper for a unique Workflow Session identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Explicit lifecycle states of a workflow session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Ready,
    Active,
    Paused,
    Completed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SessionError {
    #[error("Invalid state transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: SessionState,
        to: SessionState,
    },

    #[error("Cannot cancel an Unbreakable (Ulysses) workflow before target duration has elapsed")]
    CannotBreakUnbreakableWorkflow,

    #[error(
        "Early exit from this Accountable workflow requires a written reason of at least {min_chars} characters (received {actual_chars})"
    )]
    InsufficientReason { min_chars: u32, actual_chars: u32 },
}

/// Authoritative domain entity representing a run of a workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSession {
    pub id: SessionId,
    pub workflow_id: WorkflowId,
    pub state: SessionState,
    pub commitment_contract: CommitmentContract,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub early_exit_reason: Option<String>,
    pub total_focus_seconds: u64,
    pub total_grace_seconds: u64,
    pub total_distraction_seconds: u64,
    pub total_paused_seconds: u64,
    pub total_afk_seconds: u64,
    pub pull_back_count: u32,
    pub intervention_count: u32,
}

impl WorkflowSession {
    /// Creates a new workflow session in the `Ready` state.
    pub fn new(
        id: SessionId,
        workflow_id: WorkflowId,
        commitment_contract: CommitmentContract,
    ) -> Self {
        Self {
            id,
            workflow_id,
            state: SessionState::Ready,
            commitment_contract,
            start_time: Utc::now(),
            end_time: None,
            early_exit_reason: None,
            total_focus_seconds: 0,
            total_grace_seconds: 0,
            total_distraction_seconds: 0,
            total_paused_seconds: 0,
            total_afk_seconds: 0,
            pull_back_count: 0,
            intervention_count: 0,
        }
    }

    /// Transitions session from `Ready` to `Active`.
    pub fn start(&mut self) -> Result<(), SessionError> {
        if self.state != SessionState::Ready {
            return Err(SessionError::InvalidTransition {
                from: self.state,
                to: SessionState::Active,
            });
        }
        self.state = SessionState::Active;
        self.start_time = Utc::now();
        Ok(())
    }

    /// Transitions session from `Active` to `Paused`.
    pub fn pause(&mut self) -> Result<(), SessionError> {
        if self.state != SessionState::Active {
            return Err(SessionError::InvalidTransition {
                from: self.state,
                to: SessionState::Paused,
            });
        }
        self.state = SessionState::Paused;
        Ok(())
    }

    /// Transitions session from `Paused` back to `Active`.
    pub fn resume(&mut self) -> Result<(), SessionError> {
        if self.state != SessionState::Paused {
            return Err(SessionError::InvalidTransition {
                from: self.state,
                to: SessionState::Active,
            });
        }
        self.state = SessionState::Active;
        Ok(())
    }

    /// Transitions session to `Completed`.
    pub fn complete(&mut self) -> Result<(), SessionError> {
        if self.state != SessionState::Active && self.state != SessionState::Paused {
            return Err(SessionError::InvalidTransition {
                from: self.state,
                to: SessionState::Completed,
            });
        }
        self.state = SessionState::Completed;
        self.end_time = Some(Utc::now());
        Ok(())
    }

    /// Attempts to cancel the session early, strictly enforcing commitment contracts.
    pub fn cancel(&mut self, reason: Option<String>) -> Result<(), SessionError> {
        if self.state != SessionState::Active && self.state != SessionState::Paused {
            return Err(SessionError::InvalidTransition {
                from: self.state,
                to: SessionState::Cancelled,
            });
        }

        match &self.commitment_contract {
            CommitmentContract::Standard => {
                self.early_exit_reason = reason;
            }
            CommitmentContract::AccountableBreakable {
                min_reason_characters,
            } => {
                let r = reason.as_deref().unwrap_or("").trim();
                let actual_chars = r.chars().count() as u32;
                if actual_chars < *min_reason_characters {
                    return Err(SessionError::InsufficientReason {
                        min_chars: *min_reason_characters,
                        actual_chars,
                    });
                }
                self.early_exit_reason = Some(r.to_string());
            }
            CommitmentContract::Unbreakable { .. } => {
                return Err(SessionError::CannotBreakUnbreakableWorkflow);
            }
        }

        self.state = SessionState::Cancelled;
        self.end_time = Some(Utc::now());
        Ok(())
    }

    /// Returns the total elapsed wall-clock duration of the session in seconds.
    pub fn duration_seconds(&self, now: DateTime<Utc>) -> u64 {
        let end = self.end_time.unwrap_or(now);
        end.signed_duration_since(self.start_time)
            .num_seconds()
            .max(0) as u64
    }

    /// Marks the session as interrupted due to external crash or system sleep.
    pub fn interrupt(&mut self) {
        if self.state == SessionState::Active || self.state == SessionState::Paused {
            self.state = SessionState::Interrupted;
            self.end_time = Some(Utc::now());
        }
    }
}
