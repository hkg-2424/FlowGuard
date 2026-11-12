use reqwest::blocking::{Client, RequestBuilder};
use reqwest::StatusCode;
use std::time::Duration;
use tracing::debug;

use super::models::{AwRawEvent, BucketMap, ServerInfo};
use crate::application::ports::SourceError;

/// HTTP client for interacting with the local ActivityWatch REST API.
///
/// Designed to be instantiated once and reused across polling cycles.
/// Enforces request timeouts and strictly verifies service identity.
#[derive(Debug, Clone)]
pub struct ActivityWatchClient {
    base_url: String,
    timeout_ms: u64,
    api_key: Option<String>,
    client: Client,
}

impl ActivityWatchClient {
    /// Creates a new ActivityWatch client with the specified configuration, returning a [`SourceError`]
    /// if the underlying HTTP client cannot be constructed with the configured timeout.
    ///
    /// This strictly prevents silently discarding configured timeout bounds or falling back
    /// to an unconstrained connection.
    pub fn try_new(
        base_url: &str,
        timeout_ms: u64,
        api_key_env: Option<&str>,
    ) -> Result<Self, SourceError> {
        let normalized_base = base_url.trim_end_matches('/').to_string();

        let api_key = api_key_env.and_then(|env_var| match std::env::var(env_var) {
            Ok(val) if !val.trim().is_empty() => {
                debug!(
                    "Loaded ActivityWatch API key from environment variable '{}'",
                    env_var
                );
                Some(val.trim().to_string())
            }
            _ => None,
        });

        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .map_err(|e| {
                SourceError::Other(format!(
                    "Failed to configure HTTP client with timeout for '{}': {}",
                    normalized_base, e
                ))
            })?;

        Ok(Self {
            base_url: normalized_base,
            timeout_ms,
            api_key,
            client,
        })
    }

    /// Creates a new ActivityWatch client with the specified configuration.
    ///
    /// # Panics:
    /// Panics if the underlying HTTP client cannot be constructed with the configured timeout.
    /// For fallible construction, use [`ActivityWatchClient::try_new`].
    pub fn new(base_url: &str, timeout_ms: u64, api_key_env: Option<&str>) -> Self {
        Self::try_new(base_url, timeout_ms, api_key_env)
            .expect("Failed to initialize ActivityWatch HTTP client with configured timeout")
    }

    /// Returns the configured base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Attaches common headers (e.g. optional authentication) to an outgoing request.
    fn prepare_request(&self, req: RequestBuilder) -> RequestBuilder {
        if let Some(ref key) = self.api_key {
            req.header("Authorization", format!("Bearer {}", key))
        } else {
            req
        }
    }

    /// Performs the initial handshake by querying `GET /api/0/info`.
    ///
    /// Validates that the endpoint is reachable AND that it identifies itself as
    /// a valid ActivityWatch server. Distinguishes between connection errors,
    /// timeouts, authorization errors, and unrelated applications occupying the port.
    pub fn handshake(&self) -> Result<ServerInfo, SourceError> {
        let endpoint = format!("{}/api/0/info", self.base_url);
        debug!("Connecting to ActivityWatch server at '{}'", endpoint);

        let req = self.prepare_request(self.client.get(&endpoint));

        let response = match req.send() {
            Ok(resp) => resp,
            Err(err) => {
                if err.is_connect() {
                    return Err(SourceError::ConnectionRefused(self.base_url.clone()));
                }
                if err.is_timeout() {
                    return Err(SourceError::Timeout(self.timeout_ms));
                }
                return Err(SourceError::ConnectionRefused(format!(
                    "{}: {}",
                    self.base_url, err
                )));
            }
        };

        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(SourceError::AuthRequired(self.base_url.clone()));
        }

        if !status.is_success() {
            // If /api/0/info is not 200 OK (e.g. 404 Not Found), another service is occupying the port.
            return Err(SourceError::WrongService {
                endpoint: self.base_url.clone(),
                reason: format!(
                    "Endpoint returned HTTP {} instead of 200 OK. Another application may be occupying this port.",
                    status
                ),
            });
        }

        let body_bytes = response.bytes().map_err(|e| {
            SourceError::SerializationError(format!("Failed to read response body: {}", e))
        })?;

        let info: ServerInfo = match serde_json::from_slice(&body_bytes) {
            Ok(parsed) => parsed,
            Err(e) => {
                return Err(SourceError::WrongService {
                    endpoint: self.base_url.clone(),
                    reason: format!(
                        "Response is valid JSON but does not match ActivityWatch server signature: {}. Another application is likely occupying this port.",
                        e
                    ),
                });
            }
        };

        // Strictly validate that essential ActivityWatch metadata is present and non-empty.
        if info.version.trim().is_empty() || info.hostname.trim().is_empty() {
            return Err(SourceError::WrongService {
                endpoint: self.base_url.clone(),
                reason: "Server info payload is missing expected ActivityWatch version or hostname fields."
                    .to_string(),
            });
        }

        Ok(info)
    }

    /// Retrieves the list of available buckets via `GET /api/0/buckets/`.
    pub fn get_buckets(&self) -> Result<BucketMap, SourceError> {
        let endpoint = format!("{}/api/0/buckets/", self.base_url);
        let req = self.prepare_request(self.client.get(&endpoint));

        let response = match req.send() {
            Ok(resp) => resp,
            Err(err) => {
                if err.is_connect() {
                    return Err(SourceError::ConnectionRefused(self.base_url.clone()));
                }
                if err.is_timeout() {
                    return Err(SourceError::Timeout(self.timeout_ms));
                }
                return Err(SourceError::ConnectionRefused(format!(
                    "{}: {}",
                    self.base_url, err
                )));
            }
        };

        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(SourceError::AuthRequired(self.base_url.clone()));
        }

        if !status.is_success() {
            return Err(SourceError::ApiError {
                status: status.as_u16(),
                message: format!("Failed to retrieve buckets from '{}'", endpoint),
            });
        }

        let buckets: BucketMap = response.json().map_err(|e| {
            SourceError::SerializationError(format!("Failed to parse bucket list: {}", e))
        })?;

        Ok(buckets)
    }

    /// Retrieves the single latest event from a specific bucket via `GET /api/0/buckets/{id}/events?limit=1`.
    pub fn get_latest_event(&self, bucket_id: &str) -> Result<Option<AwRawEvent>, SourceError> {
        let endpoint = format!(
            "{}/api/0/buckets/{}/events?limit=1",
            self.base_url, bucket_id
        );
        let req = self.prepare_request(self.client.get(&endpoint));

        let response = match req.send() {
            Ok(resp) => resp,
            Err(err) => {
                if err.is_connect() {
                    return Err(SourceError::ConnectionRefused(self.base_url.clone()));
                }
                if err.is_timeout() {
                    return Err(SourceError::Timeout(self.timeout_ms));
                }
                return Err(SourceError::ConnectionRefused(format!(
                    "{}: {}",
                    self.base_url, err
                )));
            }
        };

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Err(SourceError::MissingRequiredBucket(bucket_id.to_string()));
        }

        if !status.is_success() {
            return Err(SourceError::ApiError {
                status: status.as_u16(),
                message: format!("Failed to retrieve event from bucket '{}'", bucket_id),
            });
        }

        let events: Vec<AwRawEvent> = response.json().map_err(|e| {
            SourceError::SerializationError(format!(
                "Failed to parse event from bucket '{}': {}",
                bucket_id, e
            ))
        })?;

        Ok(events.into_iter().next())
    }
}
