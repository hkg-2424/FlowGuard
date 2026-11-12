use chrono::Utc;
use std::sync::{Arc, Mutex};

use flowguard::application::ports::{ActivitySource, SourceError, WorkflowRepository};
use flowguard::application::workflow_service::{WorkflowService, WorkflowServiceError};
use flowguard::domain::activity::{ActivityObservation, BrowserContext, FreshnessState, IdleState};
use flowguard::domain::workflow::{
    CommitmentContract, DriftResolution, PolicyDecision, RuleEffect, RuleId, RuleMatcher,
    RuleScope, SessionError, SessionState, Workflow, WorkflowId, WorkflowRule,
};
use flowguard::domain::{ActivityCapabilities, DiagnosticReport, SourceCapabilities, SourceHealth};
use flowguard::infrastructure::persistence::{
    InMemorySessionRepository, InMemoryWorkflowRepository,
};

#[derive(Clone)]
struct MockSource {
    current_obs: Arc<Mutex<ActivityObservation>>,
}

impl MockSource {
    fn new(obs: ActivityObservation) -> Self {
        Self {
            current_obs: Arc::new(Mutex::new(obs)),
        }
    }

    fn set(&self, obs: ActivityObservation) {
        *self.current_obs.lock().unwrap() = obs;
    }
}

impl ActivitySource for MockSource {
    fn connect(&mut self) -> Result<SourceCapabilities, SourceError> {
        Ok(SourceCapabilities {
            provider: "mock".to_string(),
            server_version: Some("1.0.0".to_string()),
            hostname: Some("mock-host".to_string()),
            is_testing: true,
            capabilities: ActivityCapabilities {
                application_tracking: true,
                window_title_tracking: true,
                idle_tracking: true,
                browser_url_tracking: true,
                incognito_detection: true,
            },
        })
    }

    fn poll(&mut self) -> Result<ActivityObservation, SourceError> {
        Ok(self.current_obs.lock().unwrap().clone())
    }

    fn health(&self) -> SourceHealth {
        SourceHealth::Connected
    }

    fn diagnose(&mut self) -> Result<DiagnosticReport, SourceError> {
        Ok(DiagnosticReport::unreachable(
            "MockSource",
            "mock://",
            SourceHealth::Connected,
            String::new(),
        ))
    }
}

fn sample_obs(app: &str, title: &str, url: Option<&str>, idle: IdleState) -> ActivityObservation {
    ActivityObservation {
        observed_at: Utc::now(),
        application: Some(app.to_string()),
        window_title: Some(title.to_string()),
        browser: url.map(|u| BrowserContext {
            url: Some(u.to_string()),
            title: Some(title.to_string()),
            audible: Some(false),
            incognito: Some(false),
        }),
        idle_state: idle,
        capabilities: ActivityCapabilities {
            application_tracking: true,
            window_title_tracking: true,
            idle_tracking: true,
            browser_url_tracking: url.is_some(),
            incognito_detection: true,
        },
        source_health: SourceHealth::Connected,
        window_freshness: FreshnessState::Fresh,
        idle_freshness: FreshnessState::Fresh,
        browser_freshness: if url.is_some() {
            FreshnessState::Fresh
        } else {
            FreshnessState::Missing
        },
        diagnostics: Vec::new(),
    }
}

fn create_test_workflow(contract: CommitmentContract) -> Workflow {
    let wf_id = WorkflowId::new("wf-study");
    let mut workflow = Workflow::new(wf_id.clone(), "Focused Study")
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-code"),
            workflow_id: wf_id.clone(),
            description: "Allow VS Code".into(),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName("code.exe".into()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        })
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-docs"),
            workflow_id: wf_id.clone(),
            description: "Allow Rust Documentation".into(),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern("doc.rust-lang.org".into()),
            effect: RuleEffect::Allow,
            priority_weight: 20,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        })
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-block-social"),
            workflow_id: wf_id.clone(),
            description: "Block Social Media".into(),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern("reddit.com".into()),
            effect: RuleEffect::Restrict,
            priority_weight: 50,
            custom_grace_seconds: Some(30),
            is_enabled: true,
            expires_at: None,
        });

    workflow.commitment_contract = contract;
    workflow
}

#[test]
fn test_ws01_session_lifecycle() {
    let mock = MockSource::new(sample_obs("code.exe", "main.rs", None, IdleState::Active));
    let mut wf_repo = InMemoryWorkflowRepository::new();
    let session_repo = InMemorySessionRepository::new();

    let workflow = create_test_workflow(CommitmentContract::AccountableBreakable {
        min_reason_characters: 10,
    });
    let wf_id = workflow.id.clone();
    wf_repo.save(&workflow).unwrap();

    let mut service = WorkflowService::new(
        Box::new(mock),
        Box::new(wf_repo),
        Box::new(session_repo),
        1000,
    );

    // 1. Starting session succeeds
    let session = service
        .start_session(&wf_id)
        .expect("Failed to start session");
    assert_eq!(session.state, SessionState::Active);
    assert_eq!(session.workflow_id, wf_id);
    assert!(service.active_session().is_some());

    // 2. Starting another session while one is active produces an error
    let duplicate_res = service.start_session(&wf_id);
    assert!(matches!(
        duplicate_res,
        Err(WorkflowServiceError::ActiveSessionAlreadyExists(_))
    ));

    // 3. Pause session
    service.pause_session().expect("Failed to pause");
    assert_eq!(
        service.active_session().unwrap().state,
        SessionState::Paused
    );

    // 4. Resume session
    service.resume_session().expect("Failed to resume");
    assert_eq!(
        service.active_session().unwrap().state,
        SessionState::Active
    );

    // 5. Complete session
    let completed = service.complete_session().expect("Failed to complete");
    assert_eq!(completed.state, SessionState::Completed);
    assert!(service.active_session().is_none());
}

#[test]
fn test_ws02_cancellation_contracts() {
    let mock = MockSource::new(sample_obs("code.exe", "main.rs", None, IdleState::Active));
    let mut wf_repo = InMemoryWorkflowRepository::new();
    let session_repo = InMemorySessionRepository::new();

    // Workflow with AccountableBreakable contract
    let breakable_wf = create_test_workflow(CommitmentContract::AccountableBreakable {
        min_reason_characters: 10,
    });
    let breakable_id = breakable_wf.id.clone();
    wf_repo.save(&breakable_wf).unwrap();

    // Workflow with Unbreakable contract
    let mut unbreakable_wf = create_test_workflow(CommitmentContract::Unbreakable {
        target_duration_seconds: 1800,
    });
    unbreakable_wf.id = WorkflowId::new("wf-unbreakable");
    let unbreakable_id = unbreakable_wf.id.clone();
    wf_repo.save(&unbreakable_wf).unwrap();

    let mut service = WorkflowService::new(
        Box::new(mock.clone()),
        Box::new(wf_repo),
        Box::new(session_repo),
        1000,
    );

    // Test 1: AccountableBreakable requires a reason of min length
    service.start_session(&breakable_id).unwrap();
    let no_reason_res = service.cancel_session(None);
    assert!(matches!(
        no_reason_res,
        Err(WorkflowServiceError::Session(
            SessionError::InsufficientReason {
                min_chars: 10,
                actual_chars: 0
            }
        ))
    ));
    // Short reason also rejected
    let short_reason_res = service.cancel_session(Some("hi".into()));
    assert!(matches!(
        short_reason_res,
        Err(WorkflowServiceError::Session(
            SessionError::InsufficientReason {
                min_chars: 10,
                actual_chars: 2
            }
        ))
    ));
    // Session is still active after failed cancellation attempt
    assert_eq!(
        service.active_session().unwrap().state,
        SessionState::Active
    );

    // Cancellation with valid reason (>= 10 chars) succeeds
    let cancelled = service
        .cancel_session(Some("Need to take an urgent call".into()))
        .unwrap();
    assert_eq!(cancelled.state, SessionState::Cancelled);
    assert!(service.active_session().is_none());

    // Test 2: Unbreakable contract strictly rejects cancellation
    service.start_session(&unbreakable_id).unwrap();
    let unbreakable_attempt = service.cancel_session(Some("I want to quit early".into()));
    assert!(matches!(
        unbreakable_attempt,
        Err(WorkflowServiceError::Session(
            SessionError::CannotBreakUnbreakableWorkflow
        ))
    ));
    assert_eq!(
        service.active_session().unwrap().state,
        SessionState::Active
    );

    // Can still naturally complete
    service.complete_session().unwrap();
}

#[test]
fn test_ws03_evaluate_tick_focus_accounting() {
    let mock = MockSource::new(sample_obs("code.exe", "main.rs", None, IdleState::Active));
    let mut wf_repo = InMemoryWorkflowRepository::new();
    let session_repo = InMemorySessionRepository::new();

    let workflow = create_test_workflow(CommitmentContract::AccountableBreakable {
        min_reason_characters: 10,
    });
    let wf_id = workflow.id.clone();
    wf_repo.save(&workflow).unwrap();

    let mut service = WorkflowService::new(
        Box::new(mock.clone()),
        Box::new(wf_repo),
        Box::new(session_repo),
        1000,
    );

    service.start_session(&wf_id).unwrap();

    // Tick 1: Allowed application
    let eval1 = service.evaluate_tick().unwrap();
    assert!(eval1.decision.is_allowed());
    assert_eq!(eval1.session.total_focus_seconds, 1);

    // Tick 2: Allowed browser tab
    mock.set(sample_obs(
        "chrome.exe",
        "Rust Docs",
        Some("https://doc.rust-lang.org/book/ch01.html"),
        IdleState::Active,
    ));
    let eval2 = service.evaluate_tick().unwrap();
    assert!(eval2.decision.is_allowed());
    assert_eq!(eval2.session.total_focus_seconds, 2);
    assert_eq!(eval2.session.total_distraction_seconds, 0);
    assert_eq!(eval2.session.total_grace_seconds, 0);
}

#[test]
fn test_ws04_evaluate_tick_grace_and_pull_back_resilience() {
    let mock = MockSource::new(sample_obs("code.exe", "main.rs", None, IdleState::Active));
    let mut wf_repo = InMemoryWorkflowRepository::new();
    let session_repo = InMemorySessionRepository::new();

    let workflow = create_test_workflow(CommitmentContract::AccountableBreakable {
        min_reason_characters: 10,
    });
    let wf_id = workflow.id.clone();
    wf_repo.save(&workflow).unwrap();

    let mut service = WorkflowService::new(
        Box::new(mock.clone()),
        Box::new(wf_repo),
        Box::new(session_repo),
        1000,
    );

    service.start_session(&wf_id).unwrap();

    // 1. First tick: focused in VS Code
    service.evaluate_tick().unwrap();

    // 2. User drifts to reddit.com (restricted rule with 30s grace)
    mock.set(sample_obs(
        "chrome.exe",
        "Reddit - Dive into anything",
        Some("https://reddit.com/r/all"),
        IdleState::Active,
    ));

    let eval_drift = service.evaluate_tick().unwrap();
    assert!(
        matches!(eval_drift.decision, PolicyDecision::GracePeriod { .. }),
        "Expected GracePeriod decision, got: {:?}",
        eval_drift.decision
    );
    assert_eq!(eval_drift.session.total_grace_seconds, 1);
    assert_eq!(eval_drift.session.pull_back_count, 0);

    // 3. User self-corrects and returns to VS Code within 30 seconds!
    mock.set(sample_obs("code.exe", "main.rs", None, IdleState::Active));
    let eval_returned = service.evaluate_tick().unwrap();

    assert!(eval_returned.decision.is_allowed());
    // Pull-back resilience should be registered!
    assert!(
        eval_returned.pull_back_event.is_some(),
        "Expected pull_back_event to be generated"
    );
    let pb_event = eval_returned.pull_back_event.unwrap();
    assert!(matches!(
        pb_event.resolution,
        DriftResolution::SelfCorrected { .. }
    ));
    assert_eq!(eval_returned.session.pull_back_count, 1);
}

#[test]
fn test_ws05_afk_and_paused_accounting() {
    let mock = MockSource::new(sample_obs("code.exe", "main.rs", None, IdleState::Active));
    let mut wf_repo = InMemoryWorkflowRepository::new();
    let session_repo = InMemorySessionRepository::new();

    let workflow = create_test_workflow(CommitmentContract::AccountableBreakable {
        min_reason_characters: 10,
    });
    let wf_id = workflow.id.clone();
    wf_repo.save(&workflow).unwrap();

    let mut service = WorkflowService::new(
        Box::new(mock.clone()),
        Box::new(wf_repo),
        Box::new(session_repo),
        1000,
    );

    service.start_session(&wf_id).unwrap();

    // 1. AFK state does not penalize or start grace countdown
    mock.set(sample_obs("game.exe", "Game", None, IdleState::Afk));
    let eval_afk = service.evaluate_tick().unwrap();
    assert!(matches!(eval_afk.decision, PolicyDecision::Ignored { .. }));
    assert_eq!(eval_afk.session.total_afk_seconds, 1);
    assert_eq!(eval_afk.session.total_distraction_seconds, 0);
    assert_eq!(eval_afk.session.total_grace_seconds, 0);

    // 2. Pause session
    service.pause_session().unwrap();
    let eval_paused = service.evaluate_tick().unwrap();
    assert!(matches!(
        eval_paused.decision,
        PolicyDecision::Ignored { .. }
    ));
    assert_eq!(eval_paused.session.total_paused_seconds, 1);
}
