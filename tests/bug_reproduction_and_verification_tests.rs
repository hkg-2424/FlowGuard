use chrono::Utc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flowguard::application::ports::{
    ActivitySource, SessionRepository, SourceError, WorkflowRepository,
};
use flowguard::application::workflow_service::WorkflowService;
use flowguard::domain::activity::{ActivityObservation, BrowserContext, FreshnessState, IdleState};
use flowguard::domain::workflow::{
    parse_canonical_resource, CanonicalResource, CommitmentContract, DriftResolution,
    GracePeriodTimer, PolicyDecision, RuleEffect, RuleEngine, RuleId, RuleMatcher, RuleScope,
    SessionId, SessionState, TimerStatus, Workflow, WorkflowId, WorkflowRule, WorkflowSession,
};
use flowguard::domain::{ActivityCapabilities, DiagnosticReport, SourceCapabilities, SourceHealth};
use flowguard::infrastructure::persistence::sqlite::{
    SqliteDb, SqliteSessionRepository, SqliteWorkflowRepository,
};
use flowguard::infrastructure::persistence::{
    InMemorySessionRepository, InMemoryWorkflowRepository,
};

#[derive(Clone)]
struct MockActivitySource {
    current_obs: Arc<Mutex<ActivityObservation>>,
}

impl MockActivitySource {
    fn new(obs: ActivityObservation) -> Self {
        Self {
            current_obs: Arc::new(Mutex::new(obs)),
        }
    }

    fn set(&self, obs: ActivityObservation) {
        *self.current_obs.lock().unwrap() = obs;
    }
}

impl ActivitySource for MockActivitySource {
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
            "MockActivitySource",
            "mock://",
            SourceHealth::Connected,
            String::new(),
        ))
    }
}

fn create_observation(
    app: &str,
    title: &str,
    url: Option<&str>,
    idle: IdleState,
) -> ActivityObservation {
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

// =========================================================================
// Verification 1: GracePeriodTimer AFK Pause Freezes Timer
// =========================================================================
#[test]
fn test_reproduce_grace_period_afk_pause_leak() {
    let mut timer = GracePeriodTimer::new();
    let rule_id = RuleId::new("rule-distract");
    let target = "reddit.com";

    // Start a grace period with 1 second duration
    let status1 = timer.tick(&rule_id, target, 1);
    assert!(
        matches!(status1, TimerStatus::GracePeriod { .. }),
        "Timer should enter GracePeriod"
    );

    // User steps away (AFK), so pause is called
    timer.pause();

    // Sleep for 1.1s (exceeding the 1.0s grace window while paused)
    std::thread::sleep(Duration::from_millis(1100));

    // Spec §4.3 Invariant 4 & ADR-08 require: countdown FREEZES during AFK/pause.
    let status2 = timer.tick(&rule_id, target, 1);
    assert!(
        matches!(status2, TimerStatus::GracePeriod { .. }),
        "Timer must remain in GracePeriod because elapsed time was frozen during pause"
    );

    // Furthermore, cancel() records self-corrected resolution when returning within grace:
    let resolution = timer.cancel();
    assert!(
        matches!(resolution, Some(DriftResolution::SelfCorrected { .. })),
        "DriftResolution must be SelfCorrected when self-correcting within grace window"
    );
}

// =========================================================================
// Verification 2: Unbreakable Contract Rejects Pause
// =========================================================================
#[test]
fn test_reproduce_unbreakable_contract_pause_bypass() {
    let mut session = WorkflowSession::new(
        SessionId::new("sess-unbreak-001"),
        WorkflowId::new("wf-deep-work"),
        CommitmentContract::Unbreakable {
            target_duration_seconds: 3600,
        },
    );
    session.start().expect("Session should start");

    // Spec §3.1 & §6.2 state that an Unbreakable (Ulysses) contract session cannot be paused or cancelled early.
    let pause_result = session.pause();
    assert!(
        matches!(
            pause_result,
            Err(flowguard::domain::workflow::SessionError::CannotBreakUnbreakableWorkflow)
        ),
        "Unbreakable contract must reject pause with CannotBreakUnbreakableWorkflow"
    );
    assert_eq!(session.state, SessionState::Active);
}

// =========================================================================
// Verification 3: Rule Specificity Scope Hierarchy Overrides Priority Weight
// =========================================================================
#[test]
fn test_reproduce_specificity_calculation_scope_leak() {
    let wf_id = WorkflowId::new("wf-spec-leak");
    let mut workflow = Workflow::new(wf_id.clone(), "Specificity Leak Test");

    // Application rule (Scope: Application = 200) with high priority_weight
    workflow = workflow.with_rule(WorkflowRule {
        id: RuleId::new("rule-app-allow"),
        workflow_id: wf_id.clone(),
        description: "Allow browser with high weight".into(),
        scope: RuleScope::Application, // 200
        matcher: RuleMatcher::ApplicationName("chrome.exe".into()),
        effect: RuleEffect::Allow,
        priority_weight: 201_000, // High priority weight
        custom_grace_seconds: None,
        is_enabled: true,
        expires_at: None,
    });

    // Domain rule (Scope: Domain = 400) intended to restrict Reddit
    workflow = workflow.with_rule(WorkflowRule {
        id: RuleId::new("rule-domain-restrict"),
        workflow_id: wf_id.clone(),
        description: "Restrict Reddit".into(),
        scope: RuleScope::Domain, // 400
        matcher: RuleMatcher::DomainPattern("reddit.com".into()),
        effect: RuleEffect::Restrict,
        priority_weight: 0,
        custom_grace_seconds: Some(30),
        is_enabled: true,
        expires_at: None,
    });

    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let obs = create_observation(
        "chrome.exe",
        "reddit.com: r/all",
        Some("https://reddit.com/r/all"),
        IdleState::Active,
    );

    let decision = engine.evaluate(&obs, &workflow, &mut timer, Utc::now());

    // Spec §4.2 Invariant 1: Higher scope level (Domain = 400) MUST unconditionally override lower scope level (Application = 200).
    assert!(
        decision.is_violation(),
        "Domain scope rule must override Application scope rule regardless of priority weight"
    );
}

// =========================================================================
// Verification 4: Canonical Resource Phishing Rejection and Normalization
// =========================================================================
#[test]
fn test_reproduce_canonical_resource_phishing_vulnerability() {
    // 1. Phishing YouTube domain must NOT match YouTubeVideo
    let phishing_yt = "https://phishingyoutube.com/watch?v=dQw4w9WgXcQ";
    let yt_res = parse_canonical_resource(phishing_yt);
    assert!(
        yt_res.is_none(),
        "phishingyoutube.com must not be parsed as a valid YouTube canonical resource"
    );

    // 2. Another variant: notyoutube.com
    let evil_yt = "https://notyoutube.com/watch?v=dQw4w9WgXcQ";
    assert!(
        parse_canonical_resource(evil_yt).is_none(),
        "notyoutube.com must not be accepted as canonical YouTube"
    );

    // 3. Phishing LeetCode domain
    let fake_lc = "https://fakeleetcode.com/problems/two-sum/";
    let lc_res = parse_canonical_resource(fake_lc);
    assert!(
        lc_res.is_none(),
        "fakeleetcode.com must not be parsed as a valid LeetCode canonical resource"
    );

    // 4. Shorts trailing slash issue normalized
    let shorts_trailing = "https://www.youtube.com/shorts/dQw4w9WgXcQ/";
    let shorts_res = parse_canonical_resource(shorts_trailing);
    assert_eq!(
        shorts_res,
        Some(CanonicalResource::YouTubeVideo {
            video_id: "dQw4w9WgXcQ".to_string(),
        }),
        "YouTube shorts video ID must have trailing slashes stripped"
    );
}

// =========================================================================
// Verification 5: WorkflowService Intervention Deduplication & Target Preservation
// =========================================================================
#[test]
fn test_reproduce_intervention_inflation_and_target_inversion() {
    let mock = MockActivitySource::new(create_observation(
        "code.exe",
        "main.rs",
        None,
        IdleState::Active,
    ));
    let mut wf_repo = InMemoryWorkflowRepository::new();
    let session_repo = InMemorySessionRepository::new();

    let wf_id = WorkflowId::new("wf-eval-bugs");
    let mut workflow = Workflow::new(wf_id.clone(), "Eval Bugs Workflow")
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-code"),
            workflow_id: wf_id.clone(),
            description: "Allow Code".into(),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName("code.exe".into()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        })
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-reddit"),
            workflow_id: wf_id.clone(),
            description: "Restrict Reddit".into(),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern("reddit.com".into()),
            effect: RuleEffect::Restrict,
            priority_weight: 20,
            custom_grace_seconds: Some(0), // 0 grace = immediate intervention
            is_enabled: true,
            expires_at: None,
        });

    workflow.commitment_contract = CommitmentContract::Standard;
    wf_repo.save(&workflow).unwrap();

    let mut service = WorkflowService::new(
        Box::new(mock.clone()),
        Box::new(wf_repo),
        Box::new(session_repo),
        1000,
    );

    service.start_session(&wf_id).unwrap();

    // 1. User navigates to Reddit (immediate intervention because grace = 0)
    mock.set(create_observation(
        "chrome.exe",
        "reddit.com",
        Some("https://reddit.com/r/all"),
        IdleState::Active,
    ));

    // Tick 1 in intervention
    let eval1 = service.evaluate_tick().unwrap();
    assert!(matches!(
        eval1.decision,
        PolicyDecision::InterventionRequired { .. }
    ));
    assert_eq!(eval1.session.intervention_count, 1);

    // Tick 2 in same continuous violation episode: should NOT increment again
    let eval2 = service.evaluate_tick().unwrap();
    assert!(matches!(
        eval2.decision,
        PolicyDecision::InterventionRequired { .. }
    ));
    assert_eq!(
        eval2.session.intervention_count, 1,
        "intervention_count must remain 1 during a continuous intervention episode"
    );

    // Tick 3 in same episode: should still NOT increment
    let eval3 = service.evaluate_tick().unwrap();
    assert_eq!(
        eval3.session.intervention_count, 1,
        "intervention_count must not inflate on subsequent ticks"
    );

    // 2. Target Preservation in Pull-Back DriftEvent:
    // Reset service with 30s grace rule
    let mut wf2 = Workflow::new(WorkflowId::new("wf2"), "Grace WF")
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-code"),
            workflow_id: WorkflowId::new("wf2"),
            description: "Allow Code".into(),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName("code.exe".into()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        })
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-reddit"),
            workflow_id: WorkflowId::new("wf2"),
            description: "Restrict Reddit".into(),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern("reddit.com".into()),
            effect: RuleEffect::Restrict,
            priority_weight: 20,
            custom_grace_seconds: Some(30),
            is_enabled: true,
            expires_at: None,
        });
    wf2.commitment_contract = CommitmentContract::Standard;

    let mut wf2_repo = InMemoryWorkflowRepository::new();
    let s2_repo = InMemorySessionRepository::new();
    wf2_repo.save(&wf2).unwrap();

    let mut service2 = WorkflowService::new(
        Box::new(mock.clone()),
        Box::new(wf2_repo),
        Box::new(s2_repo),
        1000,
    );
    service2.start_session(&wf2.id).unwrap();

    // User visits reddit -> enters grace period
    mock.set(create_observation(
        "chrome.exe",
        "reddit.com",
        Some("https://reddit.com/r/all"),
        IdleState::Active,
    ));
    let eval_drift = service2.evaluate_tick().unwrap();
    assert!(matches!(
        eval_drift.decision,
        PolicyDecision::GracePeriod { .. }
    ));

    // User returns to code.exe -> self-corrects!
    mock.set(create_observation(
        "code.exe",
        "main.rs",
        None,
        IdleState::Active,
    ));
    let eval_recover = service2.evaluate_tick().unwrap();
    assert!(eval_recover.decision.is_allowed());
    assert!(eval_recover.pull_back_event.is_some());

    let pb_event = eval_recover.pull_back_event.unwrap();
    assert_eq!(
        pb_event.target_name, "https://reddit.com/r/all",
        "pull_back_event must preserve the violation target, not the productive application returned to"
    );
}

// =========================================================================
// Verification Suite: SQLite Persistence Under Concurrent Thread Access
// =========================================================================
#[test]
fn test_sqlite_concurrent_multithreaded_access() {
    let db = SqliteDb::open_in_memory().expect("Failed to create in-memory SQLite DB");
    let mut wf_repo = SqliteWorkflowRepository::new(db.clone());
    let session_repo = SqliteSessionRepository::new(db.clone());

    // Seed an initial workflow
    let base_wf_id = WorkflowId::new("wf-concurrent-base");
    let base_wf = Workflow::new(base_wf_id.clone(), "Base Concurrent Workflow");
    wf_repo
        .save(&base_wf)
        .expect("Failed to save base workflow");

    let num_threads = 8;
    let iterations_per_thread = 15;
    let mut handles = Vec::new();

    for t_idx in 0..num_threads {
        let wf_repo_clone = wf_repo.clone();
        let mut session_repo_clone = session_repo.clone();
        let base_wf_id_clone = base_wf_id.clone();

        let handle = std::thread::spawn(move || {
            for i in 0..iterations_per_thread {
                // 1. Read existing workflow
                let retrieved = wf_repo_clone.get(&base_wf_id_clone).unwrap();
                assert!(retrieved.is_some());

                // 2. Create unique session
                let session_id = SessionId::new(format!("sess_t{}_{}", t_idx, i));
                let mut session = WorkflowSession::new(
                    session_id.clone(),
                    base_wf_id_clone.clone(),
                    CommitmentContract::Standard,
                );
                session.start().unwrap();
                session.total_focus_seconds = (t_idx * 100 + i) as u64;

                session_repo_clone.save(&session).unwrap();

                // 3. Record drift event
                let drift = flowguard::domain::workflow::DriftEvent {
                    session_id: session_id.clone(),
                    target_name: format!("target_{}", i),
                    duration_seconds: 5,
                    resolution: DriftResolution::SelfCorrected {
                        recovery_seconds: 5,
                    },
                    timestamp: Utc::now(),
                };
                session_repo_clone.record_drift(&drift).unwrap();

                // 4. Retrieve session and verify
                let s = session_repo_clone.get(&session_id).unwrap().unwrap();
                assert_eq!(s.total_focus_seconds, (t_idx * 100 + i) as u64);

                // 5. Retrieve drifts
                let drifts = session_repo_clone.list_drifts(&session_id).unwrap();
                assert_eq!(drifts.len(), 1);
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle
            .join()
            .expect("Thread failed during concurrent SQLite operations");
    }

    // Verify all sessions were recorded correctly
    for t_idx in 0..num_threads {
        for i in 0..iterations_per_thread {
            let session_id = SessionId::new(format!("sess_t{}_{}", t_idx, i));
            let s = session_repo.get(&session_id).unwrap();
            assert!(
                s.is_some(),
                "Session {} was lost in concurrent writes!",
                session_id
            );
        }
    }
}

#[test]
fn test_domain_matching_does_not_leak_to_non_browsers() {
    let wf_id = WorkflowId::new("wf-study");
    let mut wf = Workflow::new(wf_id.clone(), "Study".to_string());
    wf.default_effect = RuleEffect::Restrict;
    wf.rules.push(WorkflowRule {
        id: RuleId::new("rule-yt-domain"),
        workflow_id: wf_id.clone(),
        description: "Allow youtube.com".to_string(),
        scope: RuleScope::Domain,
        matcher: RuleMatcher::DomainPattern("youtube.com".to_string()),
        effect: RuleEffect::Allow,
        priority_weight: 100,
        custom_grace_seconds: None,
        is_enabled: true,
        expires_at: None,
    });

    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();

    // 1. Browser window with youtube in title -> ALLOWED
    let browser_obs = create_observation(
        "chrome.exe",
        "Rust Tutorial - youtube.com - Google Chrome",
        None,
        IdleState::Active,
    );
    let decision = engine.evaluate(&browser_obs, &wf, &mut timer, now);
    assert!(
        decision.is_allowed(),
        "Browser window should match domain rule: {:?}",
        decision
    );

    // 2. Non-browser window with youtube in title (e.g. Discord chat #youtube or Spotify song) -> RESTRICTED
    let discord_obs = create_observation(
        "discord.exe",
        "#youtube-chat - Discord",
        None,
        IdleState::Active,
    );
    let decision_discord = engine.evaluate(&discord_obs, &wf, &mut timer, now);
    assert!(
        decision_discord.is_violation(),
        "Non-browser application must NOT match domain rule fallback: {:?}",
        decision_discord
    );

    let steam_obs = create_observation(
        "steam.exe",
        "Steam - youtube fan club",
        None,
        IdleState::Active,
    );
    let decision_steam = engine.evaluate(&steam_obs, &wf, &mut timer, now);
    assert!(
        decision_steam.is_violation(),
        "Steam must NOT match domain rule fallback: {:?}",
        decision_steam
    );
}

#[test]
fn test_sqlite_workflow_rule_id_uniqueness_across_multiple_custom_workflows() {
    let db = SqliteDb::open_in_memory().unwrap();
    let mut wf_repo = SqliteWorkflowRepository::new(db);

    let wf1_id = WorkflowId::new("wf-custom-1");
    let mut wf1 = Workflow::new(wf1_id.clone(), "Custom 1".to_string());
    wf1.rules.push(WorkflowRule {
        id: RuleId::new(format!("rule-{}-app-0", wf1_id.0)),
        workflow_id: wf1_id.clone(),
        description: "Allow code".to_string(),
        scope: RuleScope::Application,
        matcher: RuleMatcher::ApplicationName("code.exe".to_string()),
        effect: RuleEffect::Allow,
        priority_weight: 10,
        custom_grace_seconds: None,
        is_enabled: true,
        expires_at: None,
    });

    let wf2_id = WorkflowId::new("wf-custom-2");
    let mut wf2 = Workflow::new(wf2_id.clone(), "Custom 2".to_string());
    wf2.rules.push(WorkflowRule {
        id: RuleId::new(format!("rule-{}-app-0", wf2_id.0)),
        workflow_id: wf2_id.clone(),
        description: "Allow obsidian".to_string(),
        scope: RuleScope::Application,
        matcher: RuleMatcher::ApplicationName("obsidian.exe".to_string()),
        effect: RuleEffect::Allow,
        priority_weight: 10,
        custom_grace_seconds: None,
        is_enabled: true,
        expires_at: None,
    });

    // Both should save without UNIQUE constraint error on workflow_rules.id
    assert!(wf_repo.save(&wf1).is_ok());
    assert!(wf_repo.save(&wf2).is_ok());

    let loaded1 = wf_repo.get(&wf1_id).unwrap().unwrap();
    assert_eq!(loaded1.rules.len(), 1);
    let loaded2 = wf_repo.get(&wf2_id).unwrap().unwrap();
    assert_eq!(loaded2.rules.len(), 1);
}

#[test]
fn test_app_matches_with_and_without_exe_extensions() {
    use flowguard::domain::workflow::app_matches;

    // Direct and alias matches
    assert!(app_matches("code.exe", None, "code"));
    assert!(app_matches("code", None, "code.exe"));
    assert!(app_matches("code.exe", None, "vs code"));
    assert!(app_matches("code.exe", None, "visual studio code"));

    assert!(app_matches("windowsterminal.exe", None, "terminal"));
    assert!(app_matches("cmd.exe", None, "terminal"));
    assert!(app_matches("powershell.exe", None, "terminal"));
    assert!(app_matches("pwsh.exe", None, "terminal"));

    assert!(app_matches("msedge.exe", None, "edge"));
    assert!(app_matches("chrome.exe", None, "chrome"));
    assert!(app_matches("brave.exe", None, "brave"));

    // Antigravity matching
    assert!(app_matches("antigravity.exe", None, "antigravity"));
    assert!(app_matches(
        "electron.exe",
        Some("Antigravity - Focus"),
        "antigravity"
    ));
}
