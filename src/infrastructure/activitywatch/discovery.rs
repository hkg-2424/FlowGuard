use std::collections::HashMap;
use tracing::{debug, warn};

use super::models::BucketMetadata;
use crate::config::BucketOverridesConfig;

/// Represents the buckets discovered and selected for activity observation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiscoveredBuckets {
    /// Bucket recording desktop window changes (`currentwindow`). Essential.
    pub window_bucket: Option<String>,
    /// Bucket recording user AFK state (`afkstatus`). Optional.
    pub afk_bucket: Option<String>,
    /// Bucket recording browser tab activity (`web.tab.current`). Optional.
    pub web_bucket: Option<String>,
    /// Informational warnings or ambiguity alerts produced during discovery.
    pub diagnostics: Vec<String>,
}

impl DiscoveredBuckets {
    /// True if the essential window watcher bucket is present.
    pub fn has_essential_window_bucket(&self) -> bool {
        self.window_bucket.is_some()
    }
}

/// Discovers and selects the best matching ActivityWatch buckets dynamically.
///
/// Rules:
/// 1. Prioritize configured explicit overrides if defined.
/// 2. Match based on ActivityWatch `type` metadata (`currentwindow`, `afkstatus`, `web.tab.current`).
/// 3. Prefer buckets matching the server's local hostname.
/// 4. If multiple matching buckets exist on the same host, flag the ambiguity with a diagnostic.
/// 5. Never hardcode machine-specific bucket IDs.
pub fn discover_buckets(
    buckets: &HashMap<String, BucketMetadata>,
    server_hostname: &str,
    overrides: Option<&BucketOverridesConfig>,
) -> DiscoveredBuckets {
    let mut discovered = DiscoveredBuckets::default();

    // 1. Discover Window Bucket (Essential)
    discovered.window_bucket = select_bucket(
        buckets,
        server_hostname,
        "window",
        overrides.and_then(|o| o.window_bucket.as_deref()),
        |b| b.event_type == "currentwindow" || b.client == "aw-watcher-window",
        &mut discovered.diagnostics,
    );

    if discovered.window_bucket.is_none() {
        discovered.diagnostics.push(
            "Essential window watcher bucket (type: 'currentwindow') was not found. Window activity cannot be observed."
                .to_string(),
        );
    }

    // 2. Discover AFK Bucket (Optional)
    discovered.afk_bucket = select_bucket(
        buckets,
        server_hostname,
        "afk",
        overrides.and_then(|o| o.afk_bucket.as_deref()),
        |b| b.event_type == "afkstatus" || b.client == "aw-watcher-afk",
        &mut discovered.diagnostics,
    );

    if discovered.afk_bucket.is_none() {
        discovered.diagnostics.push(
            "AFK watcher bucket (type: 'afkstatus') was not found. Idle detection will be marked as Unknown."
                .to_string(),
        );
    }

    // 3. Discover Web/Browser Bucket (Optional)
    discovered.web_bucket = select_bucket(
        buckets,
        server_hostname,
        "web",
        overrides.and_then(|o| o.web_bucket.as_deref()),
        |b| {
            b.event_type == "web.tab.current"
                || b.client.starts_with("aw-watcher-web")
                || b.id.starts_with("aw-watcher-web")
        },
        &mut discovered.diagnostics,
    );

    if discovered.web_bucket.is_none() {
        debug!("No web watcher bucket discovered (optional capability).");
    }

    discovered
}

fn select_bucket<F>(
    buckets: &HashMap<String, BucketMetadata>,
    server_hostname: &str,
    category: &str,
    explicit_override: Option<&str>,
    predicate: F,
    diagnostics: &mut Vec<String>,
) -> Option<String>
where
    F: Fn(&BucketMetadata) -> bool,
{
    // Check if an explicit override is configured
    if let Some(override_id) = explicit_override {
        if buckets.contains_key(override_id) {
            debug!(
                "Using explicit bucket override for {}: '{}'",
                category, override_id
            );
            return Some(override_id.to_string());
        } else {
            diagnostics.push(format!(
                "Configured bucket override '{}' for {} was not found in ActivityWatch bucket list.",
                override_id, category
            ));
        }
    }

    // Find all matching buckets
    let matching: Vec<&BucketMetadata> = buckets.values().filter(|b| predicate(b)).collect();

    if matching.is_empty() {
        return None;
    }

    // Separate buckets matching local hostname from other hosts
    let same_host: Vec<&BucketMetadata> = matching
        .iter()
        .copied()
        .filter(|b| b.hostname.eq_ignore_ascii_case(server_hostname))
        .collect();

    let candidates = if !same_host.is_empty() {
        same_host
    } else {
        matching
    };

    if candidates.len() > 1 {
        let bucket_ids: Vec<String> = candidates.iter().map(|b| b.id.clone()).collect();
        let warning = format!(
            "Ambiguity detected: Multiple matching buckets for {} on host '{}': [{}]. Selecting '{}'. Specify an explicit override in flowguard.toml to resolve.",
            category, server_hostname, bucket_ids.join(", "), candidates[0].id
        );
        warn!("{}", warning);
        diagnostics.push(warning);
    }

    Some(candidates[0].id.clone())
}
