use flowguard::application::ports::ActivitySource;
use flowguard::application::ActivityService;
use flowguard::config::{ActivityWatchConfig, FlowGuardConfig};
use flowguard::domain::{FreshnessState, IdleState, SourceHealth, SubsystemStatus};
use flowguard::infrastructure::auto_source::{ActiveBackend, AutoSourceAdapter};
use flowguard::infrastructure::{create_activity_source, NativeWindowsAdapter};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn make_aw_config(base_url: &str) -> ActivityWatchConfig {
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

// 1. NativeWindowsAdapter connect discovers native Win32 capabilities
#[test]
fn test_01_native_windows_adapter_connect() {
    let mut adapter = NativeWindowsAdapter::new(180);
    let caps = adapter.connect().expect("Native adapter connects cleanly");

    assert_eq!(caps.provider, "native_windows");
    assert!(caps.capabilities.application_tracking);
    assert!(caps.capabilities.window_title_tracking);
    assert!(caps.capabilities.idle_tracking);
    assert!(!caps.capabilities.browser_url_tracking);
    assert!(!caps.capabilities.incognito_detection);
    assert_eq!(adapter.health(), SourceHealth::Connected);
}

// 2. NativeWindowsAdapter poll returns live fresh observation
#[test]
fn test_02_native_windows_adapter_poll() {
    let mut adapter = NativeWindowsAdapter::default();
    let obs = adapter.poll().expect("Native adapter polls successfully");

    assert_eq!(obs.source_health, SourceHealth::Connected);
    assert_eq!(obs.window_freshness, FreshnessState::Fresh);
    assert_eq!(obs.idle_freshness, FreshnessState::Fresh);
    assert_eq!(obs.browser_freshness, FreshnessState::Missing);
    assert!(obs.idle_state == IdleState::Active || obs.idle_state == IdleState::Afk);
    assert!(obs.capabilities.application_tracking);
    assert!(obs.capabilities.window_title_tracking);
}

// 3. NativeWindowsAdapter diagnose produces healthy doctor report
#[test]
fn test_03_native_windows_adapter_diagnose() {
    let mut adapter = NativeWindowsAdapter::new(180);
    let report = adapter
        .diagnose()
        .expect("Native adapter diagnoses cleanly");

    assert_eq!(report.provider_name, "NativeWindows");
    assert!(report.is_reachable);
    assert_eq!(report.health, SourceHealth::Connected);
    assert_eq!(report.subsystems.len(), 2);

    let win_sub = report
        .subsystems
        .iter()
        .find(|s| s.name == "Active Window Poller")
        .expect("Active Window Poller subsystem present");
    assert_eq!(win_sub.status, SubsystemStatus::Healthy);

    let idle_sub = report
        .subsystems
        .iter()
        .find(|s| s.name == "User Idle Detection")
        .expect("User Idle Detection subsystem present");
    assert_eq!(idle_sub.status, SubsystemStatus::Healthy);
}

// 4. AutoSourceAdapter falls back to Native Windows when ActivityWatch is unreachable
#[test]
fn test_04_auto_source_adapter_fallback_on_unreachable_activitywatch() {
    // Port 59988 is closed / unreachable
    let aw_config = make_aw_config("http://127.0.0.1:59988");
    let mut auto_source = AutoSourceAdapter::new(aw_config);

    // Initial backend is ActivityWatch
    assert_eq!(auto_source.active_backend(), ActiveBackend::ActivityWatch);
    assert!(!auto_source.is_fallback_active());

    // Connect automatically falls back to Native Windows
    let caps = auto_source
        .connect()
        .expect("AutoSource seamlessly connects via fallback");
    assert!(caps.provider.contains("fallback") || caps.provider.contains("native_windows"));
    assert_eq!(auto_source.active_backend(), ActiveBackend::NativeWindows);
    assert!(auto_source.is_fallback_active());
    assert_eq!(auto_source.health(), SourceHealth::Connected);

    // Polling produces valid native observations seamlessly
    let obs = auto_source.poll().expect("Polls cleanly on fallback");
    assert_eq!(obs.source_health, SourceHealth::Connected);
    assert_eq!(obs.window_freshness, FreshnessState::Fresh);

    // Doctor report succeeds on fallback with explanatory note
    let report = auto_source
        .diagnose()
        .expect("Diagnose succeeds on fallback");
    assert!(report.is_reachable);
    assert_eq!(report.provider_name, "NativeWindows");
    assert!(report
        .diagnostics
        .iter()
        .any(|d| d.contains("seamlessly fallen back")));
}

// 5. AutoSourceAdapter prefers ActivityWatch when server is running and reachable
#[tokio::test]
async fn test_05_auto_source_adapter_prefers_activitywatch_when_available() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/0/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "TESTPC",
            "version": "v0.14.0",
            "testing": false,
            "profile": "default"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/0/buckets/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "aw-watcher-window_TESTPC": {
                "id": "aw-watcher-window_TESTPC",
                "type": "currentwindow",
                "client": "aw-watcher-window",
                "hostname": "TESTPC"
            },
            "aw-watcher-afk_TESTPC": {
                "id": "aw-watcher-afk_TESTPC",
                "type": "afkstatus",
                "client": "aw-watcher-afk",
                "hostname": "TESTPC"
            }
        })))
        .mount(&server)
        .await;

    let base_url = server.uri();
    tokio::task::spawn_blocking(move || {
        let aw_config = make_aw_config(&base_url);
        let mut auto_source = AutoSourceAdapter::new(aw_config);

        let caps = auto_source
            .connect()
            .expect("Connects to ActivityWatch primary");
        assert_eq!(caps.provider, "activitywatch");
        assert_eq!(auto_source.active_backend(), ActiveBackend::ActivityWatch);
        assert!(!auto_source.is_fallback_active());

        let report = auto_source
            .diagnose()
            .expect("Diagnoses ActivityWatch primary");
        assert_eq!(report.provider_name, "ActivityWatch");
        assert!(report.is_reachable);
    })
    .await
    .unwrap();
}

// 6. AutoSourceAdapter dynamically falls back to Native Windows if AW disconnects mid-session
#[test]
fn test_06_auto_source_adapter_dynamic_mid_session_fallback() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_port = listener.local_addr().unwrap().port();
    drop(listener);

    let closed_url = format!("http://127.0.0.1:{}", closed_port);
    let aw_config = make_aw_config(&closed_url);
    let mut auto_source = AutoSourceAdapter::new(aw_config);

    // Initial backend is ActivityWatch
    assert_eq!(auto_source.active_backend(), ActiveBackend::ActivityWatch);

    // When polling while ActivityWatch is disconnected, it detects the disconnection
    // and seamlessly switches backend to NativeWindows
    let obs = auto_source
        .poll()
        .expect("Polls successfully via failover to native windows");
    assert_eq!(auto_source.active_backend(), ActiveBackend::NativeWindows);
    assert!(auto_source.is_fallback_active());
    assert_eq!(obs.source_health, SourceHealth::Connected);
    assert_eq!(obs.window_freshness, FreshnessState::Fresh);
}

// 7. create_activity_source factory obeys provider configurations
#[test]
fn test_07_create_activity_source_factory() {
    let mut config = FlowGuardConfig::default();

    // Native explicit provider
    config.activity_source.provider = "native_windows".to_string();
    let mut native_src = create_activity_source(&config);
    let caps = native_src.connect().unwrap();
    assert_eq!(caps.provider, "native_windows");

    // Auto provider
    config.activity_source.provider = "auto".to_string();
    config.activity_source.activitywatch.base_url = "http://127.0.0.1:59987".to_string();
    let mut auto_src = create_activity_source(&config);
    // Unreachable AW falls back to native
    let auto_caps = auto_src.connect().unwrap();
    assert!(auto_caps.provider.contains("native_windows"));
}

// 8. ActivityService::new_auto works out of the box for novice users
#[test]
fn test_08_activity_service_new_auto_out_of_the_box() {
    let mut config = FlowGuardConfig::default();
    config.activity_source.activitywatch.base_url = "http://127.0.0.1:59986".to_string();

    let mut service = ActivityService::new_auto(config);

    // Doctor report works out of the box without ActivityWatch running
    let report = service.run_doctor();
    assert!(report.is_reachable);
    assert_eq!(report.health, SourceHealth::Connected);

    // Status works out of the box
    let (health, caps) = service.status();
    assert_eq!(health, SourceHealth::Connected);
    assert!(caps.application_tracking);

    // Observe once works out of the box
    let obs = service
        .observe_once()
        .expect("Immediate observation succeeds");
    assert_eq!(obs.source_health, SourceHealth::Connected);
}
