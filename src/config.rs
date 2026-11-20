use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read configuration file at '{path}': {source}")]
    IoError {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to parse TOML configuration from '{path}': {source}")]
    ParseError {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },

    #[error("Configuration validation error: {0}")]
    ValidationError(String),
}

/// Root configuration for FlowGuard.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlowGuardConfig {
    pub activity_source: ActivitySourceConfig,
}

/// Activity source provider selection and provider-specific configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivitySourceConfig {
    /// Active provider name ("activitywatch" for M1).
    pub provider: String,
    /// Settings for the ActivityWatch adapter.
    pub activitywatch: ActivityWatchConfig,
}

impl Default for ActivitySourceConfig {
    fn default() -> Self {
        Self {
            provider: "activitywatch".to_string(),
            activitywatch: ActivityWatchConfig::default(),
        }
    }
}

/// Configuration settings for the ActivityWatch REST adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityWatchConfig {
    /// Base URL for the ActivityWatch REST API (default: http://127.0.0.1:5600).
    pub base_url: String,
    /// Polling interval in milliseconds (default: 1000).
    pub poll_interval_ms: u64,
    /// HTTP request timeout in milliseconds (default: 1500).
    pub request_timeout_ms: u64,
    /// Reconnect retry interval in seconds when disconnected (default: 5).
    pub reconnect_interval_secs: u64,
    /// Staleness threshold in seconds for observations (default: 15).
    pub stale_after_secs: u64,
    /// AFK inactivity timeout in seconds (default: 180).
    /// ActivityWatch aw-watcher-afk defaults to 180s (3 minutes) before declaring AFK.
    /// During this window, periods without input are valid reading/thinking pauses and do not indicate AFK or staleness.
    pub afk_timeout_secs: u64,
    /// Environment variable name holding the optional API key (default: FLOWGUARD_ACTIVITYWATCH_API_KEY).
    pub api_key_env: Option<String>,
    /// Whether to allow remote (non-loopback) ActivityWatch hosts. Default: false.
    #[serde(default)]
    pub allow_remote: bool,
    /// Optional overrides for specific bucket IDs.
    pub bucket_overrides: Option<BucketOverridesConfig>,
}

impl Default for ActivityWatchConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:5600".to_string(),
            poll_interval_ms: 1000,
            request_timeout_ms: 1500,
            reconnect_interval_secs: 5,
            stale_after_secs: 15,
            afk_timeout_secs: 180,
            api_key_env: Some("FLOWGUARD_ACTIVITYWATCH_API_KEY".to_string()),
            allow_remote: false,
            bucket_overrides: None,
        }
    }
}

/// Optional explicit overrides for bucket identification in unusual setups.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BucketOverridesConfig {
    pub window_bucket: Option<String>,
    pub afk_bucket: Option<String>,
    pub web_bucket: Option<String>,
}

impl FlowGuardConfig {
    /// Validates all configuration parameters to prevent unexpected runtime failures.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let aw = &self.activity_source.activitywatch;

        // Structural URL validation (H-01)
        let parsed_url = url::Url::parse(&aw.base_url).map_err(|e| {
            ConfigError::ValidationError(format!("Invalid base_url '{}': {}", aw.base_url, e))
        })?;

        if parsed_url.scheme() != "http" && parsed_url.scheme() != "https" {
            return Err(ConfigError::ValidationError(format!(
                "Invalid base_url '{}'. Scheme must be 'http' or 'https'",
                aw.base_url
            )));
        }

        if !parsed_url.username().is_empty() || parsed_url.password().is_some() {
            return Err(ConfigError::ValidationError(format!(
                "Invalid base_url '{}': URL userinfo (username/password) is prohibited",
                aw.base_url
            )));
        }

        if parsed_url.query().is_some() {
            return Err(ConfigError::ValidationError(format!(
                "Invalid base_url '{}': query parameters are prohibited in base_url",
                aw.base_url
            )));
        }

        if parsed_url.fragment().is_some() {
            return Err(ConfigError::ValidationError(format!(
                "Invalid base_url '{}': URL fragments are prohibited in base_url",
                aw.base_url
            )));
        }

        let host = parsed_url.host().ok_or_else(|| {
            ConfigError::ValidationError(format!(
                "Invalid base_url '{}': host component is required",
                aw.base_url
            ))
        })?;

        let is_loopback = match host {
            url::Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
            url::Host::Ipv4(ip) => ip.is_loopback(),
            url::Host::Ipv6(ip) => ip.is_loopback(),
        };

        if !is_loopback && !aw.allow_remote {
            return Err(ConfigError::ValidationError(format!(
                "Invalid base_url '{}': remote ActivityWatch hosts are rejected by default. Set 'allow_remote = true' to opt in.",
                aw.base_url
            )));
        }

        if let Some(ref env_name) = aw.api_key_env {
            let upper = env_name.to_ascii_uppercase();
            if !upper.starts_with("FLOWGUARD_")
                && !upper.starts_with("AW_")
                && !upper.starts_with("ACTIVITYWATCH_")
            {
                return Err(ConfigError::ValidationError(format!(
                    "Invalid api_key_env '{}': environment variable name must start with 'FLOWGUARD_', 'AW_', or 'ACTIVITYWATCH_'",
                    env_name
                )));
            }
        }

        if aw.poll_interval_ms < 100 {
            return Err(ConfigError::ValidationError(format!(
                "poll_interval_ms ({}) is too small. Minimum is 100 ms to avoid excessive CPU usage.",
                aw.poll_interval_ms
            )));
        }

        if aw.request_timeout_ms < 100 {
            return Err(ConfigError::ValidationError(format!(
                "request_timeout_ms ({}) is too small. Minimum is 100 ms.",
                aw.request_timeout_ms
            )));
        }

        if aw.reconnect_interval_secs < 1 {
            return Err(ConfigError::ValidationError(format!(
                "reconnect_interval_secs ({}) must be at least 1 second.",
                aw.reconnect_interval_secs
            )));
        }

        if aw.stale_after_secs < 1 {
            return Err(ConfigError::ValidationError(format!(
                "stale_after_secs ({}) must be at least 1 second.",
                aw.stale_after_secs
            )));
        }

        if aw.afk_timeout_secs < 10 {
            return Err(ConfigError::ValidationError(format!(
                "afk_timeout_secs ({}) is too small. Minimum is 10 seconds.",
                aw.afk_timeout_secs
            )));
        }

        Ok(())
    }

    /// Loads configuration from candidate locations in order:
    /// 1. Explicit path passed as argument.
    /// 2. `./flowguard.toml` or `./config/flowguard.toml` in the current directory.
    /// 3. `%APPDATA%/flowguard/config.toml` (or platform equivalent).
    /// 4. Fallback to default configuration if none of the above exist.
    pub fn load_or_default(
        explicit_path: Option<&Path>,
    ) -> Result<(Self, Option<PathBuf>), ConfigError> {
        if let Some(path) = explicit_path {
            if path.exists() {
                let content = std::fs::read_to_string(path).map_err(|e| ConfigError::IoError {
                    path: path.to_path_buf(),
                    source: e,
                })?;
                let config: FlowGuardConfig =
                    toml::from_str(&content).map_err(|e| ConfigError::ParseError {
                        path: path.to_path_buf(),
                        source: Box::new(e),
                    })?;
                config.validate()?;
                return Ok((config, Some(path.to_path_buf())));
            } else {
                return Err(ConfigError::ValidationError(format!(
                    "Explicit configuration file '{}' was not found.",
                    path.display()
                )));
            }
        }

        // Check local directory candidates
        let local_candidates = [
            PathBuf::from("flowguard.toml"),
            PathBuf::from("config/flowguard.toml"),
        ];

        for path in &local_candidates {
            if path.exists() {
                let content = std::fs::read_to_string(path).map_err(|e| ConfigError::IoError {
                    path: path.clone(),
                    source: e,
                })?;
                let config: FlowGuardConfig =
                    toml::from_str(&content).map_err(|e| ConfigError::ParseError {
                        path: path.clone(),
                        source: Box::new(e),
                    })?;
                config.validate()?;
                return Ok((config, Some(path.clone())));
            }
        }

        // Check user configuration directory (e.g. %APPDATA%\flowguard\config.toml)
        if let Some(mut config_dir) = dirs::config_dir() {
            config_dir.push("flowguard");
            config_dir.push("config.toml");
            if config_dir.exists() {
                let content =
                    std::fs::read_to_string(&config_dir).map_err(|e| ConfigError::IoError {
                        path: config_dir.clone(),
                        source: e,
                    })?;
                let config: FlowGuardConfig =
                    toml::from_str(&content).map_err(|e| ConfigError::ParseError {
                        path: config_dir.clone(),
                        source: Box::new(e),
                    })?;
                config.validate()?;
                return Ok((config, Some(config_dir)));
            }
        }

        // Fallback to default
        let default_config = FlowGuardConfig::default();
        default_config.validate()?;
        Ok((default_config, None))
    }
}

/// Returns the standard path to the local SQLite database file.
///
/// On Windows: `%APPDATA%\flowguard\flowguard.db`
/// On macOS/Linux: `~/.local/share/flowguard/flowguard.db`
pub fn default_database_path() -> PathBuf {
    if let Some(mut data_dir) = dirs::data_dir() {
        data_dir.push("flowguard");
        let _ = std::fs::create_dir_all(&data_dir);
        data_dir.push("flowguard.db");
        data_dir
    } else {
        PathBuf::from("flowguard.db")
    }
}
