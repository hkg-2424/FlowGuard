use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::session::SessionId;

/// Type-safe newtype wrapper for a unique Workflow identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WorkflowId(pub String);

impl WorkflowId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type-safe newtype wrapper for a unique Rule identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RuleId(pub String);

impl RuleId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Canonical identity of a structured web resource, decoupling platform from content.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CanonicalResource {
    /// Exact YouTube video ID (e.g. `dQw4w9WgXcQ`), regardless of URL format.
    YouTubeVideo { video_id: String },
    /// GitHub repository (e.g. `owner/repo`).
    GitHubRepo { owner: String, repo: String },
    /// LeetCode problem slug (e.g. `trapping-rain-water`).
    LeetCodeProblem { slug: String },
    /// Generic URI resource.
    GenericResource { scheme: String, identifier: String },
}

/// The 7-level specificity weight hierarchy defined in Section 4.2 of the Master Specification.
/// Higher numeric values unconditionally override lower numeric values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RuleScope {
    GlobalDefault = 0,
    WorkflowDefault = 100,
    Application = 200,
    Domain = 400,
    Origin = 600,
    ExactUrl = 800,
    ExactResource = 1000,
}

/// Pattern or identifier targeted by a workflow rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleMatcher {
    /// Matches normalized executable process name (e.g. "code.exe", "windowsterminal.exe").
    ApplicationName(String),
    /// Matches web host pattern (e.g. "leetcode.com" or "*.github.com").
    DomainPattern(String),
    /// Matches exact URL path with query/fragments stripped.
    ExactUrl(String),
    /// Matches structured canonical resource (e.g. specific YouTube video ID).
    Canonical(CanonicalResource),
    /// Catch-all fallback matcher.
    Any,
}

/// Policy effect produced when a rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleEffect {
    /// Declared aligned with intention. No intervention or grace period.
    Allow,
    /// Permitted temporarily with an ambient non-blocking reminder.
    Warn,
    /// Violates intention. Begins grace countdown and progressive escalation.
    Restrict,
    /// Transitory shell interaction (lock screen, Alt-Tab); ignored for policy evaluation.
    Ignore,
    /// Passively monitored without intervention regardless of dwell duration.
    Monitor,
}

/// Record of a drift away from productive applications and its eventual resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftEvent {
    pub session_id: SessionId,
    pub target_name: String,
    pub duration_seconds: u32,
    pub resolution: super::timer::DriftResolution,
    pub timestamp: DateTime<Utc>,
}

/// Individual policy rule within a workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRule {
    pub id: RuleId,
    pub workflow_id: WorkflowId,
    pub description: String,
    pub scope: RuleScope,
    pub matcher: RuleMatcher,
    pub effect: RuleEffect,
    pub priority_weight: u32,
    pub custom_grace_seconds: Option<u32>,
    pub is_enabled: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

impl WorkflowRule {
    /// Checks whether this rule is currently active and unexpired.
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        if !self.is_enabled {
            return false;
        }
        if let Some(exp) = self.expires_at {
            if now >= exp {
                return false;
            }
        }
        true
    }
}

/// The contractual commitment level chosen by the user when running a workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitmentContract {
    /// Flexible session; can be paused or cancelled at any time.
    Standard,
    /// Can only be ended early by typing an explicit written reason for reflection.
    AccountableBreakable { min_reason_characters: u32 },
    /// The Ulysses Contract: cannot be ended, paused, or cancelled early under any circumstance.
    Unbreakable { target_duration_seconds: u64 },
}

/// Complete workflow definition containing intention, rules, and commitment settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    pub id: WorkflowId,
    pub name: String,
    pub description: String,
    pub target_duration_minutes: Option<u32>,
    pub default_grace_period_seconds: u32,
    pub commitment_contract: CommitmentContract,
    pub rules: Vec<WorkflowRule>,
    pub default_effect: RuleEffect,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Workflow {
    /// Creates a new workflow with default configuration.
    pub fn new(id: WorkflowId, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id,
            name: name.into(),
            description: String::new(),
            target_duration_minutes: None,
            default_grace_period_seconds: 30, // Authoritative 30-second default
            commitment_contract: CommitmentContract::Standard,
            rules: Vec::new(),
            default_effect: RuleEffect::Restrict,
            created_at: now,
            updated_at: now,
        }
    }

    /// Adds a rule to this workflow.
    pub fn with_rule(mut self, rule: WorkflowRule) -> Self {
        self.rules.push(rule);
        self
    }
}
