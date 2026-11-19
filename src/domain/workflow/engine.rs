use chrono::{DateTime, Utc};
use url::Url;

use super::decision::PolicyDecision;
use super::entities::{
    CanonicalResource, RuleEffect, RuleMatcher, RuleScope, Workflow, WorkflowRule,
};
use super::timer::{GracePeriodTimer, TimerStatus};
use crate::domain::activity::{ActivityObservation, FreshnessState, IdleState};

fn is_valid_youtube_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parses a structured canonical resource from a URL.
pub fn parse_canonical_resource(url_str: &str) -> Option<CanonicalResource> {
    let parsed = Url::parse(url_str).ok()?;
    let host = parsed.host_str()?.to_lowercase();
    let path = parsed.path();

    // YouTube video recognition across desktop, mobile, shorts, and embeds
    let is_yt = host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be";
    if is_yt {
        if host == "youtu.be" {
            let id = path.trim_start_matches('/').split('/').next()?;
            if is_valid_youtube_video_id(id) {
                return Some(CanonicalResource::YouTubeVideo {
                    video_id: id.to_string(),
                });
            }
        } else if path == "/watch" {
            let vid = parsed
                .query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.to_string())?;
            if is_valid_youtube_video_id(&vid) {
                return Some(CanonicalResource::YouTubeVideo { video_id: vid });
            }
        } else if path.starts_with("/shorts/") {
            let id = path.trim_start_matches("/shorts/").split('/').next()?;
            if is_valid_youtube_video_id(id) {
                return Some(CanonicalResource::YouTubeVideo {
                    video_id: id.to_string(),
                });
            }
        } else if path.starts_with("/embed/") {
            let id = path.trim_start_matches("/embed/").split('/').next()?;
            if is_valid_youtube_video_id(id) {
                return Some(CanonicalResource::YouTubeVideo {
                    video_id: id.to_string(),
                });
            }
        }
    }

    // LeetCode problem slug recognition
    let is_leetcode = host == "leetcode.com" || host.ends_with(".leetcode.com");
    if is_leetcode && path.starts_with("/problems/") {
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
    if host == "github.com" || host == "www.github.com" {
        let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        if segments.len() >= 2 && !segments[0].is_empty() && !segments[1].is_empty() {
            let owner = segments[0].to_lowercase();
            let reserved = [
                "settings",
                "features",
                "explore",
                "trending",
                "pricing",
                "contact",
                "security",
                "login",
                "signup",
                "organizations",
                "orgs",
                "pulls",
                "issues",
                "notifications",
                "marketplace",
                "topics",
                "collections",
                "events",
                "about",
            ];
            if !reserved.contains(&owner.as_str()) {
                let repo = segments[1].trim_end_matches(".git");
                if !repo.is_empty() {
                    return Some(CanonicalResource::GitHubRepo {
                        owner: segments[0].to_string(),
                        repo: repo.to_string(),
                    });
                }
            }
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

/// Matches a window title against a domain pattern when browser URL tracking is unavailable
/// (e.g. native Windows observer without browser extensions).
pub fn title_matches_domain(title: &str, pattern: &str) -> bool {
    let title_lower = title.to_lowercase();
    let pattern_lower = pattern.to_lowercase();
    let sld = pattern_lower.strip_prefix("*.").unwrap_or(&pattern_lower);

    // 1. Direct substring match (e.g. title contains "docs.rs" or "github.com")
    if title_lower.contains(sld) {
        return true;
    }

    // 2. Main domain token match (e.g. "github.com" -> "github", "wikipedia.org" -> "wikipedia")
    let parts: Vec<&str> = sld.split('.').collect();
    if parts.len() >= 2 {
        let main_token = parts[parts.len() - 2];
        if main_token.len() >= 3 {
            // Check direct token in title or title without whitespace (e.g. "Stack Overflow" for "stackoverflow")
            if title_lower.contains(main_token) || title_lower.replace(' ', "").contains(main_token)
            {
                return true;
            }
        }
    }

    false
}

/// Matches an observed application and window title against an allowed/restricted application rule.
///
/// Supports exact executable names (e.g. `code.exe`), names without `.exe` (e.g. `code`),
/// friendly application aliases (e.g. `Visual Studio Code`, `VS Code`, `Antigravity`, `Windows Terminal`),
/// and window title substrings.
pub fn app_matches(app_name: &str, window_title: Option<&str>, target_app: &str) -> bool {
    let app_lower = app_name.to_lowercase();
    let target_lower = target_app.to_lowercase();

    // 1. Direct exact or sanitized match (e.g. "code.exe" == "code.exe" or "code" == "code")
    if app_lower == target_lower
        || app_lower.trim_end_matches(".exe") == target_lower.trim_end_matches(".exe")
    {
        return true;
    }

    // 2. Friendly display name / alias mapping
    let matches_alias = match target_lower.as_str() {
        "visual studio code" | "vs code" | "vscode" => {
            app_lower == "code.exe" || app_lower == "code"
        }
        "antigravity" => {
            app_lower.contains("antigravity")
                || window_title
                    .map(|t| t.to_lowercase().contains("antigravity"))
                    .unwrap_or(false)
        }
        "terminal" | "windows terminal" => {
            app_lower == "windowsterminal.exe"
                || app_lower == "cmd.exe"
                || app_lower == "powershell.exe"
        }
        "chrome" | "google chrome" => app_lower == "chrome.exe",
        "edge" | "microsoft edge" => app_lower == "msedge.exe",
        "brave" | "brave browser" => app_lower == "brave.exe",
        "firefox" | "mozilla firefox" => app_lower == "firefox.exe",
        "notion" => app_lower == "notion.exe",
        "obsidian" => app_lower == "obsidian.exe",
        "slack" => app_lower == "slack.exe",
        "discord" => app_lower == "discord.exe",
        "spotify" => app_lower == "spotify.exe",
        "steam" => app_lower == "steam.exe",
        "visual studio" | "devenv" => app_lower == "devenv.exe",
        "intellij" | "intellij idea" => app_lower == "idea64.exe" || app_lower == "idea.exe",
        "pycharm" => app_lower == "pycharm64.exe" || app_lower == "pycharm.exe",
        "word" | "microsoft word" => app_lower == "winword.exe",
        "excel" | "microsoft excel" => app_lower == "excel.exe",
        "powerpoint" => app_lower == "powerpnt.exe",
        "onenote" => app_lower == "onenote.exe",
        "acrobat" | "adobe acrobat" | "pdf reader" => {
            app_lower == "acrobat.exe" || app_lower == "foxitreader.exe"
        }
        _ => false,
    };

    if matches_alias {
        return true;
    }

    // 3. Fallback: Check if window title explicitly contains the friendly target
    if let Some(title) = window_title {
        let title_lower = title.to_lowercase();
        if title_lower.contains(&target_lower) {
            return true;
        }
    }

    false
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
                RuleMatcher::DomainPattern(pattern) => {
                    if let Some(ref h) = browser_host {
                        domain_matches(h, pattern)
                    } else if let Some(ref title) = observation.window_title {
                        title_matches_domain(title, pattern)
                    } else {
                        false
                    }
                }
                RuleMatcher::ApplicationName(target_app) => {
                    app_matches(&app_lower, observation.window_title.as_deref(), target_app)
                }
                RuleMatcher::Any => true,
            };

            if matches {
                matching_rules.push((rule, rule.priority_weight));
            }
        }

        // 6. Conflict Resolution & Specificity Sorting: Higher scope unconditionally wins
        matching_rules.sort_by(|a, b| {
            b.0.scope
                .cmp(&a.0.scope)
                .then_with(|| b.1.cmp(&a.1))
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
