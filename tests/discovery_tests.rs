use std::collections::HashMap;

use flowguard::config::BucketOverridesConfig;
use flowguard::infrastructure::activitywatch::discovery::discover_buckets;
use flowguard::infrastructure::activitywatch::models::BucketMetadata;

fn make_bucket(id: &str, event_type: &str, client: &str, hostname: &str) -> BucketMetadata {
    BucketMetadata {
        id: id.to_string(),
        created: None,
        name: None,
        event_type: event_type.to_string(),
        client: client.to_string(),
        hostname: hostname.to_string(),
        last_updated: None,
    }
}

// 1. Single candidate selection
#[test]
fn test_01_single_candidate_selection() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "aw-watcher-window_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-window_MYHOST",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );
    buckets.insert(
        "aw-watcher-afk_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-afk_MYHOST",
            "afkstatus",
            "aw-watcher-afk",
            "MYHOST",
        ),
    );

    let discovered = discover_buckets(&buckets, "MYHOST", None);
    assert_eq!(
        discovered.window_bucket.as_deref(),
        Some("aw-watcher-window_MYHOST")
    );
    assert_eq!(
        discovered.afk_bucket.as_deref(),
        Some("aw-watcher-afk_MYHOST")
    );
    assert_eq!(discovered.web_bucket, None);
    assert!(discovered.diagnostics.is_empty());
}

// 2. Explicit valid override takes precedence
#[test]
fn test_02_explicit_valid_override_takes_precedence() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "aw-watcher-window_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-window_MYHOST",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );
    buckets.insert(
        "custom-window-bucket".to_string(),
        make_bucket(
            "custom-window-bucket",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );

    let overrides = BucketOverridesConfig {
        window_bucket: Some("custom-window-bucket".to_string()),
        afk_bucket: None,
        web_bucket: None,
    };

    let discovered = discover_buckets(&buckets, "MYHOST", Some(&overrides));
    assert_eq!(
        discovered.window_bucket.as_deref(),
        Some("custom-window-bucket")
    );
}

// 3. Explicit override referencing missing bucket fails cleanly
#[test]
fn test_03_override_referencing_missing_bucket_fails_cleanly() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "aw-watcher-window_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-window_MYHOST",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );

    let overrides = BucketOverridesConfig {
        window_bucket: Some("non-existent-bucket".to_string()),
        afk_bucket: None,
        web_bucket: None,
    };

    let discovered = discover_buckets(&buckets, "MYHOST", Some(&overrides));
    // Must return None instead of falling back or crashing
    assert_eq!(discovered.window_bucket, None);
    assert!(discovered
        .diagnostics
        .iter()
        .any(|d| d.contains("was not found in ActivityWatch bucket list")));
}

// 4. Explicit override referencing wrong bucket category fails cleanly
#[test]
fn test_04_override_referencing_wrong_category_fails_cleanly() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "aw-watcher-afk_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-afk_MYHOST",
            "afkstatus",
            "aw-watcher-afk",
            "MYHOST",
        ),
    );

    // User mistakenly overrides window_bucket with an AFK bucket ID
    let overrides = BucketOverridesConfig {
        window_bucket: Some("aw-watcher-afk_MYHOST".to_string()),
        afk_bucket: None,
        web_bucket: None,
    };

    let discovered = discover_buckets(&buckets, "MYHOST", Some(&overrides));
    assert_eq!(discovered.window_bucket, None);
    assert!(discovered
        .diagnostics
        .iter()
        .any(|d| d.contains("does not match expected watcher category")));
}

// 5. Multiple candidates on host with exactly one canonical bucket selects the canonical bucket
#[test]
fn test_05_multiple_candidates_with_one_canonical_bucket_selects_canonical() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "aw-watcher-window_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-window_MYHOST",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );
    buckets.insert(
        "aw-watcher-window-secondary_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-window-secondary_MYHOST",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );

    let discovered = discover_buckets(&buckets, "MYHOST", None);
    // Unambiguously selects the canonical bucket
    assert_eq!(
        discovered.window_bucket.as_deref(),
        Some("aw-watcher-window_MYHOST")
    );
    // Diagnostic alerts user that multiple candidates were present and canonical was chosen
    assert!(discovered
        .diagnostics
        .iter()
        .any(|d| d.contains("Resolved by selecting unique canonical bucket")));
}

// 6. Multiple canonical candidates on host fails cleanly (diagnostic recorded, None returned)
#[test]
fn test_06_multiple_canonical_candidates_fails_cleanly() {
    let mut buckets = HashMap::new();
    // Simulate multiple canonical matches (e.g. casing anomaly or duplicates)
    buckets.insert(
        "aw-watcher-window_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-window_MYHOST",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );
    buckets.insert(
        "aw-watcher-window_myhost".to_string(),
        make_bucket(
            "aw-watcher-window_myhost",
            "currentwindow",
            "aw-watcher-window",
            "myhost",
        ),
    );

    let discovered = discover_buckets(&buckets, "MYHOST", None);
    // Must NOT guess or pick candidates[0]
    assert_eq!(discovered.window_bucket, None);
    assert!(discovered
        .diagnostics
        .iter()
        .any(|d| d.contains("Ambiguity cannot be resolved automatically")));
}

// 7. Multiple non-canonical candidates on host fails cleanly (diagnostic recorded, None returned)
#[test]
fn test_07_multiple_non_canonical_candidates_fails_cleanly() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "custom-window-1".to_string(),
        make_bucket(
            "custom-window-1",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );
    buckets.insert(
        "custom-window-2".to_string(),
        make_bucket(
            "custom-window-2",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );

    let discovered = discover_buckets(&buckets, "MYHOST", None);
    // Must NOT guess or pick candidates[0]
    assert_eq!(discovered.window_bucket, None);
    assert!(discovered
        .diagnostics
        .iter()
        .any(|d| d.contains("Ambiguity cannot be resolved automatically")
            && d.contains("custom-window-1")
            && d.contains("custom-window-2")));
}

// 8. Deterministic behavior: running selection repeatedly produces identical results
#[test]
fn test_08_deterministic_behavior_across_repeated_runs() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "custom-window-beta".to_string(),
        make_bucket(
            "custom-window-beta",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );
    buckets.insert(
        "custom-window-alpha".to_string(),
        make_bucket(
            "custom-window-alpha",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );

    let first = discover_buckets(&buckets, "MYHOST", None);
    for _ in 0..100 {
        let current = discover_buckets(&buckets, "MYHOST", None);
        assert_eq!(first.window_bucket, current.window_bucket);
        assert_eq!(first.diagnostics, current.diagnostics);
    }
}

// 9. No matching candidate
#[test]
fn test_09_no_matching_candidate() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "unrelated-bucket".to_string(),
        make_bucket("unrelated-bucket", "other_type", "other_client", "MYHOST"),
    );

    let discovered = discover_buckets(&buckets, "MYHOST", None);
    assert_eq!(discovered.window_bucket, None);
    assert_eq!(discovered.afk_bucket, None);
    assert_eq!(discovered.web_bucket, None);
    assert!(discovered
        .diagnostics
        .iter()
        .any(|d| d.contains("Essential window watcher bucket") && d.contains("not found")));
}

// 10. Multiple hosts: matches local server hostname and ignores other hosts
#[test]
fn test_10_multiple_hosts_filtered_by_server_hostname() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "aw-watcher-window_OTHERHOST".to_string(),
        make_bucket(
            "aw-watcher-window_OTHERHOST",
            "currentwindow",
            "aw-watcher-window",
            "OTHERHOST",
        ),
    );
    buckets.insert(
        "aw-watcher-window_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-window_MYHOST",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );

    let discovered = discover_buckets(&buckets, "MYHOST", None);
    // Correctly binds to MYHOST candidate, ignoring OTHERHOST
    assert_eq!(
        discovered.window_bucket.as_deref(),
        Some("aw-watcher-window_MYHOST")
    );
}

// 11. Essential window bucket ambiguity
#[test]
fn test_11_essential_window_ambiguity() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "custom-win-1".to_string(),
        make_bucket(
            "custom-win-1",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );
    buckets.insert(
        "custom-win-2".to_string(),
        make_bucket(
            "custom-win-2",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );
    buckets.insert(
        "aw-watcher-afk_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-afk_MYHOST",
            "afkstatus",
            "aw-watcher-afk",
            "MYHOST",
        ),
    );

    let discovered = discover_buckets(&buckets, "MYHOST", None);
    assert_eq!(discovered.window_bucket, None);
    assert!(!discovered.has_essential_window_bucket());
    assert!(discovered
        .diagnostics
        .iter()
        .any(|d| d.contains("Ambiguity detected")));
}

// 12. Essential AFK bucket ambiguity
#[test]
fn test_12_essential_afk_ambiguity() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "aw-watcher-window_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-window_MYHOST",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );
    buckets.insert(
        "custom-afk-1".to_string(),
        make_bucket("custom-afk-1", "afkstatus", "aw-watcher-afk", "MYHOST"),
    );
    buckets.insert(
        "custom-afk-2".to_string(),
        make_bucket("custom-afk-2", "afkstatus", "aw-watcher-afk", "MYHOST"),
    );

    let discovered = discover_buckets(&buckets, "MYHOST", None);
    assert_eq!(discovered.afk_bucket, None);
    assert!(!discovered.capabilities.idle_tracking);
    assert!(discovered
        .diagnostics
        .iter()
        .any(|d| d.contains("Ambiguity detected")));
}

// 13. Optional browser bucket ambiguity retains desktop tracking
#[test]
fn test_13_optional_browser_ambiguity() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "aw-watcher-window_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-window_MYHOST",
            "currentwindow",
            "aw-watcher-window",
            "MYHOST",
        ),
    );
    buckets.insert(
        "aw-watcher-afk_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-afk_MYHOST",
            "afkstatus",
            "aw-watcher-afk",
            "MYHOST",
        ),
    );
    buckets.insert(
        "custom-web-1".to_string(),
        make_bucket("custom-web-1", "web.tab.current", "custom-web", "MYHOST"),
    );
    buckets.insert(
        "custom-web-2".to_string(),
        make_bucket("custom-web-2", "web.tab.current", "custom-web", "MYHOST"),
    );

    let discovered = discover_buckets(&buckets, "MYHOST", None);
    // Desktop tracking remains fully discovered
    assert_eq!(
        discovered.window_bucket.as_deref(),
        Some("aw-watcher-window_MYHOST")
    );
    assert_eq!(
        discovered.afk_bucket.as_deref(),
        Some("aw-watcher-afk_MYHOST")
    );
    // Browser is marked unresolved
    assert_eq!(discovered.web_bucket, None);
    assert!(!discovered.capabilities.browser_url_tracking);
    assert!(discovered
        .diagnostics
        .iter()
        .any(|d| d.contains("Ambiguity detected")));
}

// 14. Official aw-watcher-web supports incognito detection
#[test]
fn test_14_official_aw_watcher_web_supports_incognito() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "aw-watcher-web-chrome_MYHOST".to_string(),
        make_bucket(
            "aw-watcher-web-chrome_MYHOST",
            "web.tab.current",
            "aw-watcher-web-chrome",
            "MYHOST",
        ),
    );

    let discovered = discover_buckets(&buckets, "MYHOST", None);
    assert_eq!(
        discovered.web_bucket.as_deref(),
        Some("aw-watcher-web-chrome_MYHOST")
    );
    assert!(discovered.capabilities.browser_url_tracking);
    assert!(discovered.capabilities.incognito_detection);
}

// 15. Custom web client has unknown incognito detection support
#[test]
fn test_15_custom_web_client_unknown_incognito_support() {
    let mut buckets = HashMap::new();
    buckets.insert(
        "custom-web_MYHOST".to_string(),
        make_bucket(
            "custom-web_MYHOST",
            "web.tab.current",
            "custom-browser-tracker",
            "MYHOST",
        ),
    );

    let discovered = discover_buckets(&buckets, "MYHOST", None);
    assert_eq!(discovered.web_bucket.as_deref(), Some("custom-web_MYHOST"));
    assert!(discovered.capabilities.browser_url_tracking);
    // Incognito detection is false because support cannot be verified from custom client
    assert!(!discovered.capabilities.incognito_detection);
}
