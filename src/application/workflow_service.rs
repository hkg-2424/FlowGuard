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

#[derive(Debug, Clone)]
struct ActiveDriftEpisode {
    target_name: String,
    started_at: chrono::DateTime<Utc>,
    intervened: bool,
}

/// Generates a collision-resistant Session ID using monotonic microsecond timestamps,
/// process ID, an atomic counter, and pseudo-random hash entropy (H-10).
fn generate_session_id() -> SessionId {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let micros = Utc::now().timestamp_micros();
    let mut x = (micros as u64) ^ ((pid as u64) << 32) ^ (count << 16) ^ 0xa0761d6478bd642f;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    let r = x.wrapping_mul(0x2545F4914F6CDD1D);
    SessionId::new(format!(
        "sess_{}_{}_{:04x}_{:08x}",
        micros,
        pid,
        count % 0x10000,
        (r & 0xffffffff) as u32
    ))
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
    current_drift_episode: Option<ActiveDriftEpisode>,
    subsecond_focus_ms: u64,
    subsecond_grace_ms: u64,
    subsecond_distraction_ms: u64,
    subsecond_paused_ms: u64,
    subsecond_afk_ms: u64,
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
            current_drift_episode: None,
            subsecond_focus_ms: 0,
            subsecond_grace_ms: 0,
            subsecond_distraction_ms: 0,
            subsecond_paused_ms: 0,
            subsecond_afk_ms: 0,
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

        // Check storage for dangling active/paused sessions across instances (H-13)
        if let Ok(recent) = self.session_repo.list_recent(5) {
            for s in recent {
                if s.state == SessionState::Active || s.state == SessionState::Paused {
                    return Err(WorkflowServiceError::ActiveSessionAlreadyExists(s.id));
                }
            }
        }

        let workflow = self
            .workflow_repo
            .get(workflow_id)?
            .ok_or_else(|| WorkflowServiceError::WorkflowNotFound(workflow_id.clone()))?;

        // Generate collision-resistant monotonic+random session ID (H-10)
        let session_id = generate_session_id();
        let mut session = WorkflowSession::new(
            session_id,
            workflow.id.clone(),
            workflow.commitment_contract.clone(),
        );

        session.start()?;

        // Persist to storage FIRST before mutating in-memory service state (H-08, H-09)
        self.session_repo.save(&session)?;

        self.timer.reset();
        self.in_active_intervention = false;
        self.current_drift_episode = None;
        self.last_tick_time = Some(std::time::Instant::now());
        self.subsecond_focus_ms = 0;
        self.subsecond_grace_ms = 0;
        self.subsecond_distraction_ms = 0;
        self.subsecond_paused_ms = 0;
        self.subsecond_afk_ms = 0;

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
        let (session_id, mut candidate) = {
            let session = self
                .active_session
                .as_ref()
                .ok_or(WorkflowServiceError::NoActiveSession)?;
            (session.id.clone(), session.clone())
        };

        // Apply transition to a clone and persist first (H-09)
        candidate.pause()?;
        self.session_repo.save(&candidate)?;

        // Atomically update in-memory state upon persistence success
        self.active_session = Some(candidate);
        self.timer.pause();
        self.last_tick_time = None;
        info!("Paused workflow session '{}'", session_id);
        Ok(())
    }

    /// Resumes the paused session.
    pub fn resume_session(&mut self) -> Result<(), WorkflowServiceError> {
        let (session_id, mut candidate) = {
            let session = self
                .active_session
                .as_ref()
                .ok_or(WorkflowServiceError::NoActiveSession)?;
            (session.id.clone(), session.clone())
        };

        // Apply transition to a clone and persist first (H-09)
        candidate.resume()?;
        self.session_repo.save(&candidate)?;

        // Atomically update in-memory state upon persistence success
        self.active_session = Some(candidate);
        self.last_tick_time = Some(std::time::Instant::now());
        info!("Resumed workflow session '{}'", session_id);
        Ok(())
    }

    /// Attempts to cancel the active session early, enforcing commitment rules.
    pub fn cancel_session(
        &mut self,
        reason: Option<String>,
    ) -> Result<WorkflowSession, WorkflowServiceError> {
        let session = self
            .active_session
            .as_ref()
            .ok_or(WorkflowServiceError::NoActiveSession)?;

        // Apply transition to a clone and persist first (H-09)
        let mut candidate = session.clone();
        candidate
            .cancel(reason)
            .map_err(WorkflowServiceError::Session)?;

        // Seal any active drift episode before session closure (H-30)
        if let Some(ep) = self.current_drift_episode.take() {
            let now = Utc::now();
            let duration_seconds = now
                .signed_duration_since(ep.started_at)
                .num_seconds()
                .max(0) as u32;
            let resolution = if ep.intervened {
                DriftResolution::Intervened
            } else {
                DriftResolution::SelfCorrected {
                    recovery_seconds: duration_seconds,
                }
            };
            let event = DriftEvent {
                session_id: candidate.id.clone(),
                target_name: ep.target_name,
                duration_seconds,
                resolution,
                timestamp: now,
            };
            let _ = self.session_repo.record_drift(&event);
        }

        self.session_repo.save(&candidate)?;

        // Atomically clear active session from memory upon persistence success
        self.timer.reset();
        self.in_active_intervention = false;
        self.current_drift_episode = None;
        self.last_tick_time = None;
        self.active_session = None;
        self.active_workflow = None;

        info!("Cancelled workflow session '{}'", candidate.id);
        Ok(candidate)
    }

    /// Completes the active session naturally upon goal achievement.
    pub fn complete_session(&mut self) -> Result<WorkflowSession, WorkflowServiceError> {
        let session = self
            .active_session
            .as_ref()
            .ok_or(WorkflowServiceError::NoActiveSession)?;

        // Apply transition to a clone and persist first (H-09)
        let mut candidate = session.clone();
        candidate
            .complete()
            .map_err(WorkflowServiceError::Session)?;

        // Seal any active drift episode before session closure (H-30)
        if let Some(ep) = self.current_drift_episode.take() {
            let now = Utc::now();
            let duration_seconds = now
                .signed_duration_since(ep.started_at)
                .num_seconds()
                .max(0) as u32;
            let resolution = if ep.intervened {
                DriftResolution::Intervened
            } else {
                DriftResolution::SelfCorrected {
                    recovery_seconds: duration_seconds,
                }
            };
            let event = DriftEvent {
                session_id: candidate.id.clone(),
                target_name: ep.target_name,
                duration_seconds,
                resolution,
                timestamp: now,
            };
            let _ = self.session_repo.record_drift(&event);
        }

        self.session_repo.save(&candidate)?;

        // Atomically clear active session from memory upon persistence success
        self.timer.reset();
        self.in_active_intervention = false;
        self.current_drift_episode = None;
        self.last_tick_time = None;
        self.active_session = None;
        self.active_workflow = None;

        info!("Completed workflow session '{}'", candidate.id);
        Ok(candidate)
    }

    /// Evaluates one immediate activity observation against the active workflow rules,
    /// updating session temporal accounting and logging focus pull-backs.
    pub fn evaluate_tick(&mut self) -> Result<SessionEvaluation, WorkflowServiceError> {
        let session = self
            .active_session
            .as_ref()
            .ok_or(WorkflowServiceError::NoActiveSession)?;

        let workflow = self
            .active_workflow
            .as_ref()
            .ok_or(WorkflowServiceError::NoActiveSession)?;

        // If session is paused, skip rule evaluation and accumulate paused time accurately (H-09, H-29)
        if session.state == SessionState::Paused {
            let obs = self.source.poll()?;
            let now_instant = std::time::Instant::now();
            let elapsed_ms = if let Some(last) = self.last_tick_time {
                let ms = now_instant.saturating_duration_since(last).as_millis() as u64;
                if ms == 0 {
                    self.poll_interval.as_millis() as u64
                } else {
                    ms.min(10_000)
                }
            } else {
                self.poll_interval.as_millis() as u64
            };
            self.last_tick_time = Some(now_instant);

            let mut candidate = session.clone();
            self.subsecond_paused_ms += elapsed_ms;
            let add_secs = self.subsecond_paused_ms / 1000;
            self.subsecond_paused_ms %= 1000;
            candidate.total_paused_seconds += add_secs;

            if add_secs > 0 {
                self.session_repo.save(&candidate)?;
            }
            self.active_session = Some(candidate.clone());

            return Ok(SessionEvaluation {
                observation: obs,
                decision: PolicyDecision::Ignored {
                    reason: "Session is paused".to_string(),
                },
                session: candidate,
                pull_back_event: None,
            });
        }

        let obs = self.source.poll()?;
        let now = Utc::now();
        let violation_target = self.timer.active_target().map(|s| s.to_string());
        let was_violating = self.timer.is_active();

        let decision = self.engine.evaluate(&obs, workflow, &mut self.timer, now);

        let mut candidate = session.clone();
        let mut pull_back_event: Option<DriftEvent> = None;

        // Track drift episodes with per-episode duration and seal at intervention (H-30)
        match &decision {
            PolicyDecision::GracePeriod { target_name, .. } => {
                if self.current_drift_episode.is_none() {
                    self.current_drift_episode = Some(ActiveDriftEpisode {
                        target_name: target_name.clone(),
                        started_at: now,
                        intervened: false,
                    });
                }
            }
            PolicyDecision::InterventionRequired { target_name, .. } => {
                if let Some(ref mut ep) = self.current_drift_episode {
                    if !ep.intervened {
                        ep.intervened = true;
                        let duration_secs = now
                            .signed_duration_since(ep.started_at)
                            .num_seconds()
                            .max(1) as u32;
                        let event = DriftEvent {
                            session_id: candidate.id.clone(),
                            target_name: ep.target_name.clone(),
                            duration_seconds: duration_secs,
                            resolution: DriftResolution::Intervened,
                            timestamp: now,
                        };
                        self.session_repo.record_drift(&event)?;
                    }
                } else {
                    let ep = ActiveDriftEpisode {
                        target_name: target_name.clone(),
                        started_at: now,
                        intervened: true,
                    };
                    let event = DriftEvent {
                        session_id: candidate.id.clone(),
                        target_name: ep.target_name.clone(),
                        duration_seconds: 1,
                        resolution: DriftResolution::Intervened,
                        timestamp: now,
                    };
                    self.session_repo.record_drift(&event)?;
                    self.current_drift_episode = Some(ep);
                }
            }
            _ => {}
        }

        // Check if user just self-corrected from a grace violation back to an allowed context!
        if was_violating && decision.is_allowed() {
            if let Some(res) = self.timer.take_last_resolution() {
                let target_name = violation_target
                    .unwrap_or_else(|| obs.application.clone().unwrap_or_else(|| "unknown".into()));
                match res {
                    DriftResolution::SelfCorrected { recovery_seconds } => {
                        candidate.pull_back_count += 1;
                        let event = DriftEvent {
                            session_id: candidate.id.clone(),
                            target_name,
                            duration_seconds: recovery_seconds,
                            resolution: DriftResolution::SelfCorrected { recovery_seconds },
                            timestamp: now,
                        };
                        // Atomic evaluation persistence: don't discard drift errors (H-08)
                        self.session_repo.record_drift(&event)?;
                        pull_back_event = Some(event);
                        self.current_drift_episode = None;
                    }
                    DriftResolution::Intervened => {
                        let already_recorded = self
                            .current_drift_episode
                            .as_ref()
                            .map(|e| e.intervened)
                            .unwrap_or(false);
                        if !already_recorded {
                            let duration_seconds = (candidate.total_grace_seconds
                                + candidate.total_distraction_seconds)
                                .max(30) as u32;
                            let event = DriftEvent {
                                session_id: candidate.id.clone(),
                                target_name,
                                duration_seconds,
                                resolution: DriftResolution::Intervened,
                                timestamp: now,
                            };
                            self.session_repo.record_drift(&event)?;
                        }
                        self.current_drift_episode = None;
                    }
                }
            } else if let Some(ep) = self.current_drift_episode.take() {
                if !ep.intervened {
                    let recovery_seconds = now
                        .signed_duration_since(ep.started_at)
                        .num_seconds()
                        .max(1) as u32;
                    candidate.pull_back_count += 1;
                    let event = DriftEvent {
                        session_id: candidate.id.clone(),
                        target_name: ep.target_name,
                        duration_seconds: recovery_seconds,
                        resolution: DriftResolution::SelfCorrected { recovery_seconds },
                        timestamp: now,
                    };
                    self.session_repo.record_drift(&event)?;
                    pull_back_event = Some(event);
                }
            }
        }

        if decision.is_allowed() {
            self.in_active_intervention = false;
        }

        // Fractional monotonic temporal accounting (H-29)
        let now_instant = std::time::Instant::now();
        let elapsed_ms = if let Some(last) = self.last_tick_time {
            let ms = now_instant.saturating_duration_since(last).as_millis() as u64;
            if ms == 0 {
                self.poll_interval.as_millis() as u64
            } else {
                ms.min(10_000)
            }
        } else {
            self.poll_interval.as_millis() as u64
        };

        match &decision {
            PolicyDecision::Allowed { .. } => {
                self.subsecond_focus_ms += elapsed_ms;
                candidate.total_focus_seconds += self.subsecond_focus_ms / 1000;
                self.subsecond_focus_ms %= 1000;
            }
            PolicyDecision::GracePeriod { .. } => {
                self.subsecond_grace_ms += elapsed_ms;
                candidate.total_grace_seconds += self.subsecond_grace_ms / 1000;
                self.subsecond_grace_ms %= 1000;
            }
            PolicyDecision::InterventionRequired { .. } => {
                self.subsecond_distraction_ms += elapsed_ms;
                candidate.total_distraction_seconds += self.subsecond_distraction_ms / 1000;
                self.subsecond_distraction_ms %= 1000;
                if !self.in_active_intervention {
                    candidate.intervention_count += 1;
                    self.in_active_intervention = true;
                }
            }
            PolicyDecision::Ignored { reason } if reason.contains("AFK") => {
                self.subsecond_afk_ms += elapsed_ms;
                candidate.total_afk_seconds += self.subsecond_afk_ms / 1000;
                self.subsecond_afk_ms %= 1000;
            }
            _ => {}
        }

        // Atomic evaluation persistence: persist before updating in-memory state (H-08, H-09)
        self.session_repo.save(&candidate)?;

        // Only after save succeeds, commit candidate to active in-memory state
        self.last_tick_time = Some(now_instant);
        self.active_session = Some(candidate.clone());

        Ok(SessionEvaluation {
            observation: obs,
            decision,
            session: candidate,
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

    /// Returns a reference to the active workflow, if one is active.
    pub fn active_workflow(&self) -> Option<&Workflow> {
        self.active_workflow.as_ref()
    }

    /// Returns the configured poll interval duration.
    pub fn poll_interval(&self) -> Duration {
        self.poll_interval
    }

    /// Polls an activity observation directly from the underlying source when no session is active.
    pub fn poll_idle_observation(&mut self) -> Result<ActivityObservation, SourceError> {
        self.source.poll()
    }
}
