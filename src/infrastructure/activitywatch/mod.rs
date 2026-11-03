pub mod client;
pub mod discovery;
pub mod mapper;
pub mod models;

use tracing::{debug, info, warn};

use crate::application::ports::{ActivitySource, SourceError};
use crate::config::ActivityWatchConfig;
use crate::domain::{ActivityCapabilities, ActivityObservation, SourceCapabilities, SourceHealth};
use client::ActivityWatchClient;
use discovery::{discover_buckets, DiscoveredBuckets};
use mapper::map_events_to_observation;
use models::ServerInfo;

/// ActivityWatch adapter implementing the `ActivitySource` trait.
///
/// Encapsulates all ActivityWatch REST API communications, bucket discovery,
/// error classification, and event translation.
pub struct ActivityWatchAdapter {
    config: ActivityWatchConfig,
    client: ActivityWatchClient,
    server_info: Option<ServerInfo>,
    buckets: Option<DiscoveredBuckets>,
    health: SourceHealth,
}

impl ActivityWatchAdapter {
    /// Creates a new ActivityWatchAdapter from configuration.
    pub fn new(config: ActivityWatchConfig) -> Self {
        let client = ActivityWatchClient::new(
            &config.base_url,
            config.request_timeout_ms,
            config.api_key_env.as_deref(),
        );

        Self {
            config,
            client,
            server_info: None,
            buckets: None,
            health: SourceHealth::Connecting,
        }
    }

    /// Helper to get reference to discovered buckets if available.
    pub fn discovered_buckets(&self) -> Option<&DiscoveredBuckets> {
        self.buckets.as_ref()
    }

    /// Helper to get reference to server info if handshake succeeded.
    pub fn server_info(&self) -> Option<&ServerInfo> {
        self.server_info.as_ref()
    }

    /// Direct access to client for diagnostics (doctor command).
    pub fn client(&self) -> &ActivityWatchClient {
        &self.client
    }

    /// Performs the handshake and bucket discovery sequence.
    fn perform_connection(&mut self) -> Result<SourceCapabilities, SourceError> {
        self.health = SourceHealth::Connecting;

        // 1. Handshake with server info validation
        let info = match self.client.handshake() {
            Ok(info) => {
                let v = if info.version.starts_with('v') || info.version.starts_with('V') {
                    info.version.clone()
                } else {
                    format!("v{}", info.version)
                };
                info!(
                    "Connected to ActivityWatch server {} on '{}' (testing: {})",
                    v, info.hostname, info.testing
                );
                info
            }
            Err(e) => {
                match &e {
                    SourceError::WrongService { .. } => self.health = SourceHealth::WrongService,
                    _ => self.health = SourceHealth::Disconnected,
                }
                return Err(e);
            }
        };

        // 2. Discover available buckets
        let all_buckets = match self.client.get_buckets() {
            Ok(b) => b,
            Err(e) => {
                self.health = SourceHealth::Disconnected;
                return Err(e);
            }
        };

        let discovered = discover_buckets(
            &all_buckets,
            &info.hostname,
            self.config.bucket_overrides.as_ref(),
        );

        let initial_capabilities = ActivityCapabilities {
            application_tracking: discovered.window_bucket.is_some(),
            window_title_tracking: discovered.window_bucket.is_some(),
            idle_tracking: discovered.afk_bucket.is_some(),
            browser_url_tracking: discovered.web_bucket.is_some(),
            incognito_detection: discovered.web_bucket.is_some(),
        };

        self.health = if discovered.has_essential_window_bucket() {
            if discovered.afk_bucket.is_some() {
                SourceHealth::Connected
            } else {
                SourceHealth::Degraded
            }
        } else {
            SourceHealth::Degraded
        };

        let source_caps = SourceCapabilities {
            provider: "activitywatch".to_string(),
            server_version: Some(info.version.clone()),
            hostname: Some(info.hostname.clone()),
            is_testing: info.testing,
            capabilities: initial_capabilities,
        };

        self.server_info = Some(info);
        self.buckets = Some(discovered);

        Ok(source_caps)
    }
}

impl ActivitySource for ActivityWatchAdapter {
    fn connect(&mut self) -> Result<SourceCapabilities, SourceError> {
        self.perform_connection()
    }

    fn poll(&mut self) -> Result<ActivityObservation, SourceError> {
        // If disconnected or not yet connected, attempt connection first
        if !self.health.is_operational() || self.buckets.is_none() {
            debug!(
                "Activity source is not operational ({:?}), attempting connection...",
                self.health
            );
            if let Err(e) = self.perform_connection() {
                let diagnostics = vec![format!("Failed to connect to ActivityWatch: {}", e)];
                return Ok(ActivityObservation::disconnected(self.health, diagnostics));
            }
        }

        let buckets = match self.buckets.as_ref() {
            Some(b) => b.clone(),
            None => {
                self.health = SourceHealth::Disconnected;
                return Ok(ActivityObservation::disconnected(
                    self.health,
                    vec!["No buckets available.".to_string()],
                ));
            }
        };

        // Fetch latest events from each discovered bucket
        let window_event = if let Some(ref bucket_id) = buckets.window_bucket {
            match self.client.get_latest_event(bucket_id) {
                Ok(ev) => ev,
                Err(e) => {
                    warn!("Failed to fetch window event from '{}': {}", bucket_id, e);
                    if matches!(e, SourceError::ConnectionRefused(_)) {
                        self.health = SourceHealth::Disconnected;
                        return Ok(ActivityObservation::disconnected(
                            self.health,
                            vec![format!(
                                "Connection lost while querying window bucket: {}",
                                e
                            )],
                        ));
                    }
                    None
                }
            }
        } else {
            None
        };

        let afk_event = if let Some(ref bucket_id) = buckets.afk_bucket {
            match self.client.get_latest_event(bucket_id) {
                Ok(ev) => ev,
                Err(e) => {
                    warn!("Failed to fetch AFK event from '{}': {}", bucket_id, e);
                    if matches!(e, SourceError::ConnectionRefused(_)) {
                        self.health = SourceHealth::Disconnected;
                        return Ok(ActivityObservation::disconnected(
                            self.health,
                            vec![format!("Connection lost while querying AFK bucket: {}", e)],
                        ));
                    }
                    None
                }
            }
        } else {
            None
        };

        let web_event = if let Some(ref bucket_id) = buckets.web_bucket {
            match self.client.get_latest_event(bucket_id) {
                Ok(ev) => ev,
                Err(e) => {
                    debug!("Failed to fetch web event from '{}': {}", bucket_id, e);
                    if matches!(e, SourceError::ConnectionRefused(_)) {
                        self.health = SourceHealth::Disconnected;
                        return Ok(ActivityObservation::disconnected(
                            self.health,
                            vec![format!("Connection lost while querying web bucket: {}", e)],
                        ));
                    }
                    None
                }
            }
        } else {
            None
        };

        // Map events to FlowGuard domain observation
        let observation = map_events_to_observation(
            window_event.as_ref(),
            afk_event.as_ref(),
            web_event.as_ref(),
            self.config.stale_after_secs,
            &buckets.diagnostics,
            buckets.web_bucket.is_some(),
        );

        self.health = observation.source_health;

        Ok(observation)
    }

    fn health(&self) -> SourceHealth {
        self.health
    }
}
