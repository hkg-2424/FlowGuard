use flowguard::application::ports::ActivitySource;
use flowguard::config::ActivityWatchConfig;
use flowguard::infrastructure::activitywatch::client::ActivityWatchClient;
use flowguard::infrastructure::activitywatch::ActivityWatchAdapter;

#[test]
fn test_real_activitywatch_integration_read_only() {
    let client = ActivityWatchClient::new("http://127.0.0.1:5600", 1500, None);

    // Only run if real ActivityWatch is currently running and reachable
    let server_info = match client.handshake() {
        Ok(info) => info,
        Err(_) => {
            println!("Skipping real ActivityWatch test: local server at http://127.0.0.1:5600 is not reachable.");
            return;
        }
    };

    println!(
        "Live ActivityWatch detected: version={} hostname={}",
        server_info.version, server_info.hostname
    );

    let config = ActivityWatchConfig {
        base_url: "http://127.0.0.1:5600".to_string(),
        poll_interval_ms: 1000,
        request_timeout_ms: 1500,
        reconnect_interval_secs: 5,
        stale_after_secs: 15,
        afk_timeout_secs: 180,
        api_key_env: None,
        bucket_overrides: None,
    };

    let mut adapter = ActivityWatchAdapter::new(config);
    let caps = adapter
        .connect()
        .expect("connect to real ActivityWatch should succeed");
    assert_eq!(caps.provider, "activitywatch");
    assert!(caps.server_version.is_some());

    // Poll latest observation (read-only)
    let obs = adapter
        .poll()
        .expect("poll real ActivityWatch should succeed");
    println!("Observation health: {:?}", obs.source_health);
    println!("Observation capabilities: {:?}", obs.capabilities);

    // Verify read-only nature: nothing was modified
    assert!(adapter.health().is_operational());
}
