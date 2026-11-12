use super::entities::{RuleEffect, RuleId, RuleScope};
use serde::{Deserialize, Serialize};

/// Deterministic policy decision emitted by the Workflow Engine on each evaluation cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    /// Activity is explicitly permitted by the workflow rules.
    Allowed { rule_id: RuleId, scope: RuleScope },
    /// Activity is allowed temporarily, with an ambient non-blocking warning.
    WarningRequired { rule_id: RuleId, reason: String },
    /// Activity is restricted, but the user is within their grace period.
    GracePeriod {
        rule_id: RuleId,
        remaining_seconds: u32,
        total_grace_seconds: u32,
        target_name: String,
    },
    /// Grace period expired; an active on-screen intervention must prompt the user.
    InterventionRequired {
        rule_id: RuleId,
        target_name: String,
        reason: String,
    },
    /// Transitory or shell interaction (e.g. Lock Screen, Alt-Tab) excluded from distraction tracking.
    Ignored { reason: String },
    /// Observation is stale, missing, or lacks required visibility.
    ObservationUnavailable { reason: String },
    /// No workflow session is currently running.
    WorkflowInactive,
}

impl PolicyDecision {
    /// Returns true if this decision permits the user to continue working without intervention.
    pub fn is_allowed(&self) -> bool {
        matches!(
            self,
            PolicyDecision::Allowed { .. } | PolicyDecision::Ignored { .. }
        )
    }

    /// Returns true if this decision indicates an active policy violation requiring attention.
    pub fn is_violation(&self) -> bool {
        matches!(
            self,
            PolicyDecision::GracePeriod { .. } | PolicyDecision::InterventionRequired { .. }
        )
    }
}

/// Structured explanation of why a policy decision was reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyReason {
    pub matched_rule_id: Option<RuleId>,
    pub matched_scope: Option<RuleScope>,
    pub effect: RuleEffect,
    pub explanation: String,
}
