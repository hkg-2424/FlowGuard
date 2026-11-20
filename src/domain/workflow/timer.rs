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
    paused_at: Option<Instant>,
    total_paused_duration: Duration,
    episode_id: u64,
}

/// Monotonic, drift-resilient grace period timer.
///
/// Guarantees:
/// 1. Uses [`Instant`] so clock skew, DST shifts, or NTP updates never distort countdowns.
/// 2. Accurately records sub-30s self-corrections for focus resilience analytics.
/// 3. Pauses during AFK or session suspension without leaking elapsed grace time.
#[derive(Debug, Default)]
pub struct GracePeriodTimer {
    active_violation: Option<ActiveViolation>,
    last_resolution: Option<DriftResolution>,
    next_episode_id: u64,
}

impl GracePeriodTimer {
    pub fn new() -> Self {
        Self {
            active_violation: None,
            last_resolution: None,
            next_episode_id: 0,
        }
    }

    /// Evaluates the timer for the given rule and target.
    ///
    /// Invariant: Once an unallowed activity begins, switching between different unallowed
    /// applications (e.g. Discord -> Steam -> YouTube) does NOT reset the grace timer.
    /// The countdown continues monotonically until the user returns to an allowed app.
    pub fn tick(&mut self, rule_id: &RuleId, target_name: &str, grace_seconds: u32) -> TimerStatus {
        let now = Instant::now();
        let grace_duration = Duration::from_secs(grace_seconds as u64);

        let violation = match self.active_violation.as_mut() {
            Some(v) => {
                // Ongoing drift episode: update latest target and rule, but PRESERVE started_at
                // so the grace countdown does NOT reset when switching between unallowed applications!
                v.rule_id = rule_id.clone();
                v.target_name = target_name.to_string();

                // If resuming from pause (e.g. user was AFK), accumulate paused duration
                if let Some(paused_time) = v.paused_at.take() {
                    v.total_paused_duration += now.saturating_duration_since(paused_time);
                }

                // H-28: Stricter grace duration wins when switching unallowed rules
                v.grace_duration = v.grace_duration.min(grace_duration);
                v
            }
            None => {
                // New violation episode started
                self.next_episode_id += 1;
                self.active_violation = Some(ActiveViolation {
                    rule_id: rule_id.clone(),
                    target_name: target_name.to_string(),
                    started_at: now,
                    grace_duration,
                    intervened: false,
                    paused_at: None,
                    total_paused_duration: Duration::ZERO,
                    episode_id: self.next_episode_id,
                });
                self.active_violation.as_mut().unwrap()
            }
        };

        let raw_elapsed = now.saturating_duration_since(violation.started_at);
        let active_elapsed = raw_elapsed.saturating_sub(violation.total_paused_duration);

        if active_elapsed < violation.grace_duration {
            let remaining = (violation.grace_duration - active_elapsed).as_secs() as u32;
            TimerStatus::GracePeriod {
                remaining_seconds: remaining,
                total_grace_seconds: violation.grace_duration.as_secs() as u32,
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
        let mut violation = self.active_violation.take()?;
        let now = Instant::now();
        if let Some(paused_time) = violation.paused_at.take() {
            violation.total_paused_duration += now.saturating_duration_since(paused_time);
        }

        let raw_elapsed = now.saturating_duration_since(violation.started_at);
        let active_elapsed = raw_elapsed.saturating_sub(violation.total_paused_duration);

        // H-28: Evaluate expiry during cancellation!
        // If polling skipped the expiry moment and the user returns after the deadline,
        // it must be classified as Intervened, NOT SelfCorrected.
        let is_expired = violation.intervened || active_elapsed >= violation.grace_duration;
        let res = if is_expired {
            DriftResolution::Intervened
        } else {
            DriftResolution::SelfCorrected {
                recovery_seconds: active_elapsed.as_secs().max(1) as u32,
            }
        };
        self.last_resolution = Some(res.clone());
        Some(res)
    }

    /// Takes the last resolution recorded when an active violation was cancelled.
    pub fn take_last_resolution(&mut self) -> Option<DriftResolution> {
        self.last_resolution.take()
    }

    /// Returns the monotonic episode ID of the active violation, if one is running (H-31).
    pub fn current_episode_id(&self) -> Option<u64> {
        self.active_violation.as_ref().map(|v| v.episode_id)
    }

    /// Returns true if the active violation has already expired and triggered an intervention (H-31).
    pub fn is_intervened(&self) -> bool {
        self.active_violation
            .as_ref()
            .map(|v| v.intervened)
            .unwrap_or(false)
    }

    /// Returns the effective grace duration for the active violation, if any.
    pub fn active_grace_duration(&self) -> Option<Duration> {
        self.active_violation.as_ref().map(|v| v.grace_duration)
    }

    /// Suspends the timer during AFK or system sleep without leaking elapsed time.
    pub fn pause(&mut self) {
        if let Some(ref mut v) = self.active_violation {
            if v.paused_at.is_none() {
                v.paused_at = Some(Instant::now());
            }
        }
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

    /// Returns the target name of the currently active violation, if any.
    pub fn active_target(&self) -> Option<&str> {
        self.active_violation
            .as_ref()
            .map(|v| v.target_name.as_str())
    }

    /// Returns the rule id of the currently active violation, if any.
    pub fn active_rule_id(&self) -> Option<&RuleId> {
        self.active_violation.as_ref().map(|v| &v.rule_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grace_period_timer_persists_across_unallowed_targets() {
        let mut timer = GracePeriodTimer::new();
        let rule1 = RuleId::new("rule-discord");
        let rule2 = RuleId::new("rule-steam");

        // 1. Initial drift into Discord
        let s1 = timer.tick(&rule1, "discord.exe", 30);
        match s1 {
            TimerStatus::GracePeriod {
                remaining_seconds,
                total_grace_seconds,
            } => {
                assert_eq!(total_grace_seconds, 30);
                assert!(remaining_seconds <= 30);
            }
            _ => panic!("Expected GracePeriod"),
        }
        assert_eq!(timer.active_target(), Some("discord.exe"));

        // 2. User switches to Steam (still unallowed) -> Timer MUST NOT reset!
        std::thread::sleep(std::time::Duration::from_millis(50));
        let s2 = timer.tick(&rule2, "steam.exe", 30);
        match s2 {
            TimerStatus::GracePeriod {
                remaining_seconds, ..
            } => {
                assert!(remaining_seconds <= 30);
            }
            _ => panic!("Expected GracePeriod"),
        }
        assert_eq!(timer.active_target(), Some("steam.exe"));

        // 3. Returning to allowed app cancels the violation cleanly
        let res = timer.cancel();
        assert!(matches!(res, Some(DriftResolution::SelfCorrected { .. })));
        assert!(!timer.is_active());
    }

    #[test]
    fn test_h28_stricter_grace_duration_applied_on_rule_switch() {
        let mut timer = GracePeriodTimer::new();
        let rule1 = RuleId::new("rule-30s");
        let rule2 = RuleId::new("rule-10s");

        // 1. Initial drift with 30s grace
        timer.tick(&rule1, "discord.exe", 30);
        assert_eq!(timer.active_grace_duration(), Some(Duration::from_secs(30)));

        // 2. Switch to rule with stricter 10s grace -> must update effective duration
        let s2 = timer.tick(&rule2, "steam.exe", 10);
        assert_eq!(timer.active_grace_duration(), Some(Duration::from_secs(10)));
        match s2 {
            TimerStatus::GracePeriod {
                total_grace_seconds,
                remaining_seconds,
            } => {
                assert_eq!(total_grace_seconds, 10);
                assert!(remaining_seconds <= 10);
            }
            _ => panic!("Expected GracePeriod"),
        }
    }

    #[test]
    fn test_h28_expiry_evaluated_during_cancellation_when_tick_missed() {
        let mut timer = GracePeriodTimer::new();
        let rule = RuleId::new("rule-instant");

        // 1. Drift with 0-second grace (immediate expiry)
        let s = timer.tick(&rule, "game.exe", 0);
        assert!(matches!(s, TimerStatus::Expired { .. }));

        // 2. Cancellation must evaluate to Intervened, not SelfCorrected
        let res = timer.cancel();
        assert_eq!(res, Some(DriftResolution::Intervened));
    }

    #[test]
    fn test_h31_episode_id_tracking() {
        let mut timer = GracePeriodTimer::new();
        let rule = RuleId::new("rule-1");

        assert_eq!(timer.current_episode_id(), None);

        timer.tick(&rule, "target1", 30);
        let ep1 = timer.current_episode_id();
        assert_eq!(ep1, Some(1));

        // Switching target in same episode keeps episode ID
        timer.tick(&rule, "target2", 30);
        assert_eq!(timer.current_episode_id(), Some(1));

        timer.cancel();
        assert_eq!(timer.current_episode_id(), None);

        // Next violation gets next episode ID
        timer.tick(&rule, "target3", 30);
        assert_eq!(timer.current_episode_id(), Some(2));
    }
}
