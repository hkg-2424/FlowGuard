use flowguard::application::ports::{ActivitySource, SourceError};
use flowguard::application::ActivityService;
use flowguard::config::{ActivityWatchConfig, FlowGuardConfig};
use flowguard::domain::{
    ActivityCapabilities, ActivityObservation, DiagnosticReport, FreshnessState, IdleState,
    SourceCapabilities, SourceHealth, SubsystemDiagnostic, SubsystemStatus,
};
use flowguard::infrastructure::activitywatch::ActivityWatchAdapter;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_config(base_url: &str) -> ActivityWatchConfig {
    ActivityWatchConfig {
        base_url: base_url.to_string(),
        poll_interval_ms: 100,
        request_timeout_ms: 500,
        reconnect_interval_secs: 1,
        stale_after_secs: 15,
        afk_timeout_secs: 180,
        api_key_env: None,
        allow_remote: false,
        bucket_overrides: None,
    }
}

// 1. Valid generic diagnostic report produced by ActivityWatchAdapter
#[tokio::test]
async fn test_01_valid_activitywatch_diagnostic_report() {
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
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "app": "code.exe", "title": "Editor" }
            }
        ])))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/aw-watcher-afk_TESTHOST/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "duration": 0.0,
                "data": { "status": "not-afk" }
            }
        ])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        let report = adapter.diagnose().expect("diagnose succeeds");

        assert_eq!(report.provider_name, "ActivityWatch");
        assert!(report.is_reachable);
        assert_eq!(report.service_identity.as_deref(), Some("ActivityWatch"));
        assert_eq!(report.server_version.as_deref(), Some("v0.14.1"));
        assert_eq!(report.hostname.as_deref(), Some("TESTHOST"));
        assert_eq!(report.health, SourceHealth::Connected);
        assert_eq!(report.subsystems.len(), 3);

        let window_sub = report
            .subsystems
            .iter()
            .find(|s| s.name == "Desktop Window")
            .unwrap();
        assert_eq!(window_sub.status, SubsystemStatus::Healthy);

        let afk_sub = report
            .subsystems
            .iter()
            .find(|s| s.name == "User Idle / AFK")
            .unwrap();
        assert_eq!(afk_sub.status, SubsystemStatus::Healthy);

        let web_sub = report
            .subsystems
            .iter()
            .find(|s| s.name == "Browser Extension")
            .unwrap();
        assert_eq!(web_sub.status, SubsystemStatus::Unavailable);
    })
    .await
    .unwrap();
}

// 2. Unavailable ActivityWatch server produces useful diagnostics without panicking
#[tokio::test]
async fn test_02_unavailable_server_produces_useful_diagnostics() {
    tokio::task::spawn_blocking(move || {
        // Port 59999 is closed
        let mut adapter = ActivityWatchAdapter::new(make_config("http://127.0.0.1:59999"));
        let report = adapter
            .diagnose()
            .expect("diagnose handles connection error gracefully");

        assert_eq!(report.provider_name, "ActivityWatch");
        assert!(!report.is_reachable);
        assert_eq!(report.health, SourceHealth::Disconnected);
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.contains("Failed to connect")));
    })
    .await
    .unwrap();
}

// 3. Unrelated HTTP service produces wrong-service diagnostic
#[tokio::test]
async fn test_03_unrelated_service_produces_wrong_service_diagnostic() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "message": "Nginx Web Server"
        })))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        let report = adapter
            .diagnose()
            .expect("diagnose handles wrong service gracefully");

        assert_eq!(report.provider_name, "ActivityWatch");
        assert!(!report.is_reachable);
        assert_eq!(report.health, SourceHealth::WrongService);
        assert!(report.diagnostics.iter().any(|d| d.contains("signature")));
    })
    .await
    .unwrap();
}

// 4. Missing essential window watcher reported in subsystems as Unavailable
#[tokio::test]
async fn test_04_missing_essential_window_watcher_reported() {
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
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        let mut adapter = ActivityWatchAdapter::new(make_config(&uri));
        let report = adapter.diagnose().expect("diagnose succeeds");

        assert_eq!(report.health, SourceHealth::Degraded);
        let window_sub = report
            .subsystems
            .iter()
            .find(|s| s.name == "Desktop Window")
            .unwrap();
        assert_eq!(window_sub.status, SubsystemStatus::Unavailable);
        assert!(window_sub
            .details
            .contains("aw-watcher-window may not be running"));
    })
    .await
    .unwrap();
}

// 5. Mocked non-ActivityWatch ActivitySource provides diagnostics through ActivityService
struct MockNativeWindowsAdapter {
    health: SourceHealth,
}

impl ActivitySource for MockNativeWindowsAdapter {
    fn connect(&mut self) -> Result<SourceCapabilities, SourceError> {
        Ok(SourceCapabilities {
            provider: "native_windows".to_string(),
            server_version: Some("Win32 10.0.22631".to_string()),
            hostname: Some("NATIVE_PC".to_string()),
            is_testing: false,
            capabilities: ActivityCapabilities {
                application_tracking: true,
                window_title_tracking: true,
                idle_tracking: true,
                browser_url_tracking: true,
                incognito_detection: false,
            },
        })
    }

    fn poll(&mut self) -> Result<ActivityObservation, SourceError> {
        Ok(ActivityObservation {
            observed_at: chrono::Utc::now(),
            application: Some("notepad.exe".to_string()),
            window_title: Some("Untitled - Notepad".to_string()),
            browser: None,
            idle_state: IdleState::Active,
            capabilities: ActivityCapabilities::default(),
            source_health: self.health,
            window_freshness: FreshnessState::Fresh,
            idle_freshness: FreshnessState::Fresh,
            browser_freshness: FreshnessState::Missing,
            diagnostics: Vec::new(),
        })
    }

    fn health(&self) -> SourceHealth {
        self.health
    }

    fn diagnose(&mut self) -> Result<DiagnosticReport, SourceError> {
        Ok(DiagnosticReport {
            provider_name: "NativeWindows".to_string(),
            endpoint: "Win32 Subsystem / WinEventHook".to_string(),
            is_reachable: true,
            service_identity: Some("NativeWindows".to_string()),
            server_version: Some("Windows 11 Build 22631".to_string()),
            hostname: Some("NATIVE_PC".to_string()),
            is_testing: false,
            server_profile: None,
            subsystems: vec![
                SubsystemDiagnostic {
                    name: "Active Window Hook".to_string(),
                    target: Some("SetWinEventHook".to_string()),
                    status: SubsystemStatus::Healthy,
                    freshness: Some(FreshnessState::Fresh),
                    age_seconds: Some(0),
                    details: "WinEventHook active and receiving foreground events".to_string(),
                },
                SubsystemDiagnostic {
                    name: "User Idle Detection".to_string(),
                    target: Some("GetLastInputInfo".to_string()),
                    status: SubsystemStatus::Healthy,
                    freshness: Some(FreshnessState::Fresh),
                    age_seconds: Some(0),
                    details: "Tick counter responsive".to_string(),
                },
            ],
            capabilities: ActivityCapabilities {
                application_tracking: true,
                window_title_tracking: true,
                idle_tracking: true,
                browser_url_tracking: false,
                incognito_detection: false,
            },
            health: SourceHealth::Connected,
            diagnostics: Vec::new(),
        })
    }
}

#[test]
fn test_05_mocked_non_activitywatch_source_runs_through_activity_service() {
    let mock_source = MockNativeWindowsAdapter {
        health: SourceHealth::Connected,
    };
    let config = FlowGuardConfig::default();
    let mut service = ActivityService::new(Box::new(mock_source), config);

    // ActivityService::run_doctor() works transparently with the non-ActivityWatch provider!
    let report = service.run_doctor();

    assert_eq!(report.provider_name, "NativeWindows");
    assert_eq!(report.endpoint, "Win32 Subsystem / WinEventHook");
    assert!(report.is_reachable);
    assert_eq!(report.health, SourceHealth::Connected);
    assert_eq!(report.subsystems.len(), 2);
    assert_eq!(report.subsystems[0].name, "Active Window Hook");
    assert_eq!(report.subsystems[0].status, SubsystemStatus::Healthy);
}
