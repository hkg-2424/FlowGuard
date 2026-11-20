use std::collections::HashMap;
use tracing::{debug, warn};

use super::models::BucketMetadata;
use crate::config::BucketOverridesConfig;
use crate::domain::ActivityCapabilities;

/// Represents the buckets discovered and selected for activity observation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiscoveredBuckets {
    /// Bucket recording desktop window changes (`currentwindow`). Essential.
    pub window_bucket: Option<String>,
    /// Bucket recording user AFK state (`afkstatus`). Optional.
    pub afk_bucket: Option<String>,
    /// Bucket recording browser tab activity (`web.tab.current`). Optional.
    pub web_bucket: Option<String>,
    /// Structural capabilities verified for these discovered buckets.
    pub capabilities: ActivityCapabilities,
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
/// 1. Prioritize configured explicit overrides if defined and valid.
/// 2. If exactly one candidate matches, select it.
/// 3. If multiple candidates match on host, check for a unique canonical bucket.
/// 4. If ambiguity cannot be resolved automatically, do NOT guess. Return None and report diagnostic.
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
            "Essential window watcher bucket (type: 'currentwindow') was not found or is ambiguous. Window activity cannot be observed."
                .to_string(),
        );
    }

    // 2. Discover AFK Bucket (Essential for idle tracking)
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
            "AFK watcher bucket (type: 'afkstatus') was not found or is ambiguous. Idle detection will be marked as Unknown."
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

    // 4. Derive Structural Capabilities from Discovered & Verified Watchers
    let web_supports_incognito = if let Some(ref w_id) = discovered.web_bucket {
        buckets
            .get(w_id)
            .map(|m| m.client.starts_with("aw-watcher-web"))
            .unwrap_or(false)
    } else {
        false
    };

    discovered.capabilities = ActivityCapabilities {
        application_tracking: discovered.window_bucket.is_some(),
        window_title_tracking: discovered.window_bucket.is_some(),
        idle_tracking: discovered.afk_bucket.is_some(),
        browser_url_tracking: discovered.web_bucket.is_some(),
        incognito_detection: web_supports_incognito,
    };

    discovered
}

/// Identifies whether a bucket matches the canonical ActivityWatch naming pattern for a category.
pub fn is_canonical_bucket(b: &BucketMetadata, category: &str, hostname: &str) -> bool {
    let lower_host = hostname.to_ascii_lowercase();
    let lower_id = b.id.to_ascii_lowercase();
    match category {
        "window" => {
            lower_id == format!("aw-watcher-window_{}", lower_host)
                && (b.event_type == "currentwindow" || b.client == "aw-watcher-window")
        }
        "afk" => {
            lower_id == format!("aw-watcher-afk_{}", lower_host)
                && (b.event_type == "afkstatus" || b.client == "aw-watcher-afk")
        }
        "web" => {
            (lower_id == format!("aw-watcher-web_{}", lower_host)
                || lower_id == format!("aw-watcher-web-chrome_{}", lower_host)
                || lower_id == format!("aw-watcher-web-firefox_{}", lower_host)
                || lower_id == format!("aw-watcher-web-brave_{}", lower_host)
                || lower_id == format!("aw-watcher-web-edge_{}", lower_host))
                && b.event_type == "web.tab.current"
        }
        _ => false,
    }
}

pub fn select_bucket<F>(
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
    // 1. Explicit configured bucket override takes precedence after validating metadata
    if let Some(override_id) = explicit_override {
        if let Some(meta) = buckets.get(override_id) {
            if predicate(meta) {
                debug!(
                    "Using valid explicit bucket override for {}: '{}'",
                    category, override_id
                );
                return Some(override_id.to_string());
            } else {
                let warning = format!(
                    "Configured bucket override '{}' for {} exists but does not match expected watcher category (type: '{}', client: '{}'). Override rejected.",
                    override_id, category, meta.event_type, meta.client
                );
                warn!("{}", warning);
                diagnostics.push(warning);
                return None;
            }
        } else {
            let warning = format!(
                "Configured bucket override '{}' for {} was not found in ActivityWatch bucket list.",
                override_id, category
            );
            warn!("{}", warning);
            diagnostics.push(warning);
            return None;
        }
    }

    // 2. Find all matching buckets
    let matching: Vec<&BucketMetadata> = buckets.values().filter(|b| predicate(b)).collect();

    if matching.is_empty() {
        return None;
    }

    // Require same-host match for automatic bucket discovery (H-02)
    let same_host: Vec<&BucketMetadata> = matching
        .iter()
        .copied()
        .filter(|b| b.hostname.eq_ignore_ascii_case(server_hostname))
        .collect();

    if same_host.is_empty() {
        let foreign_candidates: Vec<String> = matching
            .iter()
            .map(|b| format!("'{}' (host: '{}')", b.id, b.hostname))
            .collect();
        let warning = format!(
            "Foreign bucket rejection (H-02): Found {} candidates for {} from other machines [{}], but none matching local hostname '{}'. Foreign buckets are rejected for local foreground tracking without an explicit bucket override.",
            matching.len(),
            category,
            foreign_candidates.join(", "),
            server_hostname
        );
        warn!("{}", warning);
        diagnostics.push(warning);
        return None;
    }

    let candidates = same_host;

    // Exactly one candidate found
    if candidates.len() == 1 {
        return Some(candidates[0].id.clone());
    }

    // 3. Multiple candidates: check for unique canonical match
    let mut candidate_ids: Vec<String> = candidates.iter().map(|b| b.id.clone()).collect();
    candidate_ids.sort(); // Deterministic sorting

    let canonical_matches: Vec<&BucketMetadata> = candidates
        .iter()
        .copied()
        .filter(|b| is_canonical_bucket(b, category, server_hostname))
        .collect();

    if canonical_matches.len() == 1 {
        let selected_id = canonical_matches[0].id.clone();
        let info = format!(
            "Ambiguity detected: Multiple candidates for {} on host '{}': [{}]. Resolved by selecting unique canonical bucket '{}'.",
            category, server_hostname, candidate_ids.join(", "), selected_id
        );
        debug!("{}", info);
        diagnostics.push(info);
        return Some(selected_id);
    }

    // 4 & 5. Still ambiguous: do NOT guess! Emit diagnostic listing competing buckets and request override
    let warning = format!(
        "Ambiguity detected: Multiple competing buckets for {} on host '{}': [{}]. Ambiguity cannot be resolved automatically. Please specify an explicit override in flowguard.toml under [activity_source.activitywatch.bucket_overrides].",
        category, server_hostname, candidate_ids.join(", ")
    );
    warn!("{}", warning);
    diagnostics.push(warning);
    None
}
