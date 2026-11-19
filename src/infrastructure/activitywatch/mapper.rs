use chrono::{DateTime, Duration, Utc};
use tracing::debug;

use super::models::{AfkEventData, AwRawEvent, WebEventData, WindowEventData};
use crate::domain::{
    ActivityCapabilities, ActivityObservation, BrowserContext, FreshnessState, IdleState,
    SourceHealth,
};

/// Status or result of querying an individual watcher channel.
#[derive(Debug, Clone)]
pub enum WatcherResult<'a> {
    /// An event was successfully retrieved from the watcher bucket.
    Present(&'a AwRawEvent),
    /// Bucket exists and query succeeded, but contains no events (`[]`).
    Empty,
    /// Bucket is not configured or was not discovered.
    NotConfigured,
    /// Request to query this bucket timed out.
    Timeout(u64),
    /// Bucket was expected but does not exist in ActivityWatch (HTTP 404).
    MissingBucket(String),
    /// Other transport, HTTP 5xx, or serialization error.
    FetchError(String),
}

impl<'a> From<Option<&'a AwRawEvent>> for WatcherResult<'a> {
    fn from(opt: Option<&'a AwRawEvent>) -> Self {
        match opt {
            Some(ev) => WatcherResult::Present(ev),
            None => WatcherResult::NotConfigured,
        }
    }
}

/// Maximum plausible single event duration in seconds (24 hours).
/// Prevents numerical overflow when adding duration milliseconds to timestamps.
pub const MAX_PLAUSIBLE_DURATION_SECS: f64 = 86_400.0;

/// Tolerance window in seconds for events with timestamps slightly in the future
/// (due to local system clock skew or sub-second timer differences).
pub const CLOCK_SKEW_TOLERANCE_SECS: i64 = 5;

/// Calculates the latest timestamp represented by an event, including its duration.
///
/// Defensively rejects:
/// - NaN or Infinite durations (falling back to 0.0)
/// - Negative durations (falling back to 0.0)
/// - Implausibly large durations exceeding 24 hours (clamped to 86,400s)
///
/// Uses checked arithmetic to prevent panics during timestamp addition.
pub fn calculate_event_latest_time(event: &AwRawEvent) -> Option<DateTime<Utc>> {
    let duration_secs =
        if event.duration.is_nan() || event.duration.is_infinite() || event.duration <= 0.0 {
            0.0
        } else {
            event.duration.min(MAX_PLAUSIBLE_DURATION_SECS)
        };

    let duration_millis = (duration_secs * 1000.0) as i64;
    event
        .timestamp
        .checked_add_signed(Duration::milliseconds(duration_millis))
}

/// Sanitizes a URL by parsing it with [`url::Url`] and removing sensitive data:
/// - Strips HTTP basic/bearer authentication credentials (username and password).
/// - Preserves essential video identity queries for YouTube (`v=...`) while stripping all tracking/auth parameters.
/// - Strips URL fragments (#hash).
pub fn sanitize_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(mut parsed) = url::Url::parse(trimmed) {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        parsed.set_fragment(None);

        let is_youtube_watch = parsed
            .host_str()
            .map(|h| h == "youtube.com" || h.ends_with(".youtube.com") || h == "youtu.be")
            .unwrap_or(false)
            && parsed.path() == "/watch";

        if is_youtube_watch {
            let video_id = parsed
                .query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.to_string());
            parsed.set_query(None);
            if let Some(vid) = video_id {
                parsed.query_pairs_mut().append_pair("v", &vid);
            }
        } else {
            parsed.set_query(None);
        }

        Some(parsed.to_string())
    } else {
        // Fallback for non-standard, internal, or relative URI patterns
        let without_query = trimmed.split('?').next().unwrap_or(trimmed);
        let without_frag = without_query.split('#').next().unwrap_or(without_query);
        let without_auth = if let Some(idx) = without_frag.find("://") {
            let scheme_part = &without_frag[..idx + 3];
            let rest = &without_frag[idx + 3..];
            if let Some(at_idx) = rest.find('@') {
                format!("{}{}", scheme_part, &rest[at_idx + 1..])
            } else {
                without_frag.to_string()
            }
        } else {
            without_frag.to_string()
        };
        let cleaned = without_auth.trim();
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.to_string())
        }
    }
}

/// Sanitizes window titles to prevent terminal escape injection, log corruption, or UI breakage:
/// - Replaces ASCII and Unicode control characters (`\x00`..`\x1F`, `\x7F`..`\x9F`) with spaces.
/// - Collapses consecutive whitespace into a single space.
/// - Trims leading and trailing whitespace.
pub fn sanitize_title(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let cleaned: String = trimmed
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();

    let mut result = String::with_capacity(cleaned.len());
    let mut prev_space = false;
    for c in cleaned.trim().chars() {
        if c == ' ' {
            if !prev_space {
                result.push(' ');
                prev_space = true;
            }
        } else {
            result.push(c);
            prev_space = false;
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Sanitizes application executable identifiers:
/// - If a full file path was provided (e.g. `C:\Users\...\Code.exe` or `/usr/bin/code`),
///   extracts only the binary filename to prevent leaking local filesystem username paths.
/// - Sanitizes control characters and normalizes to lowercase.
pub fn sanitize_app_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = std::path::Path::new(trimmed);
    let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or(trimmed);

    let cleaned = sanitize_title(file_name)?;
    Some(cleaned.to_lowercase())
}

/// Calculates the freshness of an event against the current time and configured threshold.
///
/// Handles event duration:
/// ActivityWatch heartbeats extend the effective timestamp of an event:
/// `latest_represented_time = timestamp + duration`.
/// If duration is 0.0 or negative, `latest_represented_time = timestamp`.
pub fn check_freshness(
    event: Option<&AwRawEvent>,
    now: DateTime<Utc>,
    stale_threshold_secs: u64,
) -> (FreshnessState, Option<i64>) {
    let event = match event {
        Some(ev) => ev,
        None => return (FreshnessState::Missing, None),
    };

    let latest_represented = match calculate_event_latest_time(event) {
        Some(time) => time,
        None => return (FreshnessState::Stale, None),
    };

    let age_seconds = (now - latest_represented).num_seconds();

    // Check for future timestamp (clock skew defense):
    if age_seconds < -CLOCK_SKEW_TOLERANCE_SECS {
        // Event timestamp is too far in the future to be considered a valid current observation
        return (FreshnessState::Stale, Some(age_seconds));
    }

    let effective_age = age_seconds.max(0);

    if effective_age <= stale_threshold_secs as i64 {
        (FreshnessState::Fresh, Some(effective_age))
    } else {
        (FreshnessState::Stale, Some(age_seconds))
    }
}

/// Calculates the freshness of an AFK watcher event based on ActivityWatch heartbeat semantics.
///
/// `aw-watcher-afk` operates with distinct state semantics:
/// - When `status == "not-afk"`: Heartbeats occur while user interacts with keyboard or mouse.
///   When the user pauses (e.g. reading, thinking), heartbeats stop, but ActivityWatch will not
///   declare AFK until `afk_timeout_secs` (default 180s) of continuous inactivity elapse.
///   Therefore, a "not-afk" observation remains valid and fresh during this thinking pause.
///   It only becomes stale if `age > afk_timeout_secs + stale_threshold_secs`, which would
///   mean ActivityWatch failed to emit the expected AFK transition event.
/// - When `status == "afk"`: `aw-watcher-afk` continuously emits periodic heartbeats (every ~5s)
///   extending the event's duration for as long as the user remains AFK.
///   Therefore, an "afk" observation must be refreshed within `stale_threshold_secs`.
/// - For any other or missing status, it falls back to `stale_threshold_secs`.
pub fn check_afk_freshness(
    event: Option<&AwRawEvent>,
    now: DateTime<Utc>,
    stale_threshold_secs: u64,
    afk_timeout_secs: u64,
) -> (FreshnessState, Option<i64>) {
    let event = match event {
        Some(ev) => ev,
        None => return (FreshnessState::Missing, None),
    };

    let latest_represented = match calculate_event_latest_time(event) {
        Some(time) => time,
        None => return (FreshnessState::Stale, None),
    };

    let age_seconds = (now - latest_represented).num_seconds();

    // Check for future timestamp (clock skew defense):
    if age_seconds < -CLOCK_SKEW_TOLERANCE_SECS {
        return (FreshnessState::Stale, Some(age_seconds));
    }

    let data: AfkEventData = serde_json::from_value(event.data.clone()).unwrap_or_default();
    let threshold = match data.status.as_deref() {
        Some("not-afk") => (afk_timeout_secs + stale_threshold_secs) as i64,
        _ => stale_threshold_secs as i64,
    };

    let effective_age = age_seconds.max(0);

    if effective_age <= threshold {
        (FreshnessState::Fresh, Some(effective_age))
    } else {
        (FreshnessState::Stale, Some(age_seconds))
    }
}

/// Normalizes raw ActivityWatch events into the FlowGuard `ActivityObservation` domain model.
///
/// Architectural Invariants:
/// 1. `capabilities` represents what the provider is structural capable of observing,
///    derived from confirmed watcher discovery, NOT transient per-second event freshness.
/// 2. Transport errors and timeouts are explicitly handled and differentiated from absent buckets.
/// 3. Desktop monitoring remains healthy even if the optional browser extension times out or is stale.
pub fn map_events_to_observation(
    window_channel: WatcherResult<'_>,
    afk_channel: WatcherResult<'_>,
    web_channel: WatcherResult<'_>,
    capabilities: ActivityCapabilities,
    stale_threshold_secs: u64,
    afk_timeout_secs: u64,
    discovery_diagnostics: &[String],
) -> ActivityObservation {
    let now = Utc::now();
    let mut diagnostics = discovery_diagnostics.to_vec();

    // 1. Evaluate Window Freshness
    let (window_freshness, window_age, window_event) = match window_channel {
        WatcherResult::Present(ev) => {
            let (freshness, age) = check_freshness(Some(ev), now, stale_threshold_secs);
            (freshness, age, Some(ev))
        }
        WatcherResult::Empty => {
            diagnostics.push("Window watcher bucket has no recorded events yet.".to_string());
            (FreshnessState::Missing, None, None)
        }
        WatcherResult::NotConfigured => {
            diagnostics.push("Window watcher is not configured or was not discovered.".to_string());
            (FreshnessState::Missing, None, None)
        }
        WatcherResult::Timeout(ms) => {
            diagnostics.push(format!(
                "Window watcher request timed out after {}ms. Window activity could not be verified.",
                ms
            ));
            (FreshnessState::Stale, None, None)
        }
        WatcherResult::MissingBucket(b_id) => {
            diagnostics.push(format!(
                "Window watcher bucket '{}' was not found in ActivityWatch.",
                b_id
            ));
            (FreshnessState::Missing, None, None)
        }
        WatcherResult::FetchError(err) => {
            diagnostics.push(format!("Failed to query window watcher: {}", err));
            (FreshnessState::Stale, None, None)
        }
    };

    // 2. Normalize Window Event Data
    let (application, window_title) = match window_freshness {
        FreshnessState::Fresh => {
            let data: WindowEventData = window_event
                .and_then(|e| serde_json::from_value(e.data.clone()).ok())
                .unwrap_or_default();

            let app = data.app.as_deref().and_then(sanitize_app_name);
            let title = data.title.as_deref().and_then(sanitize_title);

            (app, title)
        }
        FreshnessState::Stale => {
            if let Some(age) = window_age {
                diagnostics.push(format!(
                    "Window watcher event is stale ({}s old, threshold: {}s). Window activity not current.",
                    age, stale_threshold_secs
                ));
            }
            (None, None)
        }
        FreshnessState::Missing => (None, None),
    };

    // 3. Evaluate AFK Freshness
    let (idle_freshness, idle_age, afk_event) = match afk_channel {
        WatcherResult::Present(ev) => {
            let (freshness, age) =
                check_afk_freshness(Some(ev), now, stale_threshold_secs, afk_timeout_secs);
            (freshness, age, Some(ev))
        }
        WatcherResult::Empty => {
            diagnostics.push("AFK watcher bucket has no recorded events yet.".to_string());
            (FreshnessState::Missing, None, None)
        }
        WatcherResult::NotConfigured => {
            diagnostics.push("AFK watcher is not configured or was not discovered.".to_string());
            (FreshnessState::Missing, None, None)
        }
        WatcherResult::Timeout(ms) => {
            diagnostics.push(format!(
                "AFK watcher request timed out after {}ms. Idle state could not be verified.",
                ms
            ));
            (FreshnessState::Stale, None, None)
        }
        WatcherResult::MissingBucket(b_id) => {
            diagnostics.push(format!(
                "AFK watcher bucket '{}' was not found in ActivityWatch.",
                b_id
            ));
            (FreshnessState::Missing, None, None)
        }
        WatcherResult::FetchError(err) => {
            diagnostics.push(format!("Failed to query AFK watcher: {}", err));
            (FreshnessState::Stale, None, None)
        }
    };

    // 4. Normalize AFK Event Data
    let idle_state = match idle_freshness {
        FreshnessState::Fresh => {
            let data: AfkEventData = afk_event
                .and_then(|e| serde_json::from_value(e.data.clone()).ok())
                .unwrap_or_default();

            match data.status.as_deref() {
                Some("afk") => IdleState::Afk,
                Some("not-afk") => IdleState::Active,
                _ => IdleState::Unknown,
            }
        }
        FreshnessState::Stale => {
            if let Some(age) = idle_age {
                diagnostics.push(format!(
                    "AFK watcher event is stale ({}s old). Idle state marked as Unknown.",
                    age
                ));
            }
            IdleState::Unknown
        }
        FreshnessState::Missing => IdleState::Unknown,
    };

    // 5. Evaluate Web Freshness
    let (browser_freshness, browser_age, web_event) = match web_channel {
        WatcherResult::Present(ev) => {
            let (freshness, age) = check_freshness(Some(ev), now, stale_threshold_secs);
            (freshness, age, Some(ev))
        }
        WatcherResult::Empty => {
            diagnostics.push("Web watcher bucket has no recorded events yet.".to_string());
            (FreshnessState::Missing, None, None)
        }
        WatcherResult::NotConfigured => (FreshnessState::Missing, None, None),
        WatcherResult::Timeout(ms) => {
            diagnostics.push(format!(
                "Web watcher request timed out after {}ms. Browser tab activity unavailable.",
                ms
            ));
            (FreshnessState::Stale, None, None)
        }
        WatcherResult::MissingBucket(b_id) => {
            diagnostics.push(format!(
                "Web watcher bucket '{}' was not found in ActivityWatch.",
                b_id
            ));
            (FreshnessState::Missing, None, None)
        }
        WatcherResult::FetchError(err) => {
            diagnostics.push(format!("Failed to query web watcher: {}", err));
            (FreshnessState::Stale, None, None)
        }
    };

    // 6. Normalize Browser Event Data
    let browser = match browser_freshness {
        FreshnessState::Fresh => {
            let data: WebEventData = web_event
                .and_then(|e| serde_json::from_value(e.data.clone()).ok())
                .unwrap_or_default();

            Some(BrowserContext {
                url: data.url.as_deref().and_then(sanitize_url),
                title: data.title.as_deref().and_then(sanitize_title),
                audible: data.audible,
                incognito: data.incognito,
            })
        }
        FreshnessState::Stale => {
            if let Some(age) = browser_age {
                diagnostics.push(format!("Web watcher event is stale ({}s old).", age));
            }
            None
        }
        FreshnessState::Missing => None,
    };

    // 7. Determine Source Health
    // Desktop activity monitoring is fully healthy when both essential desktop watchers
    // (window and AFK) are producing fresh observations.
    // - User AFK state (IdleState::Afk) is a normal user state transition and NEVER degrades health.
    // - Browser extension tracking is optional; an inactive or missing browser watcher does NOT degrade desktop monitoring.
    let is_fully_healthy =
        window_freshness == FreshnessState::Fresh && idle_freshness == FreshnessState::Fresh;

    let source_health = if is_fully_healthy {
        SourceHealth::Connected
    } else {
        SourceHealth::Degraded
    };

    debug!(
        "Observation mapped: app={:?}, idle={:?}, health={:?}, win_fresh={:?}, idle_fresh={:?}, web_fresh={:?}, caps={:?}",
        application, idle_state, source_health, window_freshness, idle_freshness, browser_freshness, capabilities
    );

    ActivityObservation {
        observed_at: now,
        application,
        window_title,
        browser,
        idle_state,
        capabilities,
        source_health,
        window_freshness,
        idle_freshness,
        browser_freshness,
        diagnostics,
    }
}
