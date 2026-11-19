use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::workflow::{CommitmentContract, PolicyDecision, Workflow, WorkflowSession};
use crate::domain::ActivityObservation;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullBackNotification {
    pub target: String,
    pub recovery_seconds: u32,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveSessionSnapshot {
    pub session_id: Option<String>,
    pub workflow_id: Option<String>,
    pub workflow_name: Option<String>,
    pub state: String,
    pub contract_type: String,
    pub min_reason_characters: u32,
    pub target_duration_minutes: Option<u32>,
    pub target_duration_seconds: Option<u64>,
    pub elapsed_total_seconds: u64,
    pub remaining_seconds: Option<u64>,
    pub focus_seconds: u64,
    pub grace_seconds: u64,
    pub distraction_seconds: u64,
    pub afk_seconds: u64,
    pub paused_seconds: u64,
    pub pull_back_count: u32,
    pub intervention_count: u32,
    pub quality_score: f64,
    pub policy_status: String,
    pub grace_remaining_seconds: u32,
    pub grace_total_seconds: u32,
    pub violation_target: Option<String>,
    pub current_app: Option<String>,
    pub current_title: Option<String>,
    pub current_domain: Option<String>,
    pub idle_state: String,
    pub source_type: String,
    pub source_connected: bool,
    pub last_pull_back: Option<PullBackNotification>,
    pub current_activity: Option<LiveActivityDto>,
    pub activity_summary: ActivityTrackerSummary,
    pub recent_activities: Vec<LiveActivityDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveActivityDto {
    pub app_name: String,
    pub window_title: String,
    pub category: String,
    pub detail: String,
    pub duration_seconds: u32,
    pub is_distraction: bool,
    pub is_short_or_reel: bool,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActivityTrackerSummary {
    pub current_category: String,
    pub current_detail: String,
    pub is_current_distraction: bool,
    pub is_current_short_or_reel: bool,
    pub current_duration_seconds: u32,
    pub total_shorts_count: u32,
    pub total_shorts_seconds: u64,
    pub total_distraction_seconds: u64,
    pub total_productive_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub target_duration_minutes: Option<u32>,
    pub contract_type: String,
    pub min_reason_characters: u32,
    pub default_grace_seconds: u32,
    pub allowed_apps: Vec<String>,
    pub allowed_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningAppDto {
    pub process_name: String,
    pub display_name: String,
    pub window_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkflowRequest {
    pub name: String,
    pub description: Option<String>,
    pub duration_minutes: Option<u32>,
    pub contract_type: String,
    pub min_reason_characters: Option<u32>,
    pub allowed_apps: Vec<String>,
    pub allowed_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartSessionRequest {
    pub workflow_id: String,
    pub duration_minutes: Option<u32>,
    pub contract_type: Option<String>,
    pub min_reason_characters: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopSessionRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistoryDto {
    pub id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub state: String,
    pub contract_type: String,
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
    pub quality_score: f64,
    pub drifts: Vec<DriftEventDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftEventDto {
    pub target_name: String,
    pub duration_seconds: u32,
    pub resolution: String,
    pub timestamp: DateTime<Utc>,
}

pub struct GuiState {
    pub active_workflow: Option<Workflow>,
    pub active_session: Option<WorkflowSession>,
    pub last_observation: Option<ActivityObservation>,
    pub last_decision: Option<PolicyDecision>,
    pub last_pull_back: Option<PullBackNotification>,
    pub source_name: String,
    pub source_connected: bool,
    pub activity_feed: std::collections::VecDeque<LiveActivityDto>,
    pub current_slice: Option<LiveActivityDto>,
    pub summary_stats: ActivityTrackerSummary,
}

impl GuiState {
    pub fn new(source_name: String) -> Self {
        Self {
            active_workflow: None,
            active_session: None,
            last_observation: None,
            last_decision: None,
            last_pull_back: None,
            source_name,
            source_connected: true,
            activity_feed: std::collections::VecDeque::new(),
            current_slice: None,
            summary_stats: ActivityTrackerSummary::default(),
        }
    }

    pub fn update_activity_feed(&mut self, obs: &ActivityObservation) {
        let app_name = obs
            .application
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());
        let window_title = obs.window_title.clone().unwrap_or_default();
        let browser_url = obs.browser.as_ref().and_then(|b| b.url.as_deref());

        let (category, detail, is_distraction, is_short_or_reel) =
            classify_activity(&app_name, &window_title, browser_url);

        let now = Utc::now();

        let switched = match &self.current_slice {
            Some(curr) => curr.app_name != app_name || curr.detail != detail,
            None => true,
        };

        if switched {
            if let Some(prev) = self.current_slice.take() {
                if prev.duration_seconds >= 2 {
                    if prev.is_short_or_reel {
                        self.summary_stats.total_shorts_count += 1;
                        self.summary_stats.total_shorts_seconds += prev.duration_seconds as u64;
                    }
                    if prev.is_distraction {
                        self.summary_stats.total_distraction_seconds +=
                            prev.duration_seconds as u64;
                    } else {
                        self.summary_stats.total_productive_seconds += prev.duration_seconds as u64;
                    }

                    if self.activity_feed.len() >= 50 {
                        self.activity_feed.pop_back();
                    }
                    self.activity_feed.push_front(prev);
                }
            }

            self.current_slice = Some(LiveActivityDto {
                app_name: app_name.clone(),
                window_title: window_title.clone(),
                category: category.clone(),
                detail: detail.clone(),
                duration_seconds: 1,
                is_distraction,
                is_short_or_reel,
                timestamp: now,
            });
        } else if let Some(ref mut curr) = self.current_slice {
            curr.duration_seconds += 1;
            curr.timestamp = now;
        }

        self.summary_stats.current_category = category;
        self.summary_stats.current_detail = detail;
        self.summary_stats.is_current_distraction = is_distraction;
        self.summary_stats.is_current_short_or_reel = is_short_or_reel;
        self.summary_stats.current_duration_seconds = self
            .current_slice
            .as_ref()
            .map(|s| s.duration_seconds)
            .unwrap_or(0);
    }

    pub fn snapshot(&self) -> LiveSessionSnapshot {
        let (current_app, current_title, current_domain, idle_str) = match &self.last_observation {
            Some(obs) => {
                let domain = obs
                    .browser
                    .as_ref()
                    .and_then(|b| b.url.as_deref())
                    .and_then(|u| url::Url::parse(u).ok())
                    .and_then(|u| u.host_str().map(|h| h.to_string()))
                    .or_else(|| {
                        // If no browser url, check if title has a domain
                        obs.window_title.as_deref().and_then(|t| {
                            let t_lower = t.to_lowercase();
                            if t_lower.contains("github") {
                                Some("github.com".to_string())
                            } else if t_lower.contains("wikipedia") {
                                Some("wikipedia.org".to_string())
                            } else if t_lower.contains("rust") {
                                Some("doc.rust-lang.org".to_string())
                            } else if t_lower.contains("leetcode") {
                                Some("leetcode.com".to_string())
                            } else if t_lower.contains("youtube") {
                                Some("youtube.com".to_string())
                            } else if t_lower.contains("reddit") {
                                Some("reddit.com".to_string())
                            } else {
                                None
                            }
                        })
                    });
                (
                    obs.application.clone(),
                    obs.window_title.clone(),
                    domain,
                    obs.idle_state.as_str().to_string(),
                )
            }
            None => (None, None, None, "Unknown".to_string()),
        };

        let (policy_status, grace_remaining, grace_total, violation_target) =
            match &self.last_decision {
                Some(PolicyDecision::Allowed { .. }) => ("Allowed".to_string(), 0, 30, None),
                Some(PolicyDecision::GracePeriod {
                    remaining_seconds,
                    total_grace_seconds,
                    target_name,
                    ..
                }) => (
                    "GracePeriod".to_string(),
                    *remaining_seconds,
                    *total_grace_seconds,
                    Some(target_name.clone()),
                ),
                Some(PolicyDecision::InterventionRequired { target_name, .. }) => (
                    "InterventionRequired".to_string(),
                    0,
                    30,
                    Some(target_name.clone()),
                ),
                Some(PolicyDecision::Ignored { .. }) => ("Ignored".to_string(), 0, 30, None),
                _ => ("None".to_string(), 0, 30, None),
            };

        if let Some(session) = &self.active_session {
            let wf_name = self
                .active_workflow
                .as_ref()
                .map(|w| w.name.clone())
                .unwrap_or_else(|| session.workflow_id.0.clone());

            let (contract_str, min_chars) = match &session.commitment_contract {
                CommitmentContract::Standard => ("Standard".to_string(), 0),
                CommitmentContract::AccountableBreakable {
                    min_reason_characters,
                } => ("AccountableBreakable".to_string(), *min_reason_characters),
                CommitmentContract::Unbreakable { .. } => ("Unbreakable".to_string(), 0),
            };

            let target_duration_mins = self
                .active_workflow
                .as_ref()
                .and_then(|w| w.target_duration_minutes);

            let target_duration_secs = target_duration_mins.map(|m| (m as u64) * 60);

            let total_tracked = session.total_focus_seconds
                + session.total_grace_seconds
                + session.total_distraction_seconds;

            let quality_score = if total_tracked > 0 {
                (session.total_focus_seconds as f64 / total_tracked as f64) * 100.0
            } else {
                100.0
            };

            let elapsed_total = session.total_focus_seconds
                + session.total_grace_seconds
                + session.total_distraction_seconds
                + session.total_paused_seconds;

            let remaining_secs =
                target_duration_secs.map(|target| target.saturating_sub(elapsed_total));

            LiveSessionSnapshot {
                session_id: Some(session.id.0.clone()),
                workflow_id: Some(session.workflow_id.0.clone()),
                workflow_name: Some(wf_name),
                state: format!("{:?}", session.state),
                contract_type: contract_str,
                min_reason_characters: min_chars,
                target_duration_minutes: target_duration_mins,
                target_duration_seconds: target_duration_secs,
                elapsed_total_seconds: elapsed_total,
                remaining_seconds: remaining_secs,
                focus_seconds: session.total_focus_seconds,
                grace_seconds: session.total_grace_seconds,
                distraction_seconds: session.total_distraction_seconds,
                afk_seconds: session.total_afk_seconds,
                paused_seconds: session.total_paused_seconds,
                pull_back_count: session.pull_back_count,
                intervention_count: session.intervention_count,
                quality_score,
                policy_status,
                grace_remaining_seconds: grace_remaining,
                grace_total_seconds: grace_total,
                violation_target,
                current_app,
                current_title,
                current_domain,
                idle_state: idle_str,
                source_type: self.source_name.clone(),
                source_connected: self.source_connected,
                last_pull_back: self.last_pull_back.clone(),
                current_activity: self.current_slice.clone(),
                activity_summary: self.summary_stats.clone(),
                recent_activities: self.activity_feed.iter().cloned().collect(),
            }
        } else {
            LiveSessionSnapshot {
                session_id: None,
                workflow_id: None,
                workflow_name: None,
                state: "None".to_string(),
                contract_type: "Standard".to_string(),
                min_reason_characters: 0,
                target_duration_minutes: None,
                target_duration_seconds: None,
                elapsed_total_seconds: 0,
                remaining_seconds: None,
                focus_seconds: 0,
                grace_seconds: 0,
                distraction_seconds: 0,
                afk_seconds: 0,
                paused_seconds: 0,
                pull_back_count: 0,
                intervention_count: 0,
                quality_score: 100.0,
                policy_status: "None".to_string(),
                grace_remaining_seconds: 0,
                grace_total_seconds: 30,
                violation_target: None,
                current_app,
                current_title,
                current_domain,
                idle_state: idle_str,
                source_type: self.source_name.clone(),
                source_connected: self.source_connected,
                last_pull_back: None,
                current_activity: self.current_slice.clone(),
                activity_summary: self.summary_stats.clone(),
                recent_activities: self.activity_feed.iter().cloned().collect(),
            }
        }
    }
}

pub fn classify_activity(
    app_name: &str,
    window_title: &str,
    browser_url: Option<&str>,
) -> (String, String, bool, bool) {
    let app_lower = app_name.to_lowercase();
    let title_trim = window_title.trim();
    let title_lower = title_trim.to_lowercase();
    let url_lower = browser_url.unwrap_or("").to_lowercase();

    // 1. YouTube Shorts Detection
    let is_yt_shorts = url_lower.contains("/shorts/")
        || (title_lower.contains("youtube")
            && (title_lower.contains("#shorts")
                || title_lower.contains("shorts")
                || title_lower.starts_with("shorts -")));

    if is_yt_shorts {
        let clean = clean_media_title(title_trim);
        return ("YouTube Shorts".to_string(), clean, true, true);
    }

    // 2. Instagram Reels Detection
    let is_ig_reels = url_lower.contains("instagram.com/reels/")
        || url_lower.contains("instagram.com/reel/")
        || (title_lower.contains("instagram") && title_lower.contains("reel"));

    if is_ig_reels {
        let clean = clean_media_title(title_trim);
        return ("Instagram Reels".to_string(), clean, true, true);
    }

    // 3. TikTok Detection
    if url_lower.contains("tiktok.com") || title_lower.contains("tiktok") {
        let clean = clean_media_title(title_trim);
        return ("TikTok".to_string(), clean, true, true);
    }

    // 4. Regular YouTube Video
    if url_lower.contains("youtube.com/watch") || title_lower.contains("youtube") {
        let clean = clean_media_title(title_trim);
        return ("YouTube Video".to_string(), clean, false, false);
    }

    // 5. Social Media & Entertainment Distractions
    let is_social_or_streaming = url_lower.contains("reddit.com")
        || title_lower.contains("reddit")
        || url_lower.contains("x.com")
        || url_lower.contains("twitter.com")
        || title_lower.contains("twitter")
        || url_lower.contains("instagram.com")
        || title_lower.contains("instagram")
        || url_lower.contains("facebook.com")
        || title_lower.contains("facebook")
        || url_lower.contains("twitch.tv")
        || title_lower.contains("twitch")
        || url_lower.contains("netflix.com")
        || title_lower.contains("netflix")
        || url_lower.contains("disneyplus.com")
        || url_lower.contains("primevideo.com")
        || app_lower == "discord.exe"
        || app_lower == "steam.exe";

    if is_social_or_streaming {
        let name = if title_lower.contains("reddit") || url_lower.contains("reddit") {
            "Reddit"
        } else if title_lower.contains("twitter")
            || url_lower.contains("x.com")
            || url_lower.contains("twitter")
        {
            "Twitter / X"
        } else if title_lower.contains("instagram") || url_lower.contains("instagram.com") {
            "Instagram"
        } else if title_lower.contains("facebook") || url_lower.contains("facebook.com") {
            "Facebook"
        } else if title_lower.contains("twitch") || url_lower.contains("twitch.tv") {
            "Twitch"
        } else if title_lower.contains("netflix") || url_lower.contains("netflix.com") {
            "Netflix"
        } else if url_lower.contains("disneyplus.com") {
            "Disney+"
        } else if url_lower.contains("primevideo.com") {
            "Prime Video"
        } else if app_lower == "discord.exe" {
            "Discord"
        } else if app_lower == "steam.exe" {
            "Steam"
        } else {
            "Social Media"
        };
        let clean = clean_media_title(title_trim);
        return (name.to_string(), clean, true, false);
    }

    // 6. Development & Coding
    let is_dev = app_lower == "code.exe"
        || app_lower.contains("antigravity")
        || title_lower.contains("antigravity")
        || app_lower == "windowsterminal.exe"
        || app_lower == "powershell.exe"
        || app_lower == "cmd.exe"
        || app_lower == "devenv.exe"
        || app_lower == "idea64.exe"
        || url_lower.contains("github.com")
        || title_lower.contains("github")
        || url_lower.contains("docs.rs")
        || title_lower.contains("docs.rs");

    if is_dev {
        let name = if app_lower == "code.exe" {
            "VS Code"
        } else if app_lower.contains("antigravity") || title_lower.contains("antigravity") {
            "Antigravity"
        } else if app_lower == "windowsterminal.exe" || app_lower == "powershell.exe" {
            "Terminal"
        } else if url_lower.contains("github.com") || title_lower.contains("github") {
            "GitHub"
        } else {
            "Development"
        };
        return (name.to_string(), title_trim.to_string(), false, false);
    }

    // 7. Learning & Education
    let is_learning = url_lower.contains("udemy.com")
        || title_lower.contains("udemy")
        || url_lower.contains("coursera.org")
        || title_lower.contains("coursera")
        || url_lower.contains("wikipedia.org")
        || title_lower.contains("wikipedia");

    if is_learning {
        let name = if title_lower.contains("udemy") || url_lower.contains("udemy") {
            "Udemy"
        } else if title_lower.contains("coursera") || url_lower.contains("coursera") {
            "Coursera"
        } else {
            "Learning"
        };
        return (name.to_string(), title_trim.to_string(), false, false);
    }

    // 8. General Productivity
    let is_prod = app_lower == "obsidian.exe"
        || app_lower == "notion.exe"
        || app_lower == "winword.exe"
        || app_lower == "excel.exe"
        || app_lower == "acrobat.exe"
        || app_lower == "notepad.exe";

    if is_prod {
        let name = if app_lower == "obsidian.exe" {
            "Obsidian"
        } else if app_lower == "notion.exe" {
            "Notion"
        } else if app_lower == "winword.exe" {
            "Word"
        } else {
            "Productivity"
        };
        return (name.to_string(), title_trim.to_string(), false, false);
    }

    // Default Browser or App
    if app_lower == "chrome.exe"
        || app_lower == "msedge.exe"
        || app_lower == "brave.exe"
        || app_lower == "firefox.exe"
    {
        (
            "Web Browsing".to_string(),
            title_trim.to_string(),
            false,
            false,
        )
    } else {
        (
            crate::infrastructure::native_windows::api::friendly_display_name(
                app_name,
                window_title,
            ),
            title_trim.to_string(),
            false,
            false,
        )
    }
}

fn clean_media_title(raw: &str) -> String {
    let mut s = raw.to_string();
    let suffixes = [
        " - YouTube",
        " • Instagram",
        " | Netflix",
        " - Netflix",
        " - Twitch",
        " - Google Chrome",
        " — Google Chrome",
        " - Brave",
        " — Brave",
        " - Microsoft Edge",
        " — Microsoft Edge",
        " - Mozilla Firefox",
        " — Mozilla Firefox",
    ];
    for suffix in suffixes {
        s = s.replace(suffix, "");
    }
    s.trim().to_string()
}
