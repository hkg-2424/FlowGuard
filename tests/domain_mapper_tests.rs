use chrono::{Duration, Utc};
use flowguard::domain::{FreshnessState, IdleState, SourceHealth};
use flowguard::infrastructure::activitywatch::mapper::{
    check_freshness, map_events_to_observation,
};
use flowguard::infrastructure::activitywatch::models::AwRawEvent;

#[test]
fn test_freshness_calculation_within_threshold() {
    let now = Utc::now();
    let event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(5),
        duration: 0.0,
        data: serde_json::json!({}),
    };

    let (state, age) = check_freshness(Some(&event), now, 15);
    assert_eq!(state, FreshnessState::Fresh);
    assert!(age.unwrap() <= 6);
}

#[test]
fn test_freshness_calculation_exceeding_threshold() {
    let now = Utc::now();
    let event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(20),
        duration: 0.0,
        data: serde_json::json!({}),
    };

    let (state, age) = check_freshness(Some(&event), now, 15);
    assert_eq!(state, FreshnessState::Stale);
    assert!(age.unwrap() >= 20);
}

#[test]
fn test_freshness_calculation_with_duration_extension() {
    let now = Utc::now();
    // Event started 20 seconds ago, but lasted 10 seconds, so represented end is 10s ago
    let event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(20),
        duration: 10.0,
        data: serde_json::json!({}),
    };

    let (state, age) = check_freshness(Some(&event), now, 15);
    // Age from latest represented time should be ~10s, which is within the 15s threshold
    assert_eq!(state, FreshnessState::Fresh);
    assert!(age.unwrap() <= 11);
}

#[test]
fn test_freshness_missing_event() {
    let now = Utc::now();
    let (state, age) = check_freshness(None, now, 15);
    assert_eq!(state, FreshnessState::Missing);
    assert!(age.is_none());
}

#[test]
fn test_map_valid_fresh_events() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(101),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({
            "app": "code.exe",
            "title": "src/main.rs - FlowGuard"
        }),
    };

    let afk_event = AwRawEvent {
        id: Some(102),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({
            "status": "not-afk"
        }),
    };

    let web_event = AwRawEvent {
        id: Some(103),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({
            "url": "https://crates.io/crates/reqwest",
            "title": "reqwest - crates.io",
            "audible": false,
            "incognito": false
        }),
    };

    let obs = map_events_to_observation(
        Some(&window_event),
        Some(&afk_event),
        Some(&web_event),
        15,
        &[],
        true,
    );

    assert_eq!(obs.application.as_deref(), Some("code.exe"));
    assert_eq!(obs.window_title.as_deref(), Some("src/main.rs - FlowGuard"));
    assert_eq!(obs.idle_state, IdleState::Active);
    assert_eq!(obs.source_health, SourceHealth::Connected);
    assert_eq!(obs.window_freshness, FreshnessState::Fresh);
    assert_eq!(obs.idle_freshness, FreshnessState::Fresh);
    assert_eq!(obs.browser_freshness, FreshnessState::Fresh);

    let browser = obs.browser.expect("browser context should be present");
    assert_eq!(
        browser.url.as_deref(),
        Some("https://crates.io/crates/reqwest")
    );
    assert_eq!(browser.title.as_deref(), Some("reqwest - crates.io"));
    assert_eq!(browser.audible, Some(false));
    assert_eq!(browser.incognito, Some(false));

    assert!(obs.capabilities.application_tracking);
    assert!(obs.capabilities.window_title_tracking);
    assert!(obs.capabilities.idle_tracking);
    assert!(obs.capabilities.browser_url_tracking);
    assert!(obs.capabilities.incognito_detection);
}

#[test]
fn test_map_stale_window_event_does_not_fabricate_active_window() {
    let now = Utc::now();
    let stale_window = AwRawEvent {
        id: Some(201),
        timestamp: now - Duration::seconds(60),
        duration: 0.0,
        data: serde_json::json!({
            "app": "chrome.exe",
            "title": "YouTube - React Tutorial"
        }),
    };

    let afk_event = AwRawEvent {
        id: Some(202),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };

    let obs =
        map_events_to_observation(Some(&stale_window), Some(&afk_event), None, 15, &[], false);

    // Stale window must NOT be reported as current active application
    assert_eq!(obs.application, None);
    assert_eq!(obs.window_title, None);
    assert_eq!(obs.window_freshness, FreshnessState::Stale);
    assert_eq!(obs.source_health, SourceHealth::Degraded);
    assert!(!obs.capabilities.application_tracking);
    assert!(!obs.capabilities.window_title_tracking);
    assert!(obs.diagnostics.iter().any(|d| d.contains("stale")));
}

#[test]
fn test_map_afk_status() {
    let now = Utc::now();
    let afk_event = AwRawEvent {
        id: Some(301),
        timestamp: now - Duration::seconds(1),
        duration: 0.0,
        data: serde_json::json!({ "status": "afk" }),
    };

    let obs = map_events_to_observation(None, Some(&afk_event), None, 15, &[], false);
    assert_eq!(obs.idle_state, IdleState::Afk);
}

#[test]
fn test_map_missing_and_malformed_event_fields() {
    let now = Utc::now();
    // Window event with completely missing app and title fields
    let bad_window = AwRawEvent {
        id: Some(401),
        timestamp: now - Duration::seconds(1),
        duration: 0.0,
        data: serde_json::json!({
            "unexpected_field": 12345
        }),
    };

    let obs = map_events_to_observation(Some(&bad_window), None, None, 15, &[], false);
    assert_eq!(obs.application, None);
    assert_eq!(obs.window_title, None);
    assert_eq!(obs.idle_state, IdleState::Unknown);
    assert_eq!(obs.browser, None);
}

#[test]
fn test_meaningful_change_detection() {
    let now = Utc::now();
    let mut obs1 = map_events_to_observation(None, None, None, 15, &[], false);
    obs1.application = Some("code.exe".to_string());
    obs1.window_title = Some("lib.rs".to_string());
    obs1.idle_state = IdleState::Active;

    let mut obs2 = obs1.clone();
    obs2.observed_at = now + Duration::seconds(1);

    // No meaningful change if only timestamp changes
    assert!(!obs2.has_meaningful_change(&obs1));

    // Meaningful change when application changes
    obs2.application = Some("chrome.exe".to_string());
    assert!(obs2.has_meaningful_change(&obs1));

    // Meaningful change when idle state changes
    let mut obs3 = obs1.clone();
    obs3.idle_state = IdleState::Afk;
    assert!(obs3.has_meaningful_change(&obs1));
}
