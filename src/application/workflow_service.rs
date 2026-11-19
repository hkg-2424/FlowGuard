use chrono::Utc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use thiserror::Error;
use tracing::{info, warn};

use super::ports::{
    ActivitySource, RepositoryError, SessionRepository, SourceError, WorkflowRepository,
};
use crate::domain::workflow::{
    DriftEvent, DriftResolution, GracePeriodTimer, PolicyDecision, RuleEngine, SessionError,
    SessionId, SessionState, Workflow, WorkflowId, WorkflowSession,
};
use crate::domain::ActivityObservation;

#[derive(Debug, Error)]
pub enum WorkflowServiceError {
    #[error("Source monitoring error: {0}")]
    Source(#[from] SourceError),

    #[error("Repository storage error: {0}")]
    Repository(#[from] RepositoryError),

    #[error("Session state error: {0}")]
    Session(#[from] SessionError),

    #[error("Workflow not found: {0}")]
    WorkflowNotFound(WorkflowId),

    #[error("An active session is already in progress: {0}")]
    ActiveSessionAlreadyExists(SessionId),

    #[error("No workflow session is currently active")]
    NoActiveSession,
}

/// Snapshot evaluation result produced on each workflow poll tick.
#[derive(Debug, Clone)]
pub struct SessionEvaluation {
    pub observation: ActivityObservation,
    pub decision: PolicyDecision,
    pub session: WorkflowSession,
    pub pull_back_event: Option<DriftEvent>,
}

/// Core application orchestrator managing workflow lifecycle, live activity
/// observation polling, rule evaluation, and temporal accounting.
pub struct WorkflowService {
    source: Box<dyn ActivitySource>,
    workflow_repo: Box<dyn WorkflowRepository>,
    session_repo: Box<dyn SessionRepository>,
    engine: RuleEngine,
    timer: GracePeriodTimer,
    active_session: Option<WorkflowSession>,
    active_workflow: Option<Workflow>,
    poll_interval: Duration,
    in_active_intervention: bool,
    last_tick_time: Option<std::time::Instant>,
}

impl WorkflowService {
    pub fn new(
        source: Box<dyn ActivitySource>,
        workflow_repo: Box<dyn WorkflowRepository>,
        session_repo: Box<dyn SessionRepository>,
        poll_interval_ms: u64,
    ) -> Self {
        Self {
            source,
            workflow_repo,
            session_repo,
            engine: RuleEngine::new(),
            timer: GracePeriodTimer::new(),
            active_session: None,
            active_workflow: None,
            poll_interval: Duration::from_millis(poll_interval_ms.max(500)),
            in_active_intervention: false,
            last_tick_time: None,
        }
    }

    /// Starts a new workflow session for the specified workflow ID.
    pub fn start_session(
        &mut self,
        workflow_id: &WorkflowId,
    ) -> Result<WorkflowSession, WorkflowServiceError> {
        if let Some(existing) = &self.active_session {
            if existing.state == SessionState::Active || existing.state == SessionState::Paused {
                return Err(WorkflowServiceError::ActiveSessionAlreadyExists(
                    existing.id.clone(),
                ));
            }
        }

        let workflow = self
            .workflow_repo
            .get(workflow_id)?
            .ok_or_else(|| WorkflowServiceError::WorkflowNotFound(workflow_id.clone()))?;

        let session_id = SessionId::new(format!("sess_{}", Utc::now().timestamp_millis()));
        let mut session = WorkflowSession::new(
            session_id,
            workflow.id.clone(),
            workflow.commitment_contract.clone(),
        );

        session.start()?;
        self.timer.reset();
        self.in_active_intervention = false;
        self.last_tick_time = Some(std::time::Instant::now());
        self.session_repo.save(&session)?;

        self.active_workflow = Some(workflow);
        self.active_session = Some(session.clone());

        info!(
            "Started workflow session '{}' for workflow '{}'",
            session.id, workflow_id
        );

        Ok(session)
    }

    /// Pauses the active session.
    pub fn pause_session(&mut self) -> Result<(), WorkflowServiceError> {
        let session = self
            .active_session
            .as_mut()
            .ok_or(WorkflowServiceError::NoActiveSession)?;

        session.pause()?;
        self.timer.pause();
        self.session_repo.save(session)?;
        info!("Paused workflow session '{}'", session.id);
        Ok(())
    }

    /// Resumes the paused session.
    pub fn resume_session(&mut self) -> Result<(), WorkflowServiceError> {
        let session = self
            .active_session
            .as_mut()
            .ok_or(WorkflowServiceError::NoActiveSession)?;

        session.resume()?;
        self.session_repo.save(session)?;
        info!("Resumed workflow session '{}'", session.id);
        Ok(())
    }

    /// Attempts to cancel the active session early, enforcing commitment rules.
    pub fn cancel_session(
        &mut self,
        reason: Option<String>,
    ) -> Result<WorkflowSession, WorkflowServiceError> {
        let mut session = self
            .active_session
            .take()
            .ok_or(WorkflowServiceError::NoActiveSession)?;

        if let Err(e) = session.cancel(reason) {
            // Restore session if cancellation rejected
            self.active_session = Some(session);
            return Err(WorkflowServiceError::Session(e));
        }

        self.timer.reset();
        self.in_active_intervention = false;
        self.last_tick_time = None;
        self.session_repo.save(&session)?;
        self.active_workflow = None;

        info!("Cancelled workflow session '{}'", session.id);
        Ok(session)
    }

    /// Completes the active session naturally upon goal achievement.
    pub fn complete_session(&mut self) -> Result<WorkflowSession, WorkflowServiceError> {
        let mut session = self
            .active_session
            .take()
            .ok_or(WorkflowServiceError::NoActiveSession)?;

        session.complete()?;
        self.timer.reset();
        self.in_active_intervention = false;
        self.last_tick_time = None;
        self.session_repo.save(&session)?;
        self.active_workflow = None;

        info!("Completed workflow session '{}'", session.id);
        Ok(session)
    }

    /// Evaluates one immediate activity observation against the active workflow rules,
    /// updating session temporal accounting and logging focus pull-backs.
    pub fn evaluate_tick(&mut self) -> Result<SessionEvaluation, WorkflowServiceError> {
        let session = self
            .active_session
            .as_mut()
            .ok_or(WorkflowServiceError::NoActiveSession)?;

        let workflow = self
            .active_workflow
            .as_ref()
            .ok_or(WorkflowServiceError::NoActiveSession)?;

        // If session is paused, skip rule evaluation
        if session.state == SessionState::Paused {
            let obs = self.source.poll()?;
            session.total_paused_seconds += (self.poll_interval.as_millis() / 1000).max(1) as u64;
            return Ok(SessionEvaluation {
                observation: obs,
                decision: PolicyDecision::Ignored {
                    reason: "Session is paused".to_string(),
                },
                session: session.clone(),
                pull_back_event: None,
            });
        }

        let obs = self.source.poll()?;
        let now = Utc::now();
        let violation_target = self.timer.active_target().map(|s| s.to_string());
        let was_violating = self.timer.is_active();

        let decision = self.engine.evaluate(&obs, workflow, &mut self.timer, now);

        // Check if user just self-corrected from a grace violation back to an allowed context!
        let mut pull_back_event: Option<DriftEvent> = None;
        if was_violating && decision.is_allowed() {
            if let Some(DriftResolution::SelfCorrected { recovery_seconds }) =
                self.timer.take_last_resolution()
            {
                session.pull_back_count += 1;
                let target_name = violation_target
                    .unwrap_or_else(|| obs.application.clone().unwrap_or_else(|| "unknown".into()));
                let event = DriftEvent {
                    session_id: session.id.clone(),
                    target_name,
                    duration_seconds: recovery_seconds,
                    resolution: DriftResolution::SelfCorrected { recovery_seconds },
                    timestamp: now,
                };
                let _ = self.session_repo.record_drift(&event);
                pull_back_event = Some(event);
            }
        }

        if decision.is_allowed() {
            self.in_active_intervention = false;
        }

        // Temporal accounting step
        let now_instant = std::time::Instant::now();
        let tick_secs = if let Some(last) = self.last_tick_time {
            let elapsed = now_instant.saturating_duration_since(last).as_secs();
            if elapsed == 0 {
                (self.poll_interval.as_millis() / 1000).max(1) as u64
            } else {
                elapsed
            }
        } else {
            (self.poll_interval.as_millis() / 1000).max(1) as u64
        };
        self.last_tick_time = Some(now_instant);

        match &decision {
            PolicyDecision::Allowed { .. } => {
                session.total_focus_seconds += tick_secs;
            }
            PolicyDecision::GracePeriod { .. } => {
                session.total_grace_seconds += tick_secs;
            }
            PolicyDecision::InterventionRequired { .. } => {
                session.total_distraction_seconds += tick_secs;
                if !self.in_active_intervention {
                    session.intervention_count += 1;
                    self.in_active_intervention = true;
                }
            }
            PolicyDecision::Ignored { reason } if reason.contains("AFK") => {
                session.total_afk_seconds += tick_secs;
            }
            _ => {}
        }

        let _ = self.session_repo.save(session);

        Ok(SessionEvaluation {
            observation: obs,
            decision,
            session: session.clone(),
            pull_back_event,
        })
    }

    /// Runs the continuous session monitoring loop until `running` is set to false.
    pub fn run_session_loop<F>(
        &mut self,
        running: Arc<AtomicBool>,
        mut on_tick: F,
    ) -> Result<(), WorkflowServiceError>
    where
        F: FnMut(&SessionEvaluation),
    {
        info!("Entering continuous workflow session loop...");
        let slice = Duration::from_millis(50);

        while running.load(Ordering::SeqCst) {
            match self.evaluate_tick() {
                Ok(eval) => {
                    on_tick(&eval);
                }
                Err(WorkflowServiceError::Source(err)) => {
                    warn!("Source poll error during session: {}", err);
                }
                Err(err) => return Err(err),
            }

            // Sleep in interruptible 50ms slices
            let mut remaining = self.poll_interval;
            while remaining > Duration::ZERO && running.load(Ordering::SeqCst) {
                let step = slice.min(remaining);
                sleep(step);
                remaining = remaining.saturating_sub(step);
            }
        }

        info!("Exited workflow session loop.");
        Ok(())
    }

    /// Returns a reference to the active session, if one exists.
    pub fn active_session(&self) -> Option<&WorkflowSession> {
        self.active_session.as_ref()
    }
}
