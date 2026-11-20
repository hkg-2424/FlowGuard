use chrono::Utc;
use flowguard::domain::activity::{ActivityObservation, BrowserContext, FreshnessState, IdleState};
use flowguard::domain::workflow::{
    CanonicalResource, CommitmentContract, DriftResolution, GracePeriodTimer, PolicyDecision,
    RuleEffect, RuleEngine, RuleId, RuleMatcher, RuleScope, SessionError, SessionId, SessionState,
    TimerStatus, Workflow, WorkflowId, WorkflowRule, WorkflowSession,
};
use flowguard::domain::{ActivityCapabilities, SourceHealth};

fn sample_observation(app: &str, title: &str, url: Option<&str>) -> ActivityObservation {
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
        idle_state: IdleState::Active,
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
// M2 Test Suite: Workflow Engine & Deterministic Rule Evaluation
// =========================================================================

#[test]
fn test_wf01_multi_app_legitimate_switching_without_penalty() {
    let wf_id = WorkflowId::new("wf-multi-app");
    let mut workflow = Workflow::new(wf_id.clone(), "Full-Stack Development");

    // Allow VS Code, Terminal, and Chrome
    workflow = workflow
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-code"),
            workflow_id: wf_id.clone(),
            description: "Allow IDE".into(),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName("code.exe".into()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        })
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-terminal"),
            workflow_id: wf_id.clone(),
            description: "Allow Terminal".into(),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName("windowsterminal.exe".into()),
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
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });

    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();

    // 1. Working in VS Code -> Allowed
    let obs_code = sample_observation("code.exe", "src/main.rs - FlowGuard", None);
    let decision = engine.evaluate(&obs_code, &workflow, &mut timer, now);
    assert!(decision.is_allowed());
    assert!(!timer.is_active());

    // 2. Switch to Terminal -> Allowed (zero penalty, timer stays idle)
    let obs_term = sample_observation("windowsterminal.exe", "cargo build", None);
    let decision = engine.evaluate(&obs_term, &workflow, &mut timer, now);
    assert!(decision.is_allowed());
    assert!(!timer.is_active());

    // 3. Switch to Browser reading std docs -> Allowed
    let obs_docs = sample_observation(
        "chrome.exe",
        "std::sync::Arc - Rust",
        Some("https://doc.rust-lang.org/std/sync/struct.Arc.html"),
    );
    let decision = engine.evaluate(&obs_docs, &workflow, &mut timer, now);
    assert!(decision.is_allowed());
    assert!(!timer.is_active());
}

#[test]
fn test_wf02_single_domain_allowed_resource_vs_unallowed_pages() {
    let wf_id = WorkflowId::new("wf-yt-lock");
    let mut workflow = Workflow::new(wf_id.clone(), "React Deep Dive");

    // Allow Chrome generally
    workflow = workflow.with_rule(WorkflowRule {
        id: RuleId::new("rule-chrome"),
        workflow_id: wf_id.clone(),
        description: "Allow Chrome".into(),
        scope: RuleScope::Application,
        matcher: RuleMatcher::ApplicationName("chrome.exe".into()),
        effect: RuleEffect::Allow,
        priority_weight: 0,
        custom_grace_seconds: None,
        is_enabled: true,
        expires_at: None,
    });

    // Restrict youtube.com as a whole domain
    workflow = workflow.with_rule(WorkflowRule {
        id: RuleId::new("rule-yt-domain"),
        workflow_id: wf_id.clone(),
        description: "Restrict YouTube General".into(),
        scope: RuleScope::Domain,
        matcher: RuleMatcher::DomainPattern("youtube.com".into()),
        effect: RuleEffect::Restrict,
        priority_weight: 0,
        custom_grace_seconds: Some(30),
        is_enabled: true,
        expires_at: None,
    });

    // But lock in and ALLOW one specific lecture video!
    let allowed_vid = "bMknfKXIFA8";
    workflow = workflow.with_rule(WorkflowRule {
        id: RuleId::new("rule-yt-video-lecture"),
        workflow_id: wf_id.clone(),
        description: "Authorized React Lecture".into(),
        scope: RuleScope::ExactResource,
        matcher: RuleMatcher::Canonical(CanonicalResource::YouTubeVideo {
            video_id: allowed_vid.into(),
        }),
        effect: RuleEffect::Allow,
        priority_weight: 10,
        custom_grace_seconds: None,
        is_enabled: true,
        expires_at: None,
    });

    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();

    // 1. Visiting the authorized video URL -> Allowed! (ExactResource scope overrides Domain)
    let obs_lecture = sample_observation(
        "chrome.exe",
        "React 18 Course - Module 4 - YouTube",
        Some("https://www.youtube.com/watch?v=bMknfKXIFA8&t=120s"),
    );
    let decision = engine.evaluate(&obs_lecture, &workflow, &mut timer, now);
    assert_eq!(
        decision,
        PolicyDecision::Allowed {
            rule_id: RuleId::new("rule-yt-video-lecture"),
            scope: RuleScope::ExactResource,
        }
    );
    assert!(!timer.is_active());

    // 2. Autoplay or sidebar click moves to an entertainment video -> Restrict & GracePeriod!
    let obs_unrelated = sample_observation(
        "chrome.exe",
        "Funny Cat Compilation - YouTube",
        Some("https://www.youtube.com/watch?v=xyz999cat"),
    );
    let decision2 = engine.evaluate(&obs_unrelated, &workflow, &mut timer, now);
    match decision2 {
        PolicyDecision::GracePeriod {
            rule_id,
            remaining_seconds,
            total_grace_seconds,
            ..
        } => {
            assert_eq!(rule_id, RuleId::new("rule-yt-domain"));
            assert_eq!(total_grace_seconds, 30);
            assert!(remaining_seconds <= 30);
        }
        other => panic!("Expected GracePeriod decision, got: {:?}", other),
    }
    assert!(timer.is_active());

    // 3. User realizes and clicks back to the allowed lecture video -> Grace immediately cancelled!
    let decision3 = engine.evaluate(&obs_lecture, &workflow, &mut timer, now);
    assert!(decision3.is_allowed());
    assert!(!timer.is_active());
}

#[test]
fn test_wf03_stale_observations_never_trigger_enforcement() {
    let wf_id = WorkflowId::new("wf-stale-guard");
    let mut workflow = Workflow::new(wf_id.clone(), "Focus Guard");
    workflow.default_effect = RuleEffect::Restrict;

    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();

    let mut obs = sample_observation("twitter.exe", "Twitter / X", None);
    // Mark observation as Stale
    obs.window_freshness = FreshnessState::Stale;

    let decision = engine.evaluate(&obs, &workflow, &mut timer, now);
    assert_eq!(
        decision,
        PolicyDecision::ObservationUnavailable {
            reason: "Foreground window observation is stale or missing".into(),
        }
    );
    // Timer must NEVER start on stale data
    assert!(!timer.is_active());
}

#[test]
fn test_wf04_inactivity_and_afk_never_mistaken_for_distraction() {
    let wf_id = WorkflowId::new("wf-afk-guard");
    let mut workflow = Workflow::new(wf_id.clone(), "Deep Writing");
    workflow.default_effect = RuleEffect::Restrict;

    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();

    let mut obs = sample_observation("word.exe", "Thesis Chapter 3", None);
    // User steps away from keyboard to think
    obs.idle_state = IdleState::Afk;

    let decision = engine.evaluate(&obs, &workflow, &mut timer, now);
    match decision {
        PolicyDecision::Ignored { reason } => {
            assert!(reason.contains("AFK"));
        }
        other => panic!("Expected Ignored decision for AFK state, got: {:?}", other),
    }
    assert!(!timer.is_active());
}

#[test]
fn test_wf05_transitory_windows_system_surfaces_ignored() {
    let wf_id = WorkflowId::new("wf-shell-guard");
    let mut workflow = Workflow::new(wf_id.clone(), "Focus");
    workflow.default_effect = RuleEffect::Restrict;

    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();

    let obs_alt_tab = sample_observation("shellexperiencehost.exe", "Task Switching", None);
    let decision = engine.evaluate(&obs_alt_tab, &workflow, &mut timer, now);
    assert!(decision.is_allowed());

    let obs_lock = sample_observation("lockapp.exe", "Windows Default Lock Screen", None);
    let decision_lock = engine.evaluate(&obs_lock, &workflow, &mut timer, now);
    assert!(decision_lock.is_allowed());
}

#[test]
fn test_wf06_deterministic_rule_conflict_resolution() {
    let wf_id = WorkflowId::new("wf-conflict");
    let mut workflow = Workflow::new(wf_id.clone(), "Conflict Test");

    // Rule 1: Application level allow for chrome.exe (Scope: Application = 200)
    workflow = workflow.with_rule(WorkflowRule {
        id: RuleId::new("rule-app-chrome"),
        workflow_id: wf_id.clone(),
        description: "Allow Chrome app".into(),
        scope: RuleScope::Application,
        matcher: RuleMatcher::ApplicationName("chrome.exe".into()),
        effect: RuleEffect::Allow,
        priority_weight: 0,
        custom_grace_seconds: None,
        is_enabled: true,
        expires_at: None,
    });

    // Rule 2: Domain level restrict for reddit.com (Scope: Domain = 400)
    workflow = workflow.with_rule(WorkflowRule {
        id: RuleId::new("rule-domain-reddit"),
        workflow_id: wf_id.clone(),
        description: "Restrict Reddit".into(),
        scope: RuleScope::Domain,
        matcher: RuleMatcher::DomainPattern("reddit.com".into()),
        effect: RuleEffect::Restrict,
        priority_weight: 0,
        custom_grace_seconds: Some(30),
        is_enabled: true,
        expires_at: None,
    });

    // Rule 3: Exact URL allow for r/rust (Scope: ExactUrl = 800)
    workflow = workflow.with_rule(WorkflowRule {
        id: RuleId::new("rule-url-rust-sub"),
        workflow_id: wf_id.clone(),
        description: "Allow r/rust subreddit".into(),
        scope: RuleScope::ExactUrl,
        matcher: RuleMatcher::ExactUrl("https://reddit.com/r/rust".into()),
        effect: RuleEffect::Allow,
        priority_weight: 0,
        custom_grace_seconds: None,
        is_enabled: true,
        expires_at: None,
    });

    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();

    // Scenario A: chrome.exe visiting r/all -> Domain rule (Restrict, 400) beats Application rule (Allow, 200)
    let obs_all = sample_observation(
        "chrome.exe",
        "reddit.com: r/all",
        Some("https://reddit.com/r/all"),
    );
    let decision_a = engine.evaluate(&obs_all, &workflow, &mut timer, now);
    assert!(decision_a.is_violation());

    // Scenario B: chrome.exe visiting r/rust -> ExactUrl rule (Allow, 800) beats Domain rule (Restrict, 400)
    let obs_rust = sample_observation(
        "chrome.exe",
        "r/rust - The Rust Community",
        Some("https://reddit.com/r/rust"),
    );
    let decision_b = engine.evaluate(&obs_rust, &workflow, &mut timer, now);
    assert_eq!(
        decision_b,
        PolicyDecision::Allowed {
            rule_id: RuleId::new("rule-url-rust-sub"),
            scope: RuleScope::ExactUrl,
        }
    );
}

#[test]
fn test_wf07_pull_back_resilience_and_grace_timing() {
    let mut timer = GracePeriodTimer::new();
    let rule_id = RuleId::new("rule-distract");
    let target = "reddit.com";

    // 1. Violation starts -> Grace period 30 seconds
    let status = timer.tick(&rule_id, target, 30);
    match status {
        TimerStatus::GracePeriod {
            remaining_seconds,
            total_grace_seconds,
        } => {
            assert_eq!(total_grace_seconds, 30);
            assert!(remaining_seconds <= 30);
        }
        _ => panic!("Expected GracePeriod status"),
    }
    assert!(timer.is_active());

    // 2. User catches themselves within 5 seconds and returns -> Resolved as SelfCorrected!
    let resolution = timer.cancel();
    assert_eq!(
        resolution,
        Some(DriftResolution::SelfCorrected {
            recovery_seconds: 1, // minimal non-zero representation
        })
    );
    assert!(!timer.is_active());
}

#[test]
fn test_wf08_session_lifecycle_and_commitment_contracts() {
    let session_id = SessionId::new("session-001");
    let workflow_id = WorkflowId::new("wf-study");

    // Standard Contract: Can be cancelled freely
    let mut standard_session = WorkflowSession::new(
        session_id.clone(),
        workflow_id.clone(),
        CommitmentContract::Standard,
    );
    assert_eq!(standard_session.state, SessionState::Ready);

    standard_session.start().unwrap();
    assert_eq!(standard_session.state, SessionState::Active);

    standard_session.pause().unwrap();
    assert_eq!(standard_session.state, SessionState::Paused);

    standard_session.resume().unwrap();
    assert_eq!(standard_session.state, SessionState::Active);

    standard_session.cancel(None).unwrap();
    assert_eq!(standard_session.state, SessionState::Cancelled);

    // Accountable Breakable Contract: Fails if reason is too short
    let mut accountable_session = WorkflowSession::new(
        SessionId::new("session-002"),
        workflow_id.clone(),
        CommitmentContract::AccountableBreakable {
            min_reason_characters: 15,
        },
    );
    accountable_session.start().unwrap();

    let err = accountable_session
        .cancel(Some("too short".into()))
        .unwrap_err();
    assert_eq!(
        err,
        SessionError::InsufficientReason {
            min_chars: 15,
            actual_chars: 9,
        }
    );
    // State remains Active because cancellation was rejected!
    assert_eq!(accountable_session.state, SessionState::Active);

    // Successful cancellation with thoughtful reason
    let valid_reason = "Urgent production incident on customer cluster".to_string();
    accountable_session
        .cancel(Some(valid_reason.clone()))
        .unwrap();
    assert_eq!(accountable_session.state, SessionState::Cancelled);
    assert_eq!(accountable_session.early_exit_reason, Some(valid_reason));

    // Unbreakable (Ulysses) Contract: Rejects early exit unconditionally
    let mut unbreakable_session = WorkflowSession::new(
        SessionId::new("session-003"),
        workflow_id.clone(),
        CommitmentContract::Unbreakable {
            target_duration_seconds: 3600,
        },
    );
    unbreakable_session.start().unwrap();

    let unbreak_err = unbreakable_session
        .cancel(Some("I want to quit now".into()))
        .unwrap_err();
    assert_eq!(unbreak_err, SessionError::CannotBreakUnbreakableWorkflow);
    assert_eq!(unbreakable_session.state, SessionState::Active);

    // C-02: Early completion before target duration is also strictly rejected
    let complete_err = unbreakable_session.complete().unwrap_err();
    assert_eq!(complete_err, SessionError::CannotBreakUnbreakableWorkflow);
    assert_eq!(unbreakable_session.state, SessionState::Active);

    // Natural completion works once target duration has elapsed (H-19)
    unbreakable_session.total_focus_seconds = 3600;
    unbreakable_session.complete().unwrap();
    assert_eq!(unbreakable_session.state, SessionState::Completed);
}

#[test]
fn test_wf09_native_window_title_domain_fallback() {
    let wf_id = WorkflowId::new("wf-native-domain");
    let mut workflow = Workflow::new(wf_id.clone(), "Native Windows Domain Matching");

    // Workflow allows "github.com" and "wikipedia.org", but defaults to Restrict
    workflow = workflow
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-github"),
            workflow_id: wf_id.clone(),
            description: "Allow GitHub".into(),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern("github.com".into()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        })
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-wiki"),
            workflow_id: wf_id.clone(),
            description: "Allow Wikipedia".into(),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern("wikipedia.org".into()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });

    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();

    // 1. Native Windows observation: browser is None, but title has "GitHub"
    let obs_github = sample_observation("chrome.exe", "FlowGuardAG - GitHub - Google Chrome", None);
    let decision1 = engine.evaluate(&obs_github, &workflow, &mut timer, now);
    assert!(
        decision1.is_allowed(),
        "Native Windows observation with 'GitHub' in title must match DomainPattern('github.com')"
    );

    // 2. Native Windows observation: browser is None, but title has "Wikipedia"
    let obs_wiki = sample_observation(
        "msedge.exe",
        "Rust (programming language) - Wikipedia - Microsoft Edge",
        None,
    );
    let decision2 = engine.evaluate(&obs_wiki, &workflow, &mut timer, now);
    assert!(
        decision2.is_allowed(),
        "Native Windows observation with 'Wikipedia' in title must match DomainPattern('wikipedia.org')"
    );

    // 3. Native Windows observation: browser is None, title has "YouTube" (unauthorized domain)
    let obs_yt = sample_observation(
        "chrome.exe",
        "Never Gonna Give You Up - YouTube - Google Chrome",
        None,
    );
    let decision3 = engine.evaluate(&obs_yt, &workflow, &mut timer, now);
    assert!(
        decision3.is_violation(),
        "Native Windows observation on YouTube must not match GitHub or Wikipedia rules"
    );

    // 4. Native Windows observation: browser is None, title has "Reddit" (unauthorized domain)
    let obs_reddit = sample_observation("chrome.exe", "r/rust - Reddit - Google Chrome", None);
    let decision4 = engine.evaluate(&obs_reddit, &workflow, &mut timer, now);
    assert!(
        decision4.is_violation(),
        "Native Windows observation on Reddit must not match GitHub or Wikipedia rules"
    );
}

#[test]
fn test_wf10_grace_countdown_persists_across_unallowed_app_switches() {
    let wf_id = WorkflowId::new("wf-strict");
    let workflow = Workflow::new(wf_id.clone(), "Strict Coding").with_rule(WorkflowRule {
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
    });

    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();

    // 1. Initial drift into Discord (unallowed) -> Starts 30s grace window
    let obs_discord = sample_observation("discord.exe", "Discord", None);
    let dec1 = engine.evaluate(&obs_discord, &workflow, &mut timer, now);
    assert!(dec1.is_violation());
    assert!(timer.is_active());
    assert_eq!(timer.active_target(), Some("discord.exe"));

    // 2. User switches to Steam (also unallowed) -> Timer MUST NOT reset to 30!
    std::thread::sleep(std::time::Duration::from_millis(50));
    let obs_steam = sample_observation("steam.exe", "Steam Store", None);
    let dec2 = engine.evaluate(&obs_steam, &workflow, &mut timer, now);
    assert!(dec2.is_violation());
    assert_eq!(timer.active_target(), Some("steam.exe"));

    // 3. User switches to YouTube on Chrome (unallowed) -> Still ticking down
    let obs_yt = sample_observation("chrome.exe", "YouTube - Shorts", None);
    let dec3 = engine.evaluate(&obs_yt, &workflow, &mut timer, now);
    assert!(dec3.is_violation());
    assert_eq!(timer.active_target(), Some("chrome.exe"));

    // 4. Finally returns to VS Code -> Allowed, timer cancels with self-correction
    let obs_code = sample_observation("code.exe", "main.rs", None);
    let dec4 = engine.evaluate(&obs_code, &workflow, &mut timer, now);
    assert!(dec4.is_allowed());
    assert!(!timer.is_active());
}

#[test]
fn test_wf11_friendly_app_names_and_aliases() {
    let wf_id = WorkflowId::new("wf-friendly");
    let workflow = Workflow::new(wf_id.clone(), "Friendly Setup")
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-vs-code"),
            workflow_id: wf_id.clone(),
            description: "Allow VS Code".into(),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName("Visual Studio Code".into()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        })
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-antigravity"),
            workflow_id: wf_id.clone(),
            description: "Allow Antigravity".into(),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName("Antigravity".into()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });

    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();

    // 1. "code.exe" matches rule configured as "Visual Studio Code"
    let obs_code = sample_observation("code.exe", "src/main.rs - Visual Studio Code", None);
    let dec1 = engine.evaluate(&obs_code, &workflow, &mut timer, now);
    assert!(
        dec1.is_allowed(),
        "code.exe must match 'Visual Studio Code'"
    );

    // 2. Window with Antigravity in title or process matches "Antigravity"
    let obs_agy = sample_observation("electron.exe", "Antigravity - FlowGuardAG", None);
    let dec2 = engine.evaluate(&obs_agy, &workflow, &mut timer, now);
    assert!(
        dec2.is_allowed(),
        "Antigravity title must match 'Antigravity'"
    );
}

#[test]
fn test_h24_h25_stale_observations_pause_timer_without_resetting_or_intervening() {
    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();
    let wf_id = WorkflowId::new("wf-study");
    let workflow = Workflow::new(wf_id.clone(), "Study").with_rule(WorkflowRule {
        id: RuleId::new("rule-code"),
        workflow_id: wf_id,
        description: "Allow Code".into(),
        scope: RuleScope::Application,
        matcher: RuleMatcher::ApplicationName("code.exe".into()),
        effect: RuleEffect::Allow,
        priority_weight: 10,
        custom_grace_seconds: None,
        is_enabled: true,
        expires_at: None,
    });

    // 1. Initial drift into Discord starts grace timer (30s)
    let obs_discord = sample_observation("discord.exe", "Discord", None);
    let dec1 = engine.evaluate(&obs_discord, &workflow, &mut timer, now);
    assert!(dec1.is_violation());
    assert!(timer.is_active());
    let ep_id = timer.current_episode_id();

    // 2. Stale observation arrives (H-24, H-25)
    let mut obs_stale = sample_observation("discord.exe", "Discord", None);
    obs_stale.window_freshness = FreshnessState::Stale;
    let dec_stale = engine.evaluate(&obs_stale, &workflow, &mut timer, now);

    // Must return ObservationUnavailable and PAUSE timer, NOT cancel/reset it!
    assert!(dec_stale.is_unavailable());
    assert!(
        timer.is_active(),
        "Timer must remain active (paused) during stale observation"
    );
    assert_eq!(
        timer.current_episode_id(),
        ep_id,
        "Episode ID must be preserved"
    );

    // 3. Resuming fresh violation continues the same episode without reset
    let dec3 = engine.evaluate(&obs_discord, &workflow, &mut timer, now);
    assert!(dec3.is_violation());
    assert_eq!(timer.current_episode_id(), ep_id);

    // 4. Stale browser tab in browser window also pauses policy (H-25)
    let mut obs_browser_stale =
        sample_observation("chrome.exe", "Google Chrome", Some("https://reddit.com"));
    obs_browser_stale.browser_freshness = FreshnessState::Stale;
    let dec_b_stale = engine.evaluate(&obs_browser_stale, &workflow, &mut timer, now);
    assert!(dec_b_stale.is_unavailable());
}

#[test]
fn test_h26_browser_application_gating_only_checks_active_allow_domain_rules() {
    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();
    let wf_id = WorkflowId::new("wf-research");

    // Workflow has:
    // - Chrome application -> Allow
    // - reddit.com -> Restrict (Domain scope)
    // There are NO active Allow domain rules!
    let workflow = Workflow::new(wf_id.clone(), "Research")
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-chrome"),
            workflow_id: wf_id.clone(),
            description: "Allow Chrome".into(),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName("chrome.exe".into()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        })
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-restrict-reddit"),
            workflow_id: wf_id,
            description: "Restrict Reddit".into(),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern("reddit.com".into()),
            effect: RuleEffect::Restrict,
            priority_weight: 10,
            custom_grace_seconds: Some(15),
            is_enabled: true,
            expires_at: None,
        });

    // 1. Browsing wikipedia.org in Chrome:
    // With H-26 fixed, the presence of reddit.com -> Restrict does NOT turn Chrome into a broken allowlist!
    // Wikipedia in Chrome is allowed by the Chrome application rule.
    let obs_wiki = sample_observation(
        "chrome.exe",
        "Wikipedia, the free encyclopedia - Google Chrome",
        Some("https://en.wikipedia.org/wiki/Rust"),
    );
    let dec_wiki = engine.evaluate(&obs_wiki, &workflow, &mut timer, now);
    assert!(
        dec_wiki.is_allowed(),
        "Wikipedia must be allowed under Chrome when only Restrict domain rules exist"
    );

    // 2. Browsing reddit.com in Chrome:
    // Domain rule (Restrict, scope 400) beats Application rule (Allow, scope 200)!
    let obs_reddit = sample_observation(
        "chrome.exe",
        "Reddit - Dive into anything - Google Chrome",
        Some("https://www.reddit.com/r/rust"),
    );
    let dec_reddit = engine.evaluate(&obs_reddit, &workflow, &mut timer, now);
    assert!(
        dec_reddit.is_violation(),
        "Reddit must be restricted because Domain scope beats Application scope"
    );
}

#[test]
fn test_h27_short_form_media_policy_precedence_user_allow_exception_overrides() {
    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();
    let wf_id = WorkflowId::new("wf-study");

    // Workflow with an explicit user ExactResource / Canonical Allow exception
    // for a specific educational YouTube Short!
    let allowed_video_id = "dQw4w9WgXcQ";
    let workflow = Workflow::new(wf_id.clone(), "Study").with_rule(WorkflowRule {
        id: RuleId::new("rule-allow-educational-short"),
        workflow_id: wf_id,
        description: "Allow Specific Educational Short".into(),
        scope: RuleScope::ExactResource,
        matcher: RuleMatcher::Canonical(CanonicalResource::YouTubeVideo {
            video_id: allowed_video_id.into(),
        }),
        effect: RuleEffect::Allow,
        priority_weight: 100,
        custom_grace_seconds: None,
        is_enabled: true,
        expires_at: None,
    });

    // 1. User visits the specifically allowed Short -> Explicit user Allow rule must override! (H-27)
    let allowed_url = format!("https://www.youtube.com/shorts/{allowed_video_id}");
    let obs_allowed_short = sample_observation(
        "chrome.exe",
        "Educational Short - YouTube",
        Some(&allowed_url),
    );
    let dec_allowed = engine.evaluate(&obs_allowed_short, &workflow, &mut timer, now);
    assert!(
        dec_allowed.is_allowed(),
        "Explicit user ExactResource Allow rule must override short-form restriction"
    );

    // 2. User visits another unallowed YouTube Short -> Restrict shorts rule applies
    let obs_unallowed_short = sample_observation(
        "chrome.exe",
        "Viral Meme #shorts - YouTube",
        Some("https://www.youtube.com/shorts/random12345"),
    );
    let dec_unallowed = engine.evaluate(&obs_unallowed_short, &workflow, &mut timer, now);
    assert!(
        dec_unallowed.is_violation(),
        "Generic YouTube Short without user exception must be restricted"
    );
}

#[test]
fn test_h17_flowguard_control_surface_neutral() {
    let engine = RuleEngine::new();
    let mut timer = GracePeriodTimer::new();
    let now = Utc::now();
    let wf_id = WorkflowId::new("wf-strict");
    let workflow = Workflow::new(wf_id, "Strict Deny-All"); // default is Restrict

    // FlowGuard desktop window or localhost URL must be neutral (Ignored, pauses timer)
    let obs_fg = sample_observation("flowguard.exe", "FlowGuard — Focus Session", None);
    let dec = engine.evaluate(&obs_fg, &workflow, &mut timer, now);
    assert!(
        matches!(dec, PolicyDecision::Ignored { reason } if reason.contains("FlowGuard control surface")),
        "FlowGuard control surface must be evaluated as neutral Ignored"
    );
}
