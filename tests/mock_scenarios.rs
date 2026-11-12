use chrono::Utc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use flowguard::application::ports::{ActivitySource, SourceError};
use flowguard::config::ActivityWatchConfig;
use flowguard::domain::{FreshnessState, IdleState, SourceHealth};
use flowguard::infrastructure::activitywatch::ActivityWatchAdapter;

fn make_config(base_url: &str) -> ActivityWatchConfig {
    ActivityWatchConfig {
        base_url: base_url.to_string(),
        poll_interval_ms: 100,
        request_timeout_ms: 500,
        reconnect_interval_secs: 1,
        stale_after_secs: 15,
        afk_timeout_secs: 180,
        api_key_env: None,
        bucket_overrides: None,
    }
}

// 1. Valid ActivityWatch server information.
#[tokio::test]
async fn test_01_valid_activitywatch_server_info() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1",
            "testing": false,
            "profile": "default"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        let caps = adapter.connect().expect("connect should succeed");

        assert_eq!(caps.provider, "activitywatch");
        assert_eq!(caps.server_version.as_deref(), Some("v0.14.1"));
        assert_eq!(caps.hostname.as_deref(), Some("TESTHOST"));
        assert!(!caps.is_testing);
    })
    .await
    .unwrap();
}

// 2. An unrelated HTTP service at the configured endpoint (WrongService).
#[tokio::test]
async fn test_02_unrelated_http_service_wrong_service() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "message": "Welcome to unrelated service"
        })))
        .mount(&server)
        .await;

    let uri = server.uri();
    let port = server.address().port();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        let err = adapter
            .connect()
            .expect_err("should reject unrelated service");

        match err {
            SourceError::WrongService { endpoint, reason } => {
                assert!(endpoint.contains(&port.to_string()));
                assert!(
                    reason.contains("does not match ActivityWatch server signature")
                        || reason.contains("missing expected")
                );
            }
            other => panic!("Expected WrongService error, got: {:?}", other),
        }

        assert_eq!(adapter.health(), SourceHealth::WrongService);
    })
    .await
    .unwrap();
}

// 3. Connection refused.
#[test]
fn test_03_connection_refused() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            drop(stream);
        }
    });

    let mut adapter = ActivityWatchAdapter::new(make_config(&format!("http://127.0.0.1:{}", port)));
    let err = adapter
        .connect()
        .expect_err("should fail when connection refused");

    match err {
        SourceError::ConnectionRefused(url) => assert!(url.contains(&port.to_string())),
        other => panic!("Expected ConnectionRefused error, got: {:?}", other),
    }

    assert_eq!(adapter.health(), SourceHealth::Disconnected);
}

// 4. Timeout.
#[tokio::test]
async fn test_04_request_timeout() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "hostname": "TESTHOST",
                    "version": "v0.14.1"
                }))
                .set_delay(Duration::from_millis(400)),
        )
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut config = make_config(&uri);
        config.request_timeout_ms = 100; // Shorter than mock delay

        let mut adapter = ActivityWatchAdapter::new(config);
        let err = adapter.connect().expect_err("should time out");

        match err {
            SourceError::Timeout(ms) => assert_eq!(ms, 100),
            other => panic!("Expected Timeout error, got: {:?}", other),
        }
    })
    .await
    .unwrap();
}

// 5. Missing window bucket.
#[tokio::test]
async fn test_05_missing_window_bucket() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-afk_TESTHOST": {
                "id": "aw-watcher-afk_TESTHOST",
                "type": "afkstatus",
                "client": "aw-watcher-afk",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-afk_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "status": "not-afk" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect should succeed");

        let obs = adapter.poll().expect("poll should succeed");
        assert_eq!(obs.window_freshness, FreshnessState::Missing);
        assert_eq!(obs.application, None);
        assert!(!obs.capabilities.application_tracking);
        assert_eq!(obs.source_health, SourceHealth::Degraded);
    })
    .await
    .unwrap();
}

// 6. Missing AFK bucket.
#[tokio::test]
async fn test_06_missing_afk_bucket() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "app": "notepad.exe", "title": "Untitled" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect should succeed");

        let obs = adapter.poll().expect("poll should succeed");
        assert_eq!(obs.idle_freshness, FreshnessState::Missing);
        assert_eq!(obs.idle_state, IdleState::Unknown);
        assert!(!obs.capabilities.idle_tracking);
        assert_eq!(obs.source_health, SourceHealth::Degraded);
    })
    .await
    .unwrap();
}

// 7. Missing browser bucket.
#[tokio::test]
async fn test_07_missing_browser_bucket() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            },
            "aw-watcher-afk_TESTHOST": {
                "id": "aw-watcher-afk_TESTHOST",
                "type": "afkstatus",
                "client": "aw-watcher-afk",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "app": "chrome.exe", "title": "Google - Chrome" }
            }
        ])))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-afk_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "status": "not-afk" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect should succeed");

        let obs = adapter.poll().expect("poll should succeed");
        assert_eq!(obs.application.as_deref(), Some("chrome.exe"));
        assert_eq!(obs.browser, None);
        assert!(!obs.capabilities.browser_url_tracking);
        assert_eq!(obs.source_health, SourceHealth::Connected);
    })
    .await
    .unwrap();
}

// 8. Multiple matching buckets.
#[tokio::test]
async fn test_08_multiple_matching_buckets_ambiguity() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            },
            "aw-watcher-window-secondary_TESTHOST": {
                "id": "aw-watcher-window-secondary_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect should succeed");

        let buckets = adapter.discovered_buckets().expect("buckets discovered");
        assert!(buckets.window_bucket.is_some());
        assert!(buckets
            .diagnostics
            .iter()
            .any(|d| d.contains("Ambiguity detected")));
    })
    .await
    .unwrap();
}

// 9. Malformed event JSON.
#[tokio::test]
async fn test_09_malformed_event_json() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_string("THIS IS NOT JSON [{{"))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect should succeed");

        let obs = adapter
            .poll()
            .expect("poll should handle error without crashing");
        assert_eq!(obs.window_freshness, FreshnessState::Stale);
        assert_eq!(obs.source_health, SourceHealth::Degraded);
        assert!(obs
            .diagnostics
            .iter()
            .any(|d| d.contains("Failed to query window watcher")));
    })
    .await
    .unwrap();
}

// 10. Missing event fields.
#[tokio::test]
async fn test_10_missing_event_fields() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": {}
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect should succeed");

        let obs = adapter.poll().expect("poll should handle missing fields");
        assert_eq!(obs.application, None);
        assert_eq!(obs.window_title, None);
    })
    .await
    .unwrap();
}

// 11. Stale window event.
#[tokio::test]
async fn test_11_stale_window_event() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    let old_time = Utc::now() - chrono::Duration::hours(1);
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": old_time.to_rfc3339(),
                "duration": 0.0,
                "data": { "app": "code.exe", "title": "Stale Title" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect should succeed");

        let obs = adapter.poll().expect("poll should succeed");
        assert_eq!(obs.window_freshness, FreshnessState::Stale);
        assert_eq!(obs.application, None);
        assert_eq!(obs.window_title, None);
        assert_eq!(obs.source_health, SourceHealth::Degraded);
    })
    .await
    .unwrap();
}

// 12. Stale browser event.
#[tokio::test]
async fn test_12_stale_browser_event() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            },
            "aw-watcher-web-chrome_TESTHOST": {
                "id": "aw-watcher-web-chrome_TESTHOST",
                "type": "web.tab.current",
                "client": "aw-watcher-web-chrome",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "app": "chrome.exe", "title": "Chrome" }
            }
        ])))
        .mount(&server)
        .await;

    let old_time = Utc::now() - chrono::Duration::hours(1);
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-web-chrome_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": old_time.to_rfc3339(),
                "duration": 0.0,
                "data": { "url": "https://example.com", "title": "Old Tab" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect should succeed");

        let obs = adapter.poll().expect("poll should succeed");
        assert_eq!(obs.browser_freshness, FreshnessState::Stale);
        assert_eq!(obs.browser, None);
        assert!(obs.capabilities.browser_url_tracking);
    })
    .await
    .unwrap();
}

// 13. Valid application event.
#[tokio::test]
async fn test_13_valid_application_event() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 1.5,
                "data": { "app": "code.exe", "title": "src/main.rs" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect should succeed");

        let obs = adapter.poll().expect("poll should succeed");
        assert_eq!(obs.application.as_deref(), Some("code.exe"));
        assert_eq!(obs.window_title.as_deref(), Some("src/main.rs"));
        assert!(obs.capabilities.application_tracking);
        assert!(obs.capabilities.window_title_tracking);
    })
    .await
    .unwrap();
}

// 14. Valid AFK event.
#[tokio::test]
async fn test_14_valid_afk_event() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-afk_TESTHOST": {
                "id": "aw-watcher-afk_TESTHOST",
                "type": "afkstatus",
                "client": "aw-watcher-afk",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-afk_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 5.0,
                "data": { "status": "afk" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect should succeed");

        let obs = adapter.poll().expect("poll should succeed");
        assert_eq!(obs.idle_state, IdleState::Afk);
        assert!(obs.capabilities.idle_tracking);
    })
    .await
    .unwrap();
}

// 15. Valid browser event.
#[tokio::test]
async fn test_15_valid_browser_event() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            },
            "aw-watcher-web-chrome_TESTHOST": {
                "id": "aw-watcher-web-chrome_TESTHOST",
                "type": "web.tab.current",
                "client": "aw-watcher-web-chrome",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "app": "chrome.exe", "title": "Rust Programming - YouTube" }
            }
        ])))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-web-chrome_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": {
                    "url": "https://www.youtube.com/watch?v=TEST_ID",
                    "title": "Rust Programming Tutorial",
                    "audible": true,
                    "incognito": false
                }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect should succeed");

        let obs = adapter.poll().expect("poll should succeed");
        let browser = obs.browser.expect("browser context should exist");
        assert_eq!(
            browser.url.as_deref(),
            Some("https://www.youtube.com/watch?v=TEST_ID")
        );
        assert_eq!(browser.title.as_deref(), Some("Rust Programming Tutorial"));
        assert_eq!(browser.audible, Some(true));
        assert_eq!(browser.incognito, Some(false));
        assert!(obs.capabilities.browser_url_tracking);
        assert!(obs.capabilities.incognito_detection);
    })
    .await
    .unwrap();
}

// 16. Authorization error.
#[tokio::test]
async fn test_16_authorization_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .mount(&server)
        .await;

    let uri = server.uri();
    let port = server.address().port();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        let err = adapter.connect().expect_err("should return auth error");

        match err {
            SourceError::AuthRequired(url) => assert!(url.contains(&port.to_string())),
            other => panic!("Expected AuthRequired error, got: {:?}", other),
        }
    })
    .await
    .unwrap();
}

// 17. Custom configured port.
#[tokio::test]
async fn test_17_custom_configured_port() {
    let server = MockServer::start().await;
    let custom_port = server.address().port();

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;

    tokio::task::spawn_blocking(move || {
        let mut adapter =
            ActivityWatchAdapter::new(make_config(&format!("http://127.0.0.1:{}", custom_port)));
        let caps = adapter.connect().expect("should connect to custom port");
        assert_eq!(caps.server_version.as_deref(), Some("v0.14.1"));
    })
    .await
    .unwrap();
}

// 18. Reconnection after temporary disconnection.
#[tokio::test]
async fn test_18_reconnection_after_temporary_disconnection() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "app": "code.exe", "title": "Editor" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("initial connect succeeds");

        let obs1 = adapter.poll().expect("poll 1 succeeds");
        assert_eq!(obs1.application.as_deref(), Some("code.exe"));
    })
    .await
    .unwrap();

    // Reset mock server to return 500 on events
    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        let _ = adapter.connect();
        let obs2 = adapter.poll().expect("poll 2 handles failure gracefully");
        assert_eq!(obs2.window_freshness, FreshnessState::Missing);
    })
    .await
    .unwrap();

    // Recover server
    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "app": "code.exe", "title": "Editor Recovered" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("reconnect succeeds");
        let obs3 = adapter.poll().expect("poll 3 succeeds after recovery");
        assert_eq!(obs3.application.as_deref(), Some("code.exe"));
        assert_eq!(obs3.window_title.as_deref(), Some("Editor Recovered"));
    })
    .await
    .unwrap();
}

// 19. HTTP 500 internal server error handling.
#[tokio::test]
async fn test_19_http_500_api_error_handling() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect succeeds");

        let obs = adapter.poll().expect("poll handles 500 without crashing");
        assert_eq!(obs.window_freshness, FreshnessState::Stale);
        assert_eq!(obs.source_health, SourceHealth::Degraded);
        assert!(obs
            .diagnostics
            .iter()
            .any(|d| d.contains("Failed to query window watcher")));
    })
    .await
    .unwrap();
}

// 20. HTTP 404 missing bucket invalidates reference and triggers rediscovery.
#[tokio::test]
async fn test_20_http_404_missing_bucket_triggers_rediscovery() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    // Initial bucket list returns old-bucket
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    // Old bucket returns 404
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Bucket not found"))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect succeeds");

        let obs = adapter.poll().expect("poll handles 404 without crashing");
        assert_eq!(obs.window_freshness, FreshnessState::Missing);
        assert!(obs
            .diagnostics
            .iter()
            .any(|d| d.contains("was not found in ActivityWatch")));
    })
    .await
    .unwrap();
}

// 21. Partial failure: window watcher fails while AFK watcher succeeds.
#[tokio::test]
async fn test_21_partial_failure_window_fails_afk_succeeds() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            },
            "aw-watcher-afk_TESTHOST": {
                "id": "aw-watcher-afk_TESTHOST",
                "type": "afkstatus",
                "client": "aw-watcher-afk",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    // Window returns 500
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    // AFK returns valid active event
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-afk_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "status": "not-afk" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect succeeds");

        let obs = adapter.poll().expect("poll handles partial failure");
        assert_eq!(obs.window_freshness, FreshnessState::Stale);
        assert_eq!(obs.application, None);
        // AFK observation is preserved!
        assert_eq!(obs.idle_freshness, FreshnessState::Fresh);
        assert_eq!(obs.idle_state, IdleState::Active);
        // Essential desktop tracking is degraded because window failed
        assert_eq!(obs.source_health, SourceHealth::Degraded);
    })
    .await
    .unwrap();
}

// 22. Partial failure: browser watcher fails while desktop tracking is healthy.
#[tokio::test]
async fn test_22_partial_failure_browser_fails_desktop_healthy() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            },
            "aw-watcher-afk_TESTHOST": {
                "id": "aw-watcher-afk_TESTHOST",
                "type": "afkstatus",
                "client": "aw-watcher-afk",
                "hostname": "TESTHOST"
            },
            "aw-watcher-web-chrome_TESTHOST": {
                "id": "aw-watcher-web-chrome_TESTHOST",
                "type": "web.tab.current",
                "client": "aw-watcher-web-chrome",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    // Window returns valid event
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "app": "code.exe", "title": "Editor" }
            }
        ])))
        .mount(&server)
        .await;

    // AFK returns valid event
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-afk_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "status": "not-afk" }
            }
        ])))
        .mount(&server)
        .await;

    // Web returns 500 error
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-web-chrome_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect succeeds");

        let obs = adapter
            .poll()
            .expect("poll handles partial browser failure");
        assert_eq!(obs.window_freshness, FreshnessState::Fresh);
        assert_eq!(obs.idle_freshness, FreshnessState::Fresh);
        assert_eq!(obs.browser_freshness, FreshnessState::Stale);
        assert_eq!(obs.browser, None);
        // Desktop health remains Connected (Healthy)!
        assert_eq!(obs.source_health, SourceHealth::Connected);
        assert!(obs
            .diagnostics
            .iter()
            .any(|d| d.contains("Failed to query web watcher")));
    })
    .await
    .unwrap();
}

// 23. Empty event response is distinguished from transport error.
#[tokio::test]
async fn test_23_empty_event_response_distinguished_from_timeout() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    // Empty event list returns []
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect succeeds");

        let obs = adapter.poll().expect("poll handles empty bucket");
        assert_eq!(obs.window_freshness, FreshnessState::Missing);
        assert!(obs
            .diagnostics
            .iter()
            .any(|d| d.contains("has no recorded events yet")));
        assert!(!obs.diagnostics.iter().any(|d| d.contains("timed out")));
    })
    .await
    .unwrap();
}

// 24. No stale window data presented as current active observation.
#[tokio::test]
async fn test_24_no_stale_window_data_presented_as_current() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTHOST",
            "version": "v0.14.1"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTHOST": {
                "id": "aw-watcher-window_TESTHOST",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTHOST"
            }
        })))
        .mount(&server)
        .await;

    // Stale event from 2 hours ago
    let old_timestamp = Utc::now() - chrono::Duration::hours(2);
    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-window_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": old_timestamp.to_rfc3339(),
                "duration": 0.0,
                "data": { "app": "code.exe", "title": "Old Code File" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        adapter.connect().expect("connect succeeds");

        let obs = adapter.poll().expect("poll handles stale event");
        assert_eq!(obs.window_freshness, FreshnessState::Stale);
        // Stale app and title must NOT be presented as current
        assert_eq!(obs.application, None);
        assert_eq!(obs.window_title, None);
        assert_eq!(obs.source_health, SourceHealth::Degraded);
    })
    .await
    .unwrap();
}
