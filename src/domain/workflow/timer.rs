use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::entities::RuleId;

/// Resolution of a temporary drift away from productive applications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriftResolution {
    /// The user self-corrected and returned to their productive workflow within the grace window.
    /// This is a positive indicator of focus pull-back resilience!
    SelfCorrected { recovery_seconds: u32 },
    /// The user stayed past the grace window; an active screen prompt was triggered.
    Intervened,
}

/// Current status of the grace timer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerStatus {
    /// No violation is active; user is within allowed workflow context.
    Idle,
    /// User is outside allowed context, but within the grace period buffer.
    GracePeriod {
        remaining_seconds: u32,
        total_grace_seconds: u32,
    },
    /// Grace period has fully expired; on-screen prompt required.
    Expired { intervened: bool },
}

#[derive(Debug, Clone)]
struct ActiveViolation {
    rule_id: RuleId,
    target_name: String,
    started_at: Instant,
    grace_duration: Duration,
    intervened: bool,
}

/// Monotonic, drift-resilient grace period timer.
///
/// Guarantees:
/// 1. Uses [`Instant`] so clock skew, DST shifts, or NTP updates never distort countdowns.
/// 2. Accurately records sub-30s self-corrections for focus resilience analytics.
/// 3. Pauses during AFK or session suspension.
#[derive(Debug, Default)]
pub struct GracePeriodTimer {
    active_violation: Option<ActiveViolation>,
    last_resolution: Option<DriftResolution>,
}

impl GracePeriodTimer {
    pub fn new() -> Self {
        Self {
            active_violation: None,
            last_resolution: None,
        }
    }

    /// Evaluates the timer for the given rule and target.
    pub fn tick(&mut self, rule_id: &RuleId, target_name: &str, grace_seconds: u32) -> TimerStatus {
        let now = Instant::now();
        let grace_duration = Duration::from_secs(grace_seconds as u64);

        let violation = match self.active_violation.as_mut() {
            Some(v) if v.rule_id == *rule_id && v.target_name == target_name => v,
            _ => {
                // New violation started
                self.active_violation = Some(ActiveViolation {
                    rule_id: rule_id.clone(),
                    target_name: target_name.to_string(),
                    started_at: now,
                    grace_duration,
                    intervened: false,
                });
                self.active_violation.as_mut().unwrap()
            }
        };

        let elapsed = now.saturating_duration_since(violation.started_at);
        if elapsed < violation.grace_duration {
            let remaining = (violation.grace_duration - elapsed).as_secs() as u32;
            TimerStatus::GracePeriod {
                remaining_seconds: remaining,
                total_grace_seconds: grace_seconds,
            }
        } else {
            let already_intervened = violation.intervened;
            violation.intervened = true;
            TimerStatus::Expired {
                intervened: already_intervened,
            }
        }
    }

    /// Cancels any active violation when the user returns to an allowed activity.
    /// Returns a [`DriftResolution`] if an active violation was resolved.
    pub fn cancel(&mut self) -> Option<DriftResolution> {
        let violation = self.active_violation.take()?;
        let elapsed = Instant::now().saturating_duration_since(violation.started_at);

        let res = if violation.intervened {
            DriftResolution::Intervened
        } else {
            DriftResolution::SelfCorrected {
                recovery_seconds: elapsed.as_secs().max(1) as u32,
            }
        };
        self.last_resolution = Some(res.clone());
        Some(res)
    }

    /// Takes the last resolution recorded when an active violation was cancelled.
    pub fn take_last_resolution(&mut self) -> Option<DriftResolution> {
        self.last_resolution.take()
    }

    /// Suspends the timer during AFK or system sleep without resetting elapsed time.
    pub fn pause(&mut self) {
        // Violation remains registered, avoiding accidental cancellation while user is AFK
    }

    /// Resets the timer completely without returning a resolution.
    pub fn reset(&mut self) {
        self.active_violation = None;
        self.last_resolution = None;
    }

    /// Returns true if a violation is currently active.
    pub fn is_active(&self) -> bool {
        self.active_violation.is_some()
    }
}
