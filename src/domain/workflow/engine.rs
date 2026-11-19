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

    // If pattern contains path or query (like "youtube.com/watch" or specific video id),
    // require exact substring match rather than extracting domain tokens like "youtube"
    // which would otherwise match every unrelated video on YouTube.
    if sld.contains('/') || sld.contains('?') || sld.contains('=') {
        return title_lower.contains(sld);
    }

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

pub fn is_browser_app(app_name: &str) -> bool {
    let lower = app_name.to_lowercase();
    let clean = lower.strip_suffix(".exe").unwrap_or(&lower);
    matches!(
        clean,
        "chrome"
            | "msedge"
            | "brave"
            | "firefox"
            | "opera"
            | "vivaldi"
            | "arc"
            | "safari"
            | "iexplore"
    )
}

/// Identifies algorithmic short-form video feeds (YouTube Shorts, Instagram Reels, TikTok)
/// across browser URLs and native window titles.
pub fn is_short_form_media(observation: &ActivityObservation) -> bool {
    let url_lower = observation
        .browser
        .as_ref()
        .and_then(|b| b.url.as_deref())
        .unwrap_or("")
        .to_lowercase();
    let title_lower = observation
        .window_title
        .as_deref()
        .unwrap_or("")
        .to_lowercase();

    // 1. Explicit URL routes
    if url_lower.contains("/shorts/")
        || url_lower.contains("instagram.com/reels/")
        || url_lower.contains("instagram.com/reel/")
        || url_lower.contains("tiktok.com")
    {
        return true;
    }

    // 2. Video platform hashtag and title markers
    let is_media_platform = title_lower.contains("youtube")
        || title_lower.contains("instagram")
        || title_lower.contains("tiktok")
        || url_lower.contains("youtube.com")
        || url_lower.contains("instagram.com")
        || url_lower.contains("tiktok.com");

    if is_media_platform {
        let markers = [
            "#shorts",
            "#short",
            "#ytshorts",
            "#viralshort",
            "#viralshorts",
            "short video",
            "shorts -",
            " - shorts",
            "/shorts",
            "shorts",
            "#reels",
            "#reel",
            "/reels",
            "/reel",
            "reels",
            "tiktok",
        ];
        for m in &markers {
            if title_lower.contains(m) {
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

    let app_clean = app_lower.strip_suffix(".exe").unwrap_or(&app_lower);
    let target_clean = target_lower.strip_suffix(".exe").unwrap_or(&target_lower);

    // 1. Direct exact or sanitized match (e.g. "code.exe" == "code.exe" or "code" == "code")
    if app_lower == target_lower || app_clean == target_clean {
        return true;
    }

    // 2. Friendly display name / alias mapping
    let matches_alias = match target_clean {
        "visual studio code" | "vs code" | "vscode" | "code" => app_clean == "code",
        "antigravity" => {
            app_clean.contains("antigravity")
                || window_title
                    .map(|t| t.to_lowercase().contains("antigravity"))
                    .unwrap_or(false)
        }
        "terminal" | "windows terminal" | "windowsterminal" => {
            app_clean == "windowsterminal"
                || app_clean == "cmd"
                || app_clean == "powershell"
                || app_clean == "pwsh"
        }
        "chrome" | "google chrome" => app_clean == "chrome",
        "edge" | "microsoft edge" => app_clean == "msedge",
        "brave" | "brave browser" => app_clean == "brave",
        "firefox" | "mozilla firefox" => app_clean == "firefox",
        "notion" => app_clean == "notion",
        "obsidian" => app_clean == "obsidian",
        "slack" => app_clean == "slack",
        "discord" => app_clean == "discord",
        "spotify" => app_clean == "spotify",
        "steam" => app_clean == "steam",
        "visual studio" | "devenv" => app_clean == "devenv",
        "intellij" | "intellij idea" | "idea" | "idea64" => {
            app_clean == "idea64" || app_clean == "idea"
        }
        "pycharm" | "pycharm64" => app_clean == "pycharm64" || app_clean == "pycharm",
        "word" | "microsoft word" | "winword" => app_clean == "winword",
        "excel" | "microsoft excel" => app_clean == "excel",
        "powerpoint" | "powerpnt" => app_clean == "powerpnt",
        "onenote" => app_clean == "onenote",
        "acrobat" | "adobe acrobat" | "pdf reader" | "foxitreader" => {
            app_clean == "acrobat" || app_clean == "foxitreader"
        }
        _ => false,
    };

    if matches_alias {
        return true;
    }

    // 3. Fallback: Check if window title explicitly contains the clean target
    if let Some(title) = window_title {
        let title_lower = title.to_lowercase();
        if target_clean.len() >= 3 && title_lower.contains(target_clean) {
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

        // 4b. Restrict short-form algorithmic media (Shorts, Reels, TikTok) unless explicitly permitted
        if is_short_form_media(observation) {
            let target_name = browser_url.unwrap_or(app_name).to_string();
            let shorts_rule_id = super::entities::RuleId::new("rule-restrict-shorts");
            let grace_duration = workflow.default_grace_period_seconds;

            match timer.tick(&shorts_rule_id, &target_name, grace_duration) {
                TimerStatus::GracePeriod {
                    remaining_seconds,
                    total_grace_seconds,
                } => {
                    return PolicyDecision::GracePeriod {
                        rule_id: shorts_rule_id,
                        remaining_seconds,
                        total_grace_seconds,
                        target_name,
                    };
                }
                TimerStatus::Expired { .. } => {
                    return PolicyDecision::InterventionRequired {
                        rule_id: shorts_rule_id,
                        target_name,
                        reason:
                            "Short-form algorithmic media (Shorts/Reels) violates focus intention"
                                .to_string(),
                    };
                }
                TimerStatus::Idle => {
                    return PolicyDecision::Allowed {
                        rule_id: shorts_rule_id,
                        scope: RuleScope::WorkflowDefault,
                    };
                }
            }
        }

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
                        if observation.browser.is_some() || is_browser_app(&app_lower) {
                            title_matches_domain(title, pattern)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                RuleMatcher::ApplicationName(target_app) => {
                    let is_match =
                        app_matches(&app_lower, observation.window_title.as_deref(), target_app);
                    if is_match && is_browser_app(&app_lower) {
                        // If workflow specifies domain allow rules, an allowed browser container
                        // must not bypass domain restrictions
                        let has_domain_rules = workflow.rules.iter().any(|r| {
                            r.is_active(now) && matches!(r.matcher, RuleMatcher::DomainPattern(_))
                        });
                        if has_domain_rules {
                            let matches_domain = workflow.rules.iter().any(|r| {
                                if let RuleMatcher::DomainPattern(pat) = &r.matcher {
                                    if let Some(ref h) = browser_host {
                                        domain_matches(h, pat)
                                    } else if let Some(ref t) = observation.window_title {
                                        title_matches_domain(t, pat)
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            });
                            matches_domain
                        } else {
                            true
                        }
                    } else {
                        is_match
                    }
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
