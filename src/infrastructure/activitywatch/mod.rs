pub mod client;
pub mod discovery;
pub mod mapper;
pub mod models;

use tracing::{debug, info};

use crate::application::ports::{ActivitySource, SourceError};
use crate::config::ActivityWatchConfig;
use crate::domain::{
    ActivityObservation, DiagnosticReport, FreshnessState, SourceCapabilities, SourceHealth,
    SubsystemDiagnostic, SubsystemStatus,
};
use client::ActivityWatchClient;
use discovery::{discover_buckets, DiscoveredBuckets};
use mapper::{check_afk_freshness, check_freshness, map_events_to_observation, WatcherResult};
use models::{AwRawEvent, ServerInfo};

/// Helper to convert a fetch result into a `WatcherResult` model.
fn map_fetch_result<'a>(
    res: &'a Result<Option<AwRawEvent>, SourceError>,
    bucket_configured: bool,
) -> WatcherResult<'a> {
    if !bucket_configured {
        return WatcherResult::NotConfigured;
    }
    match res {
        Ok(Some(ev)) => WatcherResult::Present(ev),
        Ok(None) => WatcherResult::Empty,
        Err(SourceError::Timeout(ms)) => WatcherResult::Timeout(*ms),
        Err(SourceError::MissingRequiredBucket(id)) => WatcherResult::MissingBucket(id.clone()),
        Err(e) => WatcherResult::FetchError(e.to_string()),
    }
}

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

        let initial_capabilities = discovered.capabilities.clone();

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

        // Query each watcher channel
        let window_res = match buckets.window_bucket.as_ref() {
            Some(bid) => self.client.get_latest_event(bid),
            None => Ok(None),
        };
        if let Err(SourceError::ConnectionRefused(ref e)) = window_res {
            self.health = SourceHealth::Disconnected;
            return Ok(ActivityObservation::disconnected(
                self.health,
                vec![format!(
                    "Connection lost while querying window bucket: {}",
                    e
                )],
            ));
        }

        let afk_res = match buckets.afk_bucket.as_ref() {
            Some(bid) => self.client.get_latest_event(bid),
            None => Ok(None),
        };
        if let Err(SourceError::ConnectionRefused(ref e)) = afk_res {
            self.health = SourceHealth::Disconnected;
            return Ok(ActivityObservation::disconnected(
                self.health,
                vec![format!("Connection lost while querying AFK bucket: {}", e)],
            ));
        }

        let web_res = match buckets.web_bucket.as_ref() {
            Some(bid) => self.client.get_latest_event(bid),
            None => Ok(None),
        };
        if let Err(SourceError::ConnectionRefused(ref e)) = web_res {
            self.health = SourceHealth::Disconnected;
            return Ok(ActivityObservation::disconnected(
                self.health,
                vec![format!("Connection lost while querying web bucket: {}", e)],
            ));
        }

        let window_channel = map_fetch_result(&window_res, buckets.window_bucket.is_some());
        let afk_channel = map_fetch_result(&afk_res, buckets.afk_bucket.is_some());
        let web_channel = map_fetch_result(&web_res, buckets.web_bucket.is_some());

        // Invalidate any 404 missing buckets to prevent hammering deleted endpoints, then attempt rediscovery
        let mut needs_rediscovery = false;
        if let Err(SourceError::MissingRequiredBucket(ref bid)) = window_res {
            tracing::warn!("Window bucket '{}' returned 404. Invalidating reference and triggering rediscovery.", bid);
            if let Some(ref mut b) = self.buckets {
                b.window_bucket = None;
            }
            needs_rediscovery = true;
        }
        if let Err(SourceError::MissingRequiredBucket(ref bid)) = afk_res {
            tracing::warn!(
                "AFK bucket '{}' returned 404. Invalidating reference and triggering rediscovery.",
                bid
            );
            if let Some(ref mut b) = self.buckets {
                b.afk_bucket = None;
            }
            needs_rediscovery = true;
        }
        if let Err(SourceError::MissingRequiredBucket(ref bid)) = web_res {
            tracing::warn!(
                "Web bucket '{}' returned 404. Invalidating reference and triggering rediscovery.",
                bid
            );
            if let Some(ref mut b) = self.buckets {
                b.web_bucket = None;
            }
            needs_rediscovery = true;
        }

        if needs_rediscovery {
            if let Ok(all_buckets) = self.client.get_buckets() {
                let hostname = self
                    .server_info
                    .as_ref()
                    .map(|s| s.hostname.as_str())
                    .unwrap_or("");
                let new_discovered = discover_buckets(
                    &all_buckets,
                    hostname,
                    self.config.bucket_overrides.as_ref(),
                );
                self.buckets = Some(new_discovered);
            }
        }

        // Map events to FlowGuard domain observation
        let observation = map_events_to_observation(
            window_channel,
            afk_channel,
            web_channel,
            buckets.capabilities.clone(),
            self.config.stale_after_secs,
            self.config.afk_timeout_secs,
            &buckets.diagnostics,
        );

        self.health = observation.source_health;

        Ok(observation)
    }

    fn health(&self) -> SourceHealth {
        self.health
    }

    fn diagnose(&mut self) -> Result<DiagnosticReport, SourceError> {
        let endpoint = self.config.base_url.clone();

        // Ensure connected
        if !self.health.is_operational() || self.buckets.is_none() {
            if let Err(e) = self.perform_connection() {
                let health = match &e {
                    SourceError::WrongService { .. } => SourceHealth::WrongService,
                    _ => SourceHealth::Disconnected,
                };
                return Ok(DiagnosticReport::unreachable(
                    "ActivityWatch",
                    endpoint,
                    health,
                    format!("Failed to connect to ActivityWatch: {}", e),
                ));
            }
        }

        let info = self.server_info.as_ref().cloned();
        let buckets = match self.buckets.as_ref() {
            Some(b) => b.clone(),
            None => {
                return Ok(DiagnosticReport::unreachable(
                    "ActivityWatch",
                    endpoint,
                    SourceHealth::Disconnected,
                    "No buckets discovered.".to_string(),
                ));
            }
        };

        let now = chrono::Utc::now();
        let mut subsystems = Vec::new();
        let mut doctor_diagnostics = buckets.diagnostics.clone();

        // 1. Desktop Window Subsystem
        if let Some(ref bid) = buckets.window_bucket {
            match self.client.get_latest_event(bid) {
                Ok(Some(ev)) => {
                    let (freshness, age) =
                        check_freshness(Some(&ev), now, self.config.stale_after_secs);
                    let (status, details) = match freshness {
                        FreshnessState::Fresh => (
                            SubsystemStatus::Healthy,
                            format!("Active and fresh (age: {}s)", age.unwrap_or(0)),
                        ),
                        FreshnessState::Stale => {
                            let msg = format!(
                                "Stale: last event recorded {}s ago (threshold: {}s)",
                                age.unwrap_or(0),
                                self.config.stale_after_secs
                            );
                            doctor_diagnostics.push(format!("Window watcher is stale: {}", msg));
                            (SubsystemStatus::Degraded, msg)
                        }
                        FreshnessState::Missing => {
                            (SubsystemStatus::Degraded, "No recent events".to_string())
                        }
                    };
                    subsystems.push(SubsystemDiagnostic {
                        name: "Desktop Window".to_string(),
                        target: Some(bid.clone()),
                        status,
                        freshness: Some(freshness),
                        age_seconds: age,
                        details,
                    });
                }
                Ok(None) => {
                    doctor_diagnostics.push(format!(
                        "Window watcher bucket '{}' has no events recorded yet.",
                        bid
                    ));
                    subsystems.push(SubsystemDiagnostic {
                        name: "Desktop Window".to_string(),
                        target: Some(bid.clone()),
                        status: SubsystemStatus::Degraded,
                        freshness: Some(FreshnessState::Missing),
                        age_seconds: None,
                        details: "Bucket exists but contains no recorded events yet.".to_string(),
                    });
                }
                Err(e) => {
                    doctor_diagnostics
                        .push(format!("Error querying window watcher '{}': {}", bid, e));
                    subsystems.push(SubsystemDiagnostic {
                        name: "Desktop Window".to_string(),
                        target: Some(bid.clone()),
                        status: SubsystemStatus::Degraded,
                        freshness: Some(FreshnessState::Stale),
                        age_seconds: None,
                        details: format!("Query failed: {}", e),
                    });
                }
            }
        } else {
            doctor_diagnostics.push("No desktop window watcher bucket found.".to_string());
            subsystems.push(SubsystemDiagnostic {
                name: "Desktop Window".to_string(),
                target: None,
                status: SubsystemStatus::Unavailable,
                freshness: Some(FreshnessState::Missing),
                age_seconds: None,
                details:
                    "No matching window watcher bucket found. aw-watcher-window may not be running."
                        .to_string(),
            });
        }

        // 2. User Idle / AFK Subsystem
        if let Some(ref bid) = buckets.afk_bucket {
            match self.client.get_latest_event(bid) {
                Ok(Some(ev)) => {
                    let (freshness, age) = check_afk_freshness(
                        Some(&ev),
                        now,
                        self.config.stale_after_secs,
                        self.config.afk_timeout_secs,
                    );
                    let (status, details) = match freshness {
                        FreshnessState::Fresh => (
                            SubsystemStatus::Healthy,
                            format!("Active and fresh (age: {}s)", age.unwrap_or(0)),
                        ),
                        FreshnessState::Stale => {
                            let msg = format!(
                                "Stale: last event recorded {}s ago (threshold: {}s)",
                                age.unwrap_or(0),
                                self.config.stale_after_secs
                            );
                            doctor_diagnostics.push(format!("AFK watcher is stale: {}", msg));
                            (SubsystemStatus::Degraded, msg)
                        }
                        FreshnessState::Missing => {
                            (SubsystemStatus::Degraded, "No recent events".to_string())
                        }
                    };
                    subsystems.push(SubsystemDiagnostic {
                        name: "User Idle / AFK".to_string(),
                        target: Some(bid.clone()),
                        status,
                        freshness: Some(freshness),
                        age_seconds: age,
                        details,
                    });
                }
                Ok(None) => {
                    doctor_diagnostics.push(format!(
                        "AFK watcher bucket '{}' has no events recorded yet.",
                        bid
                    ));
                    subsystems.push(SubsystemDiagnostic {
                        name: "User Idle / AFK".to_string(),
                        target: Some(bid.clone()),
                        status: SubsystemStatus::Degraded,
                        freshness: Some(FreshnessState::Missing),
                        age_seconds: None,
                        details: "Bucket exists but contains no recorded events yet.".to_string(),
                    });
                }
                Err(e) => {
                    doctor_diagnostics.push(format!("Error querying AFK watcher '{}': {}", bid, e));
                    subsystems.push(SubsystemDiagnostic {
                        name: "User Idle / AFK".to_string(),
                        target: Some(bid.clone()),
                        status: SubsystemStatus::Degraded,
                        freshness: Some(FreshnessState::Stale),
                        age_seconds: None,
                        details: format!("Query failed: {}", e),
                    });
                }
            }
        } else {
            doctor_diagnostics.push("No AFK watcher bucket found.".to_string());
            subsystems.push(SubsystemDiagnostic {
                name: "User Idle / AFK".to_string(),
                target: None,
                status: SubsystemStatus::Unavailable,
                freshness: Some(FreshnessState::Missing),
                age_seconds: None,
                details: "No matching AFK watcher bucket found. aw-watcher-afk may not be running."
                    .to_string(),
            });
        }

        // 3. Browser Extension Subsystem
        if let Some(ref bid) = buckets.web_bucket {
            match self.client.get_latest_event(bid) {
                Ok(Some(ev)) => {
                    let (freshness, age) =
                        check_freshness(Some(&ev), now, self.config.stale_after_secs);
                    let (status, details) = match freshness {
                        FreshnessState::Fresh => (
                            SubsystemStatus::Healthy,
                            format!("Active and fresh (age: {}s)", age.unwrap_or(0)),
                        ),
                        FreshnessState::Stale => (
                            SubsystemStatus::Degraded,
                            format!("Inactive/Stale (age: {}s)", age.unwrap_or(0)),
                        ),
                        FreshnessState::Missing => {
                            (SubsystemStatus::Degraded, "No recent events".to_string())
                        }
                    };
                    subsystems.push(SubsystemDiagnostic {
                        name: "Browser Extension".to_string(),
                        target: Some(bid.clone()),
                        status,
                        freshness: Some(freshness),
                        age_seconds: age,
                        details,
                    });
                }
                Ok(None) => {
                    subsystems.push(SubsystemDiagnostic {
                        name: "Browser Extension".to_string(),
                        target: Some(bid.clone()),
                        status: SubsystemStatus::Degraded,
                        freshness: Some(FreshnessState::Missing),
                        age_seconds: None,
                        details: "Bucket exists but contains no recorded web events yet."
                            .to_string(),
                    });
                }
                Err(e) => {
                    subsystems.push(SubsystemDiagnostic {
                        name: "Browser Extension".to_string(),
                        target: Some(bid.clone()),
                        status: SubsystemStatus::Degraded,
                        freshness: Some(FreshnessState::Stale),
                        age_seconds: None,
                        details: format!("Query failed: {}", e),
                    });
                }
            }
        } else {
            subsystems.push(SubsystemDiagnostic {
                name: "Browser Extension".to_string(),
                target: None,
                status: SubsystemStatus::Unavailable,
                freshness: Some(FreshnessState::Missing),
                age_seconds: None,
                details: "Not installed or not active (optional).".to_string(),
            });
        }

        let is_window_healthy = subsystems
            .iter()
            .any(|s| s.name == "Desktop Window" && s.status == SubsystemStatus::Healthy);
        let is_afk_healthy = subsystems
            .iter()
            .any(|s| s.name == "User Idle / AFK" && s.status == SubsystemStatus::Healthy);
        let overall_health = if is_window_healthy && is_afk_healthy {
            SourceHealth::Connected
        } else {
            SourceHealth::Degraded
        };

        let is_testing = info.as_ref().map(|i| i.testing).unwrap_or(false);
        if is_testing {
            doctor_diagnostics
                .push("Notice: ActivityWatch server is running in testing/dev mode.".to_string());
        }

        Ok(DiagnosticReport {
            provider_name: "ActivityWatch".to_string(),
            endpoint,
            is_reachable: true,
            service_identity: Some("ActivityWatch".to_string()),
            server_version: info.as_ref().map(|i| i.version.clone()),
            hostname: info.as_ref().map(|i| i.hostname.clone()),
            is_testing,
            server_profile: info.as_ref().and_then(|i| i.profile.clone()),
            subsystems,
            capabilities: buckets.capabilities.clone(),
            health: overall_health,
            diagnostics: doctor_diagnostics,
        })
    }
}
