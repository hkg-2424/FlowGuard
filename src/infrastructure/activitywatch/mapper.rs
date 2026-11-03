use chrono::{DateTime, Duration, Utc};
use tracing::debug;

use super::models::{AfkEventData, AwRawEvent, WebEventData, WindowEventData};
use crate::domain::{
    ActivityCapabilities, ActivityObservation, BrowserContext, FreshnessState, IdleState,
    SourceHealth,
};

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

    let duration_millis = if event.duration > 0.0 {
        (event.duration * 1000.0) as i64
    } else {
        0
    };

    let latest_represented = event.timestamp + Duration::milliseconds(duration_millis);
    let age_seconds = (now - latest_represented).num_seconds();

    if age_seconds <= stale_threshold_secs as i64 {
        (FreshnessState::Fresh, Some(age_seconds))
    } else {
        (FreshnessState::Stale, Some(age_seconds))
    }
}

/// Normalizes raw ActivityWatch events into the FlowGuard `ActivityObservation` domain model.
pub fn map_events_to_observation(
    window_event: Option<&AwRawEvent>,
    afk_event: Option<&AwRawEvent>,
    web_event: Option<&AwRawEvent>,
    stale_threshold_secs: u64,
    discovery_diagnostics: &[String],
    has_web_bucket: bool,
) -> ActivityObservation {
    let now = Utc::now();
    let mut diagnostics = discovery_diagnostics.to_vec();

    // 1. Evaluate Freshness
    let (window_freshness, window_age) = check_freshness(window_event, now, stale_threshold_secs);
    let (idle_freshness, idle_age) = check_freshness(afk_event, now, stale_threshold_secs);
    let (browser_freshness, browser_age) = check_freshness(web_event, now, stale_threshold_secs);

    // 2. Normalize Window Event
    let (application, window_title) = match window_freshness {
        FreshnessState::Fresh => {
            let data: WindowEventData = window_event
                .and_then(|e| serde_json::from_value(e.data.clone()).ok())
                .unwrap_or_default();

            let app = data.app.and_then(|a| {
                let trimmed = a.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            });

            let title = data.title.and_then(|t| {
                let trimmed = t.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            });

            (app, title)
        }
        FreshnessState::Stale => {
            let age_desc = window_age
                .map(|a| format!("{}s old", a))
                .unwrap_or_else(|| "expired".to_string());
            diagnostics.push(format!(
                "Window watcher event is stale ({}, threshold: {}s). Window activity not current.",
                age_desc, stale_threshold_secs
            ));
            (None, None)
        }
        FreshnessState::Missing => {
            diagnostics.push("Window watcher bucket or event is missing.".to_string());
            (None, None)
        }
    };

    // 3. Normalize AFK Event
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
            let age_desc = idle_age
                .map(|a| format!("{}s old", a))
                .unwrap_or_else(|| "expired".to_string());
            diagnostics.push(format!(
                "AFK watcher event is stale ({}). Idle state marked as Unknown.",
                age_desc
            ));
            IdleState::Unknown
        }
        FreshnessState::Missing => IdleState::Unknown,
    };

    // 4. Normalize Browser Event
    let browser = match browser_freshness {
        FreshnessState::Fresh => {
            let data: WebEventData = web_event
                .and_then(|e| serde_json::from_value(e.data.clone()).ok())
                .unwrap_or_default();

            Some(BrowserContext {
                url: data.url.and_then(|u| {
                    let trimmed = u.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                }),
                title: data.title.and_then(|t| {
                    let trimmed = t.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                }),
                audible: data.audible,
                incognito: data.incognito,
            })
        }
        FreshnessState::Stale => {
            let age_desc = browser_age
                .map(|a| format!("{}s old", a))
                .unwrap_or_else(|| "expired".to_string());
            diagnostics.push(format!("Web watcher event is stale ({}).", age_desc));
            None
        }
        FreshnessState::Missing => {
            if has_web_bucket {
                diagnostics.push("Web watcher bucket has no events yet.".to_string());
            }
            None
        }
    };

    // 5. Derive Dynamic Capabilities
    let capabilities = ActivityCapabilities {
        application_tracking: window_freshness == FreshnessState::Fresh && application.is_some(),
        window_title_tracking: window_freshness == FreshnessState::Fresh && window_title.is_some(),
        idle_tracking: idle_freshness == FreshnessState::Fresh,
        browser_url_tracking: browser_freshness == FreshnessState::Fresh
            && browser.as_ref().and_then(|b| b.url.as_ref()).is_some(),
        incognito_detection: browser_freshness == FreshnessState::Fresh,
    };

    // 6. Determine Source Health
    // Degraded if reachable but essential window watcher is stale/missing OR optional AFK/Web is stale/missing
    let is_fully_healthy = window_freshness == FreshnessState::Fresh
        && idle_freshness == FreshnessState::Fresh
        && (!has_web_bucket || browser_freshness == FreshnessState::Fresh);

    let source_health = if is_fully_healthy {
        SourceHealth::Connected
    } else {
        SourceHealth::Degraded
    };

    debug!(
        "Observation mapped: app={:?}, idle={:?}, health={:?}, win_fresh={:?}, idle_fresh={:?}, web_fresh={:?}",
        application, idle_state, source_health, window_freshness, idle_freshness, browser_freshness
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
