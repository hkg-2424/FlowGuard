use chrono::{Duration, Utc};
use flowguard::domain::{ActivityCapabilities, FreshnessState, IdleState, SourceHealth};
use flowguard::infrastructure::activitywatch::mapper::{
    check_afk_freshness, check_freshness, map_events_to_observation, WatcherResult,
};
use flowguard::infrastructure::activitywatch::models::AwRawEvent;

fn full_capabilities() -> ActivityCapabilities {
    ActivityCapabilities {
        application_tracking: true,
        window_title_tracking: true,
        idle_tracking: true,
        browser_url_tracking: true,
        incognito_detection: true,
    }
}

fn desktop_only_capabilities() -> ActivityCapabilities {
    ActivityCapabilities {
        application_tracking: true,
        window_title_tracking: true,
        idle_tracking: true,
        browser_url_tracking: false,
        incognito_detection: false,
    }
}

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
fn test_afk_freshness_not_afk_within_timeout() {
    let now = Utc::now();
    // 30 seconds since last input, status is not-afk. afk_timeout is 180s, stale_threshold is 15s.
    // 30 <= 195 => Fresh
    let event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(30),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };

    let (state, age) = check_afk_freshness(Some(&event), now, 15, 180);
    assert_eq!(state, FreshnessState::Fresh);
    assert!(age.unwrap() >= 30);
}

#[test]
fn test_afk_freshness_not_afk_exceeding_timeout() {
    let now = Utc::now();
    // 210 seconds since last input, status is not-afk.
    // 210 > 180 + 15 (195s) => Stale
    let event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(210),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };

    let (state, age) = check_afk_freshness(Some(&event), now, 15, 180);
    assert_eq!(state, FreshnessState::Stale);
    assert!(age.unwrap() >= 210);
}

#[test]
fn test_afk_freshness_afk_heartbeat_fresh() {
    let now = Utc::now();
    // AFK event started 100 seconds ago, duration is 95s => latest_represented is 5s ago.
    // 5 <= 15 => Fresh
    let event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(100),
        duration: 95.0,
        data: serde_json::json!({ "status": "afk" }),
    };

    let (state, age) = check_afk_freshness(Some(&event), now, 15, 180);
    assert_eq!(state, FreshnessState::Fresh);
    assert!(age.unwrap() <= 6);
}

#[test]
fn test_afk_freshness_afk_heartbeat_stale() {
    let now = Utc::now();
    // AFK event started 100 seconds ago, duration is 75s => latest_represented is 25s ago.
    // 25 > 15 => Stale
    let event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(100),
        duration: 75.0,
        data: serde_json::json!({ "status": "afk" }),
    };

    let (state, age) = check_afk_freshness(Some(&event), now, 15, 180);
    assert_eq!(state, FreshnessState::Stale);
    assert!(age.unwrap() >= 25);
}

#[test]
fn test_afk_freshness_missing_event() {
    let now = Utc::now();
    let (state, age) = check_afk_freshness(None, now, 15, 180);
    assert_eq!(state, FreshnessState::Missing);
    assert!(age.is_none());
}

#[test]
fn test_afk_freshness_unknown_status_uses_standard_stale_threshold() {
    let now = Utc::now();
    let event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(20),
        duration: 0.0,
        data: serde_json::json!({ "status": "other" }),
    };

    let (state, _) = check_afk_freshness(Some(&event), now, 15, 180);
    assert_eq!(state, FreshnessState::Stale);
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
        Some(&window_event).into(),
        Some(&afk_event).into(),
        Some(&web_event).into(),
        full_capabilities(),
        15,
        180,
        &[],
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

    let obs = map_events_to_observation(
        Some(&stale_window).into(),
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );

    // Stale window must NOT be reported as current active application
    assert_eq!(obs.application, None);
    assert_eq!(obs.window_title, None);
    assert_eq!(obs.window_freshness, FreshnessState::Stale);
    assert_eq!(obs.source_health, SourceHealth::Degraded);
    // Structural capabilities are preserved across transient stale states (M1.1 Section 4)
    assert!(obs.capabilities.application_tracking);
    assert!(obs.capabilities.window_title_tracking);
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

    let obs = map_events_to_observation(
        WatcherResult::NotConfigured,
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );
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

    let obs = map_events_to_observation(
        Some(&bad_window).into(),
        WatcherResult::NotConfigured,
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );
    assert_eq!(obs.application, None);
    assert_eq!(obs.window_title, None);
    assert_eq!(obs.idle_state, IdleState::Unknown);
    assert_eq!(obs.browser, None);
}

#[test]
fn test_meaningful_change_detection() {
    let now = Utc::now();
    let mut obs1 = map_events_to_observation(
        WatcherResult::NotConfigured,
        WatcherResult::NotConfigured,
        WatcherResult::NotConfigured,
        ActivityCapabilities::default(),
        15,
        180,
        &[],
    );
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

// =========================================================================
// Automated Tests for AFK State & Source Health
// =========================================================================

#[test]
fn test_01_fresh_not_afk_produces_active() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };
    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };

    let obs = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );

    assert_eq!(obs.idle_state, IdleState::Active);
    assert_eq!(obs.idle_freshness, FreshnessState::Fresh);
    assert_eq!(obs.source_health, SourceHealth::Connected);
}

#[test]
fn test_02_fresh_afk_produces_afk() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };
    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "status": "afk" }),
    };

    let obs = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );

    assert_eq!(obs.idle_state, IdleState::Afk);
    assert_eq!(obs.idle_freshness, FreshnessState::Fresh);
    assert_eq!(obs.source_health, SourceHealth::Connected);
}

#[test]
fn test_03_valid_ongoing_afk_remains_afk_and_connected() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };
    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(600),
        duration: 596.0,
        data: serde_json::json!({ "status": "afk" }),
    };

    let obs = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );

    assert_eq!(obs.idle_state, IdleState::Afk);
    assert_eq!(obs.idle_freshness, FreshnessState::Fresh);
    assert_eq!(obs.source_health, SourceHealth::Connected);
}

#[test]
fn test_04_reading_pause_remains_active_and_connected() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "brave.exe", "title": "LeetCode Problem" }),
    };
    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(35),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };

    let obs = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );

    assert_eq!(obs.idle_state, IdleState::Active);
    assert_eq!(obs.idle_freshness, FreshnessState::Fresh);
    assert_eq!(obs.source_health, SourceHealth::Connected);
}

#[test]
fn test_05_genuinely_stale_afk_observation_produces_unknown_and_degraded() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };
    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(200),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };

    let obs = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );

    assert_eq!(obs.idle_state, IdleState::Unknown);
    assert_eq!(obs.idle_freshness, FreshnessState::Stale);
    assert_eq!(obs.source_health, SourceHealth::Degraded);
}

#[test]
fn test_06_genuinely_stale_ongoing_afk_produces_unknown_and_degraded() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };
    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(120),
        duration: 90.0,
        data: serde_json::json!({ "status": "afk" }),
    };

    let obs = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );

    assert_eq!(obs.idle_state, IdleState::Unknown);
    assert_eq!(obs.idle_freshness, FreshnessState::Stale);
    assert_eq!(obs.source_health, SourceHealth::Degraded);
}

#[test]
fn test_07_missing_afk_bucket_produces_unknown_and_degraded() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };

    let obs = map_events_to_observation(
        Some(&window_event).into(),
        WatcherResult::NotConfigured,
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );

    assert_eq!(obs.idle_state, IdleState::Unknown);
    assert_eq!(obs.idle_freshness, FreshnessState::Missing);
    assert_eq!(obs.source_health, SourceHealth::Degraded);
}

#[test]
fn test_08_afk_transition_does_not_change_health_to_degraded() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };

    let active_afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };
    let obs_active = map_events_to_observation(
        Some(&window_event).into(),
        Some(&active_afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );
    assert_eq!(obs_active.idle_state, IdleState::Active);
    assert_eq!(obs_active.source_health, SourceHealth::Connected);

    let idle_afk_event = AwRawEvent {
        id: Some(3),
        timestamp: now - Duration::seconds(1),
        duration: 0.0,
        data: serde_json::json!({ "status": "afk" }),
    };
    let obs_afk = map_events_to_observation(
        Some(&window_event).into(),
        Some(&idle_afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );
    assert_eq!(obs_afk.idle_state, IdleState::Afk);
    assert_eq!(obs_afk.source_health, SourceHealth::Connected);
}

#[test]
fn test_09_returning_from_afk_to_active_changes_idle_state() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };

    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(5),
        duration: 4.0,
        data: serde_json::json!({ "status": "afk" }),
    };
    let obs_afk = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );
    assert_eq!(obs_afk.idle_state, IdleState::Afk);

    let active_event = AwRawEvent {
        id: Some(3),
        timestamp: now - Duration::seconds(1),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };
    let obs_active = map_events_to_observation(
        Some(&window_event).into(),
        Some(&active_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );
    assert_eq!(obs_active.idle_state, IdleState::Active);
    assert_eq!(obs_active.source_health, SourceHealth::Connected);
    assert!(obs_active.has_meaningful_change(&obs_afk));
}

#[test]
fn test_10_missing_or_stale_browser_tracking_does_not_degrade_desktop_health() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };
    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };

    // Case A: Web bucket exists and is discovered, but has no events recorded yet (WatcherResult::Empty)
    let obs_empty_web = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        WatcherResult::Empty,
        full_capabilities(),
        15,
        180,
        &[],
    );
    assert_eq!(obs_empty_web.browser_freshness, FreshnessState::Missing);
    assert_eq!(obs_empty_web.source_health, SourceHealth::Connected);
    // Structural capability is retained (M1.1 Section 4)
    assert!(obs_empty_web.capabilities.browser_url_tracking);

    // Case B: Web bucket exists but event is 1 hour stale
    let stale_web_event = AwRawEvent {
        id: Some(3),
        timestamp: now - Duration::hours(1),
        duration: 0.0,
        data: serde_json::json!({
            "url": "https://example.com",
            "title": "Old Tab"
        }),
    };
    let obs_stale_web = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        Some(&stale_web_event).into(),
        full_capabilities(),
        15,
        180,
        &[],
    );
    assert_eq!(obs_stale_web.browser_freshness, FreshnessState::Stale);
    assert_eq!(obs_stale_web.source_health, SourceHealth::Connected); // Must NOT degrade desktop health!
                                                                      // Structural capability is retained (M1.1 Section 4)
    assert!(obs_stale_web.capabilities.browser_url_tracking);
}

// =========================================================================
// M1.1 Section 4 & 6: Capability Separation & Transport Failures Tests
// =========================================================================

#[test]
fn test_capabilities_independent_of_freshness() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };
    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };
    let stale_web_event = AwRawEvent {
        id: Some(3),
        timestamp: now - Duration::seconds(500),
        duration: 0.0,
        data: serde_json::json!({
            "url": "https://example.com",
            "title": "Quiescent Tab"
        }),
    };

    // Source configured with browser tracking capability
    let obs = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        Some(&stale_web_event).into(),
        full_capabilities(),
        15,
        180,
        &[],
    );

    // Freshness is correctly reported as Stale
    assert_eq!(obs.browser_freshness, FreshnessState::Stale);
    // Capability remains Available (true) because browser watcher is structurally supported
    assert!(obs.capabilities.browser_url_tracking);
    assert!(obs.capabilities.incognito_detection);

    // Conversely, if source was discovered without browser watcher:
    let obs_no_browser = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );
    assert_eq!(obs_no_browser.browser_freshness, FreshnessState::Missing);
    assert!(!obs_no_browser.capabilities.browser_url_tracking);
}

#[test]
fn test_window_timeout_marks_stale_and_degraded() {
    let now = Utc::now();
    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };

    // Window fetch times out
    let obs = map_events_to_observation(
        WatcherResult::Timeout(1500),
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );

    assert_eq!(obs.window_freshness, FreshnessState::Stale);
    assert_eq!(obs.source_health, SourceHealth::Degraded);
    assert_eq!(obs.application, None);
    assert!(obs
        .diagnostics
        .iter()
        .any(|d| d.contains("timed out after 1500ms")));
    assert!(!obs
        .diagnostics
        .iter()
        .any(|d| d.contains("has no recorded events yet")));
}

#[test]
fn test_browser_timeout_retains_desktop_health() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };
    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };

    // Browser fetch times out
    let obs = map_events_to_observation(
        Some(&window_event).into(),
        Some(&afk_event).into(),
        WatcherResult::Timeout(1500),
        full_capabilities(),
        15,
        180,
        &[],
    );

    assert_eq!(obs.browser_freshness, FreshnessState::Stale);
    assert_eq!(obs.source_health, SourceHealth::Connected); // Desktop remains Connected
    assert_eq!(obs.browser, None);
    assert!(obs
        .diagnostics
        .iter()
        .any(|d| d.contains("Web watcher request timed out")));
}

#[test]
fn test_afk_timeout_marks_unknown_and_degraded() {
    let now = Utc::now();
    let window_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "app": "code.exe", "title": "FlowGuard" }),
    };

    // AFK fetch times out
    let obs = map_events_to_observation(
        Some(&window_event).into(),
        WatcherResult::Timeout(1500),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );

    assert_eq!(obs.idle_freshness, FreshnessState::Stale);
    assert_eq!(obs.idle_state, IdleState::Unknown);
    assert_eq!(obs.source_health, SourceHealth::Degraded);
    assert!(obs
        .diagnostics
        .iter()
        .any(|d| d.contains("AFK watcher request timed out")));
}

#[test]
fn test_missing_bucket_404_distinguished_from_empty() {
    let now = Utc::now();
    let afk_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(2),
        duration: 0.0,
        data: serde_json::json!({ "status": "not-afk" }),
    };

    // HTTP 404 Missing Bucket
    let obs_missing = map_events_to_observation(
        WatcherResult::MissingBucket("aw-watcher-window_HOST".to_string()),
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );
    assert!(obs_missing
        .diagnostics
        .iter()
        .any(|d| d.contains("was not found in ActivityWatch")));

    // Empty Bucket
    let obs_empty = map_events_to_observation(
        WatcherResult::Empty,
        Some(&afk_event).into(),
        WatcherResult::NotConfigured,
        desktop_only_capabilities(),
        15,
        180,
        &[],
    );
    assert!(obs_empty
        .diagnostics
        .iter()
        .any(|d| d.contains("has no recorded events yet")));
}

// =========================================================================
// M1.1 Batch 2 Regression Tests: Time Safety, Privacy & Change Detection
// =========================================================================

#[test]
fn test_nan_and_infinite_duration_safety() {
    let now = Utc::now();
    let nan_event = AwRawEvent {
        id: Some(1),
        timestamp: now - Duration::seconds(2),
        duration: f64::NAN,
        data: serde_json::json!({}),
    };
    let (state, age) = check_freshness(Some(&nan_event), now, 15);
    assert_eq!(state, FreshnessState::Fresh);
    assert!(age.unwrap() <= 3);

    let inf_event = AwRawEvent {
        id: Some(2),
        timestamp: now - Duration::seconds(2),
        duration: f64::INFINITY,
        data: serde_json::json!({}),
    };
    let (state2, age2) = check_freshness(Some(&inf_event), now, 15);
    assert_eq!(state2, FreshnessState::Fresh);
    assert!(age2.unwrap() <= 3);

    let neg_event = AwRawEvent {
        id: Some(3),
        timestamp: now - Duration::seconds(2),
        duration: -100.0,
        data: serde_json::json!({}),
    };
    let (state3, age3) = check_freshness(Some(&neg_event), now, 15);
    assert_eq!(state3, FreshnessState::Fresh);
    assert!(age3.unwrap() <= 3);
}

#[test]
fn test_future_timestamp_clock_skew_tolerance() {
    let now = Utc::now();
    // 3 seconds in the future: within the 5s tolerance
    let skew_event = AwRawEvent {
        id: Some(1),
        timestamp: now + Duration::seconds(3),
        duration: 0.0,
        data: serde_json::json!({}),
    };
    let (state, age) = check_freshness(Some(&skew_event), now, 15);
    assert_eq!(state, FreshnessState::Fresh);
    assert_eq!(age, Some(0));
}

#[test]
fn test_future_timestamp_exceeding_skew_marked_stale() {
    let now = Utc::now();
    // 15 seconds in the future: exceeds 5s tolerance -> cannot be trusted as fresh current observation
    let future_event = AwRawEvent {
        id: Some(1),
        timestamp: now + Duration::seconds(15),
        duration: 0.0,
        data: serde_json::json!({}),
    };
    let (state, age) = check_freshness(Some(&future_event), now, 15);
    assert_eq!(state, FreshnessState::Stale);
    assert!(age.unwrap() < 0);
}

#[test]
fn test_url_sanitization_privacy() {
    use flowguard::infrastructure::activitywatch::mapper::sanitize_url;

    // Credentials and tracking parameters stripped
    let raw =
        "https://alice:secret123@example.com/dashboard?token=xyz999&utm_source=twitter#welcome";
    let sanitized = sanitize_url(raw).unwrap();
    assert_eq!(sanitized, "https://example.com/dashboard");

    // YouTube video ID preserved, tracking parameters stripped
    let yt_raw =
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ&feature=shared&utm_campaign=focus#t=45";
    let yt_sanitized = sanitize_url(yt_raw).unwrap();
    assert_eq!(yt_sanitized, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
}

#[test]
fn test_title_and_app_sanitization() {
    use flowguard::infrastructure::activitywatch::mapper::{sanitize_app_name, sanitize_title};

    // Control characters stripped
    let dirty_title = "My Document\t\r\n-- Confidential\x00\x1b";
    let clean_title = sanitize_title(dirty_title).unwrap();
    assert_eq!(clean_title, "My Document -- Confidential");

    // Full file paths reduced to executable basename
    let win_path = r"C:\Users\JohnDoe\AppData\Local\Programs\VSCode\Code.exe";
    let clean_app = sanitize_app_name(win_path).unwrap();
    assert_eq!(clean_app, "code.exe");
}

#[test]
fn test_change_detection_includes_idle_freshness_and_capabilities() {
    let mut obs1 = map_events_to_observation(
        WatcherResult::NotConfigured,
        WatcherResult::NotConfigured,
        WatcherResult::NotConfigured,
        full_capabilities(),
        15,
        180,
        &[],
    );
    obs1.application = Some("code.exe".to_string());
    obs1.idle_freshness = FreshnessState::Fresh;

    let mut obs2 = obs1.clone();
    assert!(!obs2.has_meaningful_change(&obs1));

    // Meaningful change when idle freshness degrades to Stale
    obs2.idle_freshness = FreshnessState::Stale;
    assert!(obs2.has_meaningful_change(&obs1));

    // Meaningful change when capabilities change
    let mut obs3 = obs1.clone();
    obs3.capabilities = ActivityCapabilities::none();
    assert!(obs3.has_meaningful_change(&obs1));

    // Meaningful change when diagnostics change
    let mut obs4 = obs1.clone();
    obs4.diagnostics = vec!["New warning".to_string()];
    assert!(obs4.has_meaningful_change(&obs1));
}
