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
        if matches!(
            self.commitment_contract,
            CommitmentContract::Unbreakable { .. }
        ) {
            return Err(SessionError::CannotBreakUnbreakableWorkflow);
        }
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

    /// Authoritative active focus elapsed duration metric in seconds (H-19).
    /// Strictly excludes paused time, AFK disengagement, and unallowed distraction.
    pub fn active_focus_seconds(&self) -> u64 {
        self.total_focus_seconds
    }

    /// Transitions session to `Completed`.
    ///
    /// C-02 Enforcement:
    /// - `Unbreakable` contracts reject early completion when active focus elapsed duration
    ///   has not reached `target_duration_seconds`, unless an explicit Emergency Override is supplied.
    /// - `AccountableBreakable` contracts enforce that early completion before target duration
    ///   requires an emergency override reason satisfying `min_reason_characters`.
    pub fn complete(&mut self) -> Result<(), SessionError> {
        self.complete_with_override(None)
    }

    /// Transitions session to `Completed` with an optional emergency override.
    pub fn complete_with_override(
        &mut self,
        emergency_override: Option<&str>,
    ) -> Result<(), SessionError> {
        self.complete_with_contract_target(None, emergency_override)
    }

    /// Transitions session to `Completed` with an optional explicit target duration and emergency override.
    pub fn complete_with_contract_target(
        &mut self,
        target_duration_seconds: Option<u64>,
        emergency_override: Option<&str>,
    ) -> Result<(), SessionError> {
        if self.state != SessionState::Active && self.state != SessionState::Paused {
            return Err(SessionError::InvalidTransition {
                from: self.state,
                to: SessionState::Completed,
            });
        }

        let is_emergency = emergency_override
            .map(|r| {
                let trimmed = r.trim();
                !trimmed.is_empty()
            })
            .unwrap_or(false);

        match &self.commitment_contract {
            CommitmentContract::Standard => {
                if let Some(r) = emergency_override {
                    let trimmed = r.trim();
                    if !trimmed.is_empty() {
                        self.early_exit_reason = Some(trimmed.to_string());
                    }
                }
            }
            CommitmentContract::AccountableBreakable {
                min_reason_characters,
            } => {
                // If target duration is specified (e.g. from workflow) and has not elapsed:
                if let Some(target) = target_duration_seconds {
                    if self.total_focus_seconds < target {
                        let reason = emergency_override.map(|s| s.trim()).unwrap_or("");
                        let actual_chars = reason.chars().count() as u32;
                        if actual_chars < *min_reason_characters {
                            return Err(SessionError::InsufficientReason {
                                min_chars: *min_reason_characters,
                                actual_chars,
                            });
                        }
                        self.early_exit_reason = Some(reason.to_string());
                    }
                } else if is_emergency {
                    let reason = emergency_override.unwrap().trim();
                    let actual_chars = reason.chars().count() as u32;
                    if actual_chars < *min_reason_characters {
                        return Err(SessionError::InsufficientReason {
                            min_chars: *min_reason_characters,
                            actual_chars,
                        });
                    }
                    self.early_exit_reason = Some(reason.to_string());
                }
            }
            CommitmentContract::Unbreakable {
                target_duration_seconds: contract_target,
            } => {
                // Authoritative active focus check (H-19)
                if self.total_focus_seconds < *contract_target {
                    if let Some(r) = emergency_override {
                        let reason = r.trim();
                        if reason.is_empty() {
                            return Err(SessionError::CannotBreakUnbreakableWorkflow);
                        }
                        self.early_exit_reason = Some(format!("EMERGENCY OVERRIDE: {reason}"));
                    } else {
                        return Err(SessionError::CannotBreakUnbreakableWorkflow);
                    }
                }
            }
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
            CommitmentContract::Unbreakable {
                target_duration_seconds,
            } => {
                // Authoritative active focus duration metric (H-19)
                // Excludes paused time, AFK time, and distraction!
                let active_elapsed = self.total_focus_seconds;
                if active_elapsed >= *target_duration_seconds {
                    self.state = SessionState::Completed;
                    self.end_time = Some(Utc::now());
                    return Ok(());
                } else if let Some(ref r) = reason {
                    // Check for explicit emergency override during cancellation
                    let r_trim = r.trim();
                    if !r_trim.is_empty()
                        && (r_trim.to_lowercase().contains("emergency")
                            || r_trim.starts_with("EMERGENCY"))
                    {
                        self.early_exit_reason = Some(format!("EMERGENCY OVERRIDE: {r_trim}"));
                        self.state = SessionState::Cancelled;
                        self.end_time = Some(Utc::now());
                        return Ok(());
                    } else {
                        return Err(SessionError::CannotBreakUnbreakableWorkflow);
                    }
                } else {
                    return Err(SessionError::CannotBreakUnbreakableWorkflow);
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unbreakable_prevents_early_complete_without_override() {
        let mut session = WorkflowSession::new(
            SessionId::new("test-sess"),
            WorkflowId::new("wf-test"),
            CommitmentContract::Unbreakable {
                target_duration_seconds: 1800,
            },
        );
        session.start().unwrap();

        // 0 active focus seconds elapsed -> complete() must fail (C-02)
        let err = session.complete().unwrap_err();
        assert_eq!(err, SessionError::CannotBreakUnbreakableWorkflow);
        assert_eq!(session.state, SessionState::Active);

        // Even with wall-clock time elapsed, focus seconds must be authoritative (H-19)
        session.total_grace_seconds = 300;
        session.total_paused_seconds = 600;
        session.total_distraction_seconds = 1000;
        let err2 = session.complete().unwrap_err();
        assert_eq!(err2, SessionError::CannotBreakUnbreakableWorkflow);

        // Emergency override allows early completion
        session
            .complete_with_override(Some("Medical emergency"))
            .unwrap();
        assert_eq!(session.state, SessionState::Completed);
        assert_eq!(
            session.early_exit_reason,
            Some("EMERGENCY OVERRIDE: Medical emergency".to_string())
        );
    }

    #[test]
    fn test_unbreakable_completes_when_target_reached() {
        let mut session = WorkflowSession::new(
            SessionId::new("test-sess-2"),
            WorkflowId::new("wf-test"),
            CommitmentContract::Unbreakable {
                target_duration_seconds: 1800,
            },
        );
        session.start().unwrap();
        session.total_focus_seconds = 1800;

        // Target reached -> complete succeeds naturally
        session.complete().unwrap();
        assert_eq!(session.state, SessionState::Completed);
    }

    #[test]
    fn test_accountable_with_target_requires_sufficient_reason_when_early() {
        let mut session = WorkflowSession::new(
            SessionId::new("test-sess-3"),
            WorkflowId::new("wf-test"),
            CommitmentContract::AccountableBreakable {
                min_reason_characters: 15,
            },
        );
        session.start().unwrap();
        session.total_focus_seconds = 200;

        // Target is 1800s, elapsed is 200s, no reason -> early complete fails
        let err = session
            .complete_with_contract_target(Some(1800), None)
            .unwrap_err();
        assert_eq!(
            err,
            SessionError::InsufficientReason {
                min_chars: 15,
                actual_chars: 0
            }
        );

        // Too short reason -> fails
        let err2 = session
            .complete_with_contract_target(Some(1800), Some("Too short"))
            .unwrap_err();
        assert_eq!(
            err2,
            SessionError::InsufficientReason {
                min_chars: 15,
                actual_chars: 9
            }
        );

        // Sufficient reason -> succeeds
        session
            .complete_with_contract_target(
                Some(1800),
                Some("Need to prepare urgently for client presentation"),
            )
            .unwrap();
        assert_eq!(session.state, SessionState::Completed);
        assert_eq!(
            session.early_exit_reason,
            Some("Need to prepare urgently for client presentation".to_string())
        );
    }
}
