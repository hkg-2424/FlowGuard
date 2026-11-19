use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::workflow::{CommitmentContract, PolicyDecision, Workflow, WorkflowSession};
use crate::domain::ActivityObservation;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullBackNotification {
    pub target: String,
    pub recovery_seconds: u32,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveSessionSnapshot {
    pub session_id: Option<String>,
    pub workflow_id: Option<String>,
    pub workflow_name: Option<String>,
    pub state: String,
    pub contract_type: String,
    pub min_reason_characters: u32,
    pub target_duration_minutes: Option<u32>,
    pub target_duration_seconds: Option<u64>,
    pub elapsed_total_seconds: u64,
    pub remaining_seconds: Option<u64>,
    pub focus_seconds: u64,
    pub grace_seconds: u64,
    pub distraction_seconds: u64,
    pub afk_seconds: u64,
    pub paused_seconds: u64,
    pub pull_back_count: u32,
    pub intervention_count: u32,
    pub quality_score: f64,
    pub policy_status: String,
    pub grace_remaining_seconds: u32,
    pub grace_total_seconds: u32,
    pub violation_target: Option<String>,
    pub current_app: Option<String>,
    pub current_title: Option<String>,
    pub current_domain: Option<String>,
    pub idle_state: String,
    pub source_type: String,
    pub source_connected: bool,
    pub last_pull_back: Option<PullBackNotification>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub target_duration_minutes: Option<u32>,
    pub contract_type: String,
    pub min_reason_characters: u32,
    pub default_grace_seconds: u32,
    pub allowed_apps: Vec<String>,
    pub allowed_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkflowRequest {
    pub name: String,
    pub description: Option<String>,
    pub duration_minutes: Option<u32>,
    pub contract_type: String,
    pub min_reason_characters: Option<u32>,
    pub allowed_apps: Vec<String>,
    pub allowed_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartSessionRequest {
    pub workflow_id: String,
    pub duration_minutes: Option<u32>,
    pub contract_type: Option<String>,
    pub min_reason_characters: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopSessionRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistoryDto {
    pub id: String,
    pub workflow_id: String,
    pub workflow_name: String,
    pub state: String,
    pub contract_type: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub early_exit_reason: Option<String>,
    pub total_focus_seconds: u64,
    pub total_grace_seconds: u64,
    pub total_distraction_seconds: u64,
    pub total_paused_seconds: u64,
    pub total_afk_seconds: u64,
    pub pull_back_count: u32,
    pub intervention_count: u32,
    pub quality_score: f64,
    pub drifts: Vec<DriftEventDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftEventDto {
    pub target_name: String,
    pub duration_seconds: u32,
    pub resolution: String,
    pub timestamp: DateTime<Utc>,
}

pub struct GuiState {
    pub active_workflow: Option<Workflow>,
    pub active_session: Option<WorkflowSession>,
    pub last_observation: Option<ActivityObservation>,
    pub last_decision: Option<PolicyDecision>,
    pub last_pull_back: Option<PullBackNotification>,
    pub source_name: String,
    pub source_connected: bool,
}

impl GuiState {
    pub fn new(source_name: String) -> Self {
        Self {
            active_workflow: None,
            active_session: None,
            last_observation: None,
            last_decision: None,
            last_pull_back: None,
            source_name,
            source_connected: true,
        }
    }

    pub fn snapshot(&self) -> LiveSessionSnapshot {
        let (current_app, current_title, current_domain, idle_str) = match &self.last_observation {
            Some(obs) => {
                let domain = obs.browser.as_ref().and_then(|b| b.url.clone());
                (
                    obs.application.clone(),
                    obs.window_title.clone(),
                    domain,
                    obs.idle_state.as_str().to_string(),
                )
            }
            None => (None, None, None, "Unknown".to_string()),
        };

        let (policy_status, grace_remaining, grace_total, violation_target) =
            match &self.last_decision {
                Some(PolicyDecision::Allowed { .. }) => ("Allowed".to_string(), 0, 30, None),
                Some(PolicyDecision::GracePeriod {
                    remaining_seconds,
                    total_grace_seconds,
                    target_name,
                    ..
                }) => (
                    "GracePeriod".to_string(),
                    *remaining_seconds,
                    *total_grace_seconds,
                    Some(target_name.clone()),
                ),
                Some(PolicyDecision::InterventionRequired { target_name, .. }) => (
                    "InterventionRequired".to_string(),
                    0,
                    30,
                    Some(target_name.clone()),
                ),
                Some(PolicyDecision::Ignored { .. }) => ("Ignored".to_string(), 0, 30, None),
                _ => ("None".to_string(), 0, 30, None),
            };

        if let Some(session) = &self.active_session {
            let wf_name = self
                .active_workflow
                .as_ref()
                .map(|w| w.name.clone())
                .unwrap_or_else(|| session.workflow_id.0.clone());

            let (contract_str, min_chars) = match &session.commitment_contract {
                CommitmentContract::Standard => ("Standard".to_string(), 0),
                CommitmentContract::AccountableBreakable {
                    min_reason_characters,
                } => ("AccountableBreakable".to_string(), *min_reason_characters),
                CommitmentContract::Unbreakable { .. } => ("Unbreakable".to_string(), 0),
            };

            let target_duration_mins = self
                .active_workflow
                .as_ref()
                .and_then(|w| w.target_duration_minutes);

            let target_duration_secs = target_duration_mins.map(|m| (m as u64) * 60);

            let total_tracked = session.total_focus_seconds
                + session.total_grace_seconds
                + session.total_distraction_seconds;

            let quality_score = if total_tracked > 0 {
                (session.total_focus_seconds as f64 / total_tracked as f64) * 100.0
            } else {
                100.0
            };

            let elapsed_total = session.total_focus_seconds
                + session.total_grace_seconds
                + session.total_distraction_seconds
                + session.total_paused_seconds;

            let remaining_secs =
                target_duration_secs.map(|target| target.saturating_sub(elapsed_total));

            LiveSessionSnapshot {
                session_id: Some(session.id.0.clone()),
                workflow_id: Some(session.workflow_id.0.clone()),
                workflow_name: Some(wf_name),
                state: format!("{:?}", session.state),
                contract_type: contract_str,
                min_reason_characters: min_chars,
                target_duration_minutes: target_duration_mins,
                target_duration_seconds: target_duration_secs,
                elapsed_total_seconds: elapsed_total,
                remaining_seconds: remaining_secs,
                focus_seconds: session.total_focus_seconds,
                grace_seconds: session.total_grace_seconds,
                distraction_seconds: session.total_distraction_seconds,
                afk_seconds: session.total_afk_seconds,
                paused_seconds: session.total_paused_seconds,
                pull_back_count: session.pull_back_count,
                intervention_count: session.intervention_count,
                quality_score,
                policy_status,
                grace_remaining_seconds: grace_remaining,
                grace_total_seconds: grace_total,
                violation_target,
                current_app,
                current_title,
                current_domain,
                idle_state: idle_str,
                source_type: self.source_name.clone(),
                source_connected: self.source_connected,
                last_pull_back: self.last_pull_back.clone(),
            }
        } else {
            LiveSessionSnapshot {
                session_id: None,
                workflow_id: None,
                workflow_name: None,
                state: "None".to_string(),
                contract_type: "Standard".to_string(),
                min_reason_characters: 0,
                target_duration_minutes: None,
                target_duration_seconds: None,
                elapsed_total_seconds: 0,
                remaining_seconds: None,
                focus_seconds: 0,
                grace_seconds: 0,
                distraction_seconds: 0,
                afk_seconds: 0,
                paused_seconds: 0,
                pull_back_count: 0,
                intervention_count: 0,
                quality_score: 100.0,
                policy_status: "None".to_string(),
                grace_remaining_seconds: 0,
                grace_total_seconds: 30,
                violation_target: None,
                current_app,
                current_title,
                current_domain,
                idle_state: idle_str,
                source_type: self.source_name.clone(),
                source_connected: self.source_connected,
                last_pull_back: None,
            }
        }
    }
}
