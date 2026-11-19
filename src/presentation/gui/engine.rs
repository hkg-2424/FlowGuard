use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::application::ports::WorkflowRepository;
use crate::application::workflow_service::{WorkflowService, WorkflowServiceError};
use crate::domain::workflow::{SessionState, WorkflowId, WorkflowSession};

use super::state::{GuiState, PullBackNotification};

pub enum GuiCommand {
    StartSession {
        workflow_id: WorkflowId,
        reply: Sender<Result<WorkflowSession, String>>,
    },
    PauseSession {
        reply: Sender<Result<(), String>>,
    },
    ResumeSession {
        reply: Sender<Result<(), String>>,
    },
    StopSession {
        reason: Option<String>,
        reply: Sender<Result<WorkflowSession, String>>,
    },
}

pub struct GuiEngine {
    service: WorkflowService,
    state: Arc<Mutex<GuiState>>,
    command_rx: Receiver<GuiCommand>,
    running: Arc<AtomicBool>,
}

impl GuiEngine {
    pub fn new(
        service: WorkflowService,
        state: Arc<Mutex<GuiState>>,
        command_rx: Receiver<GuiCommand>,
        running: Arc<AtomicBool>,
    ) -> Self {
        Self {
            service,
            state,
            command_rx,
            running,
        }
    }

    pub fn run(mut self, wf_repo: Box<dyn WorkflowRepository>) {
        info!("GuiEngine focus orchestration loop started.");

        let mut active_poll_counter: u32 = 0;
        let mut idle_poll_counter: u32 = 0;

        while self.running.load(Ordering::SeqCst) {
            // 1. Process any incoming commands non-blockingly
            while let Ok(cmd) = self.command_rx.try_recv() {
                match cmd {
                    GuiCommand::StartSession { workflow_id, reply } => {
                        // If session is already active for this workflow, return it cleanly
                        if let Some(existing) = self.service.active_session() {
                            if existing.workflow_id == workflow_id
                                && (existing.state == SessionState::Active
                                    || existing.state == SessionState::Paused)
                            {
                                let _ = reply.send(Ok(existing.clone()));
                                continue;
                            }
                        }

                        match self.service.start_session(&workflow_id) {
                            Ok(sess) => {
                                let wf = wf_repo.get(&workflow_id).ok().flatten();
                                {
                                    let mut st = self.state.lock().unwrap();
                                    st.active_workflow = wf;
                                    st.active_session = Some(sess.clone());
                                    st.last_pull_back = None;
                                }
                                active_poll_counter = 10; // Trigger evaluation immediately
                                let _ = reply.send(Ok(sess));
                            }
                            Err(e) => {
                                let _ = reply.send(Err(e.to_string()));
                            }
                        }
                    }
                    GuiCommand::PauseSession { reply } => match self.service.pause_session() {
                        Ok(()) => {
                            {
                                let mut st = self.state.lock().unwrap();
                                if let Some(ref mut s) = st.active_session {
                                    s.state = SessionState::Paused;
                                }
                            }
                            let _ = reply.send(Ok(()));
                        }
                        Err(e) => {
                            let _ = reply.send(Err(e.to_string()));
                        }
                    },
                    GuiCommand::ResumeSession { reply } => match self.service.resume_session() {
                        Ok(()) => {
                            {
                                let mut st = self.state.lock().unwrap();
                                if let Some(ref mut s) = st.active_session {
                                    s.state = SessionState::Active;
                                }
                            }
                            active_poll_counter = 10;
                            let _ = reply.send(Ok(()));
                        }
                        Err(e) => {
                            let _ = reply.send(Err(e.to_string()));
                        }
                    },
                    GuiCommand::StopSession { reason, reply } => {
                        match self.service.cancel_session(reason) {
                            Ok(sess) => {
                                {
                                    let mut st = self.state.lock().unwrap();
                                    st.active_session = None;
                                    st.active_workflow = None;
                                    st.last_decision = None;
                                }
                                active_poll_counter = 0;
                                let _ = reply.send(Ok(sess));
                            }
                            Err(e) => {
                                let _ = reply.send(Err(e.to_string()));
                            }
                        }
                    }
                }
            }

            // 2. Perform live tick evaluation if session is active (at 1-second intervals)
            if self.service.active_session().is_some() {
                active_poll_counter += 1;
                if active_poll_counter >= 10 {
                    active_poll_counter = 0;
                    match self.service.evaluate_tick() {
                        Ok(eval) => {
                            // Check if target duration reached for natural auto-completion
                            let elapsed = eval.session.duration_seconds(chrono::Utc::now());
                            let target_secs = self
                                .service
                                .active_workflow()
                                .and_then(|w| w.target_duration_minutes)
                                .map(|m| m as u64 * 60);

                            if let Some(target) = target_secs {
                                if elapsed >= target {
                                    info!(
                                        "Target focus duration of {}s reached! Completing session naturally.",
                                        target
                                    );
                                    if let Ok(completed_sess) = self.service.complete_session() {
                                        let mut st = self.state.lock().unwrap();
                                        st.last_observation = Some(eval.observation);
                                        st.active_session = None;
                                        st.active_workflow = None;
                                        st.last_decision = Some(crate::domain::workflow::PolicyDecision::Allowed {
                                            rule_id: crate::domain::workflow::RuleId::new("completed"),
                                            scope: crate::domain::workflow::RuleScope::WorkflowDefault,
                                        });
                                        let _ = completed_sess;
                                        continue;
                                    }
                                }
                            }

                            let mut st = self.state.lock().unwrap();
                            st.update_activity_feed(&eval.observation);
                            st.last_observation = Some(eval.observation);
                            st.last_decision = Some(eval.decision);
                            st.active_session = Some(eval.session);
                            st.source_connected = true;

                            if let Some(pb) = eval.pull_back_event {
                                st.last_pull_back = Some(PullBackNotification {
                                    target: pb.target_name,
                                    recovery_seconds: pb.duration_seconds,
                                    timestamp: pb.timestamp,
                                });
                            }
                        }
                        Err(WorkflowServiceError::Source(err)) => {
                            warn!("Source poll error in GUI session: {}", err);
                            let mut st = self.state.lock().unwrap();
                            st.source_connected = false;
                        }
                        Err(e) => {
                            error!("Evaluation error in GUI session: {}", e);
                        }
                    }
                }
            } else {
                active_poll_counter = 0;
                // Inactive session: periodically poll observation every ~1s (10 ticks x 100ms)
                idle_poll_counter += 1;
                if idle_poll_counter >= 10 {
                    idle_poll_counter = 0;
                    match self.service.poll_idle_observation() {
                        Ok(obs) => {
                            let mut st = self.state.lock().unwrap();
                            st.update_activity_feed(&obs);
                            st.last_observation = Some(obs);
                            st.source_connected = true;
                        }
                        Err(e) => {
                            debug!("Idle source poll error: {}", e);
                            let mut st = self.state.lock().unwrap();
                            st.source_connected = false;
                        }
                    }
                }
            }

            sleep(Duration::from_millis(100));
        }

        info!("GuiEngine focus orchestration loop stopped.");
    }
}
