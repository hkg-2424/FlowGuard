use chrono::{DateTime, Utc};
use url::Url;

use super::decision::PolicyDecision;
use super::entities::{
    CanonicalResource, RuleEffect, RuleMatcher, RuleScope, Workflow, WorkflowRule,
};
use super::timer::{GracePeriodTimer, TimerStatus};
use crate::domain::activity::{ActivityObservation, FreshnessState, IdleState};

/// Parses a structured canonical resource from a URL.
pub fn parse_canonical_resource(url_str: &str) -> Option<CanonicalResource> {
    let parsed = Url::parse(url_str).ok()?;
    let host = parsed.host_str()?.to_lowercase();
    let path = parsed.path();

    // YouTube video recognition across desktop, mobile, shorts, and embeds
    if host.ends_with("youtube.com") || host == "youtu.be" {
        if host == "youtu.be" {
            let id = path.trim_start_matches('/');
            if !id.is_empty() {
                return Some(CanonicalResource::YouTubeVideo {
                    video_id: id.to_string(),
                });
            }
        } else if path == "/watch" {
            let vid = parsed
                .query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.to_string())?;
            return Some(CanonicalResource::YouTubeVideo { video_id: vid });
        } else if path.starts_with("/shorts/") {
            let id = path.trim_start_matches("/shorts/");
            if !id.is_empty() {
                return Some(CanonicalResource::YouTubeVideo {
                    video_id: id.to_string(),
                });
            }
        } else if path.starts_with("/embed/") {
            let id = path.trim_start_matches("/embed/");
            if !id.is_empty() {
                return Some(CanonicalResource::YouTubeVideo {
                    video_id: id.to_string(),
                });
            }
        }
    }

    // LeetCode problem slug recognition
    if host.ends_with("leetcode.com") && path.starts_with("/problems/") {
        let slug = path
            .trim_start_matches("/problems/")
            .trim_end_matches('/')
            .split('/')
            .next()?;
        if !slug.is_empty() {
            return Some(CanonicalResource::LeetCodeProblem {
                slug: slug.to_string(),
            });
        }
    }

    // GitHub repository recognition
    if host == "github.com" {
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        if segments.len() >= 2 && !segments[0].is_empty() && !segments[1].is_empty() {
            return Some(CanonicalResource::GitHubRepo {
                owner: segments[0].to_string(),
                repo: segments[1].to_string(),
            });
        }
    }

    None
}

/// Matches a host string against a domain pattern supporting wildcards (e.g. `*.github.com`)
/// and implicit subdomains (e.g. `youtube.com` matches `www.youtube.com` and `m.youtube.com`).
pub fn domain_matches(host: &str, pattern: &str) -> bool {
    let host = host.to_lowercase();
    let pattern = pattern.to_lowercase();

    if let Some(base) = pattern.strip_prefix("*.") {
        host.ends_with(&format!(".{}", base)) || host == base
    } else {
        host == pattern || host.ends_with(&format!(".{}", pattern))
    }
}

/// Pure domain evaluation engine.
///
/// Evaluates incoming [`ActivityObservation`] records against the active [`Workflow`]
/// rules according to the 7-level specificity hierarchy and monotonic grace timers.
#[derive(Debug, Default)]
pub struct RuleEngine;

impl RuleEngine {
    pub fn new() -> Self {
        Self
    }

    /// Evaluates an observation against a workflow, updating grace timers and producing
    /// a deterministic policy decision.
    pub fn evaluate(
        &self,
        observation: &ActivityObservation,
        workflow: &Workflow,
        timer: &mut GracePeriodTimer,
        now: DateTime<Utc>,
    ) -> PolicyDecision {
        // 1. Guard against stale or missing observations: Never penalize on uncertain data!
        if observation.window_freshness == FreshnessState::Stale
            || observation.window_freshness == FreshnessState::Missing
        {
            timer.cancel();
            return PolicyDecision::ObservationUnavailable {
                reason: "Foreground window observation is stale or missing".to_string(),
            };
        }

        let app_name = match observation.application.as_deref() {
            Some(a) => a,
            None => {
                timer.cancel();
                return PolicyDecision::ObservationUnavailable {
                    reason: "No foreground application reported".to_string(),
                };
            }
        };

        // 2. Ignore transitory Windows system surfaces (lock screen, Alt-Tab, taskbar)
        let app_lower = app_name.to_lowercase();
        if matches!(
            app_lower.as_str(),
            "shellexperiencehost.exe"
                | "searchhost.exe"
                | "lockapp.exe"
                | "startmenuexperiencehost.exe"
                | "applicationframehost.exe"
        ) {
            timer.pause();
            return PolicyDecision::Ignored {
                reason: "Windows system surface".to_string(),
            };
        }

        // 3. Principle 2: Do not mistake inactivity for distraction
        if observation.idle_state == IdleState::Afk {
            timer.pause();
            return PolicyDecision::Ignored {
                reason: "User is AFK (physical disengagement/thinking pause)".to_string(),
            };
        }

        // 4. Extract URL and Canonical Resource
        let browser_url = observation.browser.as_ref().and_then(|b| b.url.as_deref());
        let browser_host = browser_url
            .and_then(|u| Url::parse(u).ok())
            .and_then(|u| u.host_str().map(|h| h.to_lowercase()));
        let canonical_resource = browser_url.and_then(parse_canonical_resource);

        // 5. Evaluate all active rules
        let mut matching_rules: Vec<(&WorkflowRule, u32)> = Vec::new();

        for rule in &workflow.rules {
            if !rule.is_active(now) {
                continue;
            }

            let matches = match &rule.matcher {
                RuleMatcher::Canonical(target_res) => {
                    canonical_resource.as_ref() == Some(target_res)
                }
                RuleMatcher::ExactUrl(target_url) => {
                    browser_url.map(|u| u == target_url).unwrap_or(false)
                }
                RuleMatcher::DomainPattern(pattern) => browser_host
                    .as_ref()
                    .map(|h| domain_matches(h, pattern))
                    .unwrap_or(false),
                RuleMatcher::ApplicationName(target_app) => app_lower == target_app.to_lowercase(),
                RuleMatcher::Any => true,
            };

            if matches {
                let score = (rule.scope as u32) * 1000 + rule.priority_weight;
                matching_rules.push((rule, score));
            }
        }

        // 6. Conflict Resolution & Specificity Sorting
        matching_rules.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| {
                    // Safe-by-default: Restrict beats Allow on exact tie
                    match (a.0.effect, b.0.effect) {
                        (RuleEffect::Restrict, RuleEffect::Allow) => std::cmp::Ordering::Less,
                        (RuleEffect::Allow, RuleEffect::Restrict) => std::cmp::Ordering::Greater,
                        _ => std::cmp::Ordering::Equal,
                    }
                })
                .then_with(|| a.0.id.0.cmp(&b.0.id.0)) // Deterministic alphabetical tie-break
        });

        // 7. Route Policy Decision
        if let Some((winning_rule, _)) = matching_rules.first() {
            match winning_rule.effect {
                RuleEffect::Allow => {
                    timer.cancel();
                    PolicyDecision::Allowed {
                        rule_id: winning_rule.id.clone(),
                        scope: winning_rule.scope,
                    }
                }
                RuleEffect::Warn => {
                    timer.cancel();
                    PolicyDecision::WarningRequired {
                        rule_id: winning_rule.id.clone(),
                        reason: winning_rule.description.clone(),
                    }
                }
                RuleEffect::Ignore | RuleEffect::Monitor => {
                    timer.cancel();
                    PolicyDecision::Ignored {
                        reason: winning_rule.description.clone(),
                    }
                }
                RuleEffect::Restrict => {
                    let grace_duration = winning_rule
                        .custom_grace_seconds
                        .unwrap_or(workflow.default_grace_period_seconds);

                    let target_name = browser_url.unwrap_or(app_name).to_string();

                    match timer.tick(&winning_rule.id, &target_name, grace_duration) {
                        TimerStatus::GracePeriod {
                            remaining_seconds,
                            total_grace_seconds,
                        } => PolicyDecision::GracePeriod {
                            rule_id: winning_rule.id.clone(),
                            remaining_seconds,
                            total_grace_seconds,
                            target_name,
                        },
                        TimerStatus::Expired { .. } => PolicyDecision::InterventionRequired {
                            rule_id: winning_rule.id.clone(),
                            target_name,
                            reason: format!(
                                "Activity violates workflow intention ({} exceeded)",
                                winning_rule.description
                            ),
                        },
                        TimerStatus::Idle => PolicyDecision::Allowed {
                            rule_id: winning_rule.id.clone(),
                            scope: winning_rule.scope,
                        },
                    }
                }
            }
        } else {
            // Default fallback
            match workflow.default_effect {
                RuleEffect::Allow => {
                    timer.cancel();
                    PolicyDecision::Allowed {
                        rule_id: super::entities::RuleId::new("default"),
                        scope: RuleScope::WorkflowDefault,
                    }
                }
                RuleEffect::Restrict => {
                    let target_name = browser_url.unwrap_or(app_name).to_string();
                    let default_rule_id = super::entities::RuleId::new("default_fallback");
                    match timer.tick(
                        &default_rule_id,
                        &target_name,
                        workflow.default_grace_period_seconds,
                    ) {
                        TimerStatus::GracePeriod {
                            remaining_seconds,
                            total_grace_seconds,
                        } => PolicyDecision::GracePeriod {
                            rule_id: default_rule_id,
                            remaining_seconds,
                            total_grace_seconds,
                            target_name,
                        },
                        TimerStatus::Expired { .. } => PolicyDecision::InterventionRequired {
                            rule_id: default_rule_id,
                            target_name,
                            reason: "Activity outside planned workflow scope".to_string(),
                        },
                        TimerStatus::Idle => PolicyDecision::Allowed {
                            rule_id: default_rule_id,
                            scope: RuleScope::WorkflowDefault,
                        },
                    }
                }
                _ => {
                    timer.cancel();
                    PolicyDecision::Ignored {
                        reason: "Default workflow policy ignored".to_string(),
                    }
                }
            }
        }
    }
}
