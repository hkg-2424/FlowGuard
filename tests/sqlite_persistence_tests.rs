use chrono::Utc;
use flowguard::application::ports::{SessionRepository, WorkflowRepository};
use flowguard::domain::workflow::{
    CanonicalResource, CommitmentContract, DriftEvent, DriftResolution, RuleEffect, RuleId,
    RuleMatcher, RuleScope, SessionId, SessionState, Workflow, WorkflowId, WorkflowRule,
    WorkflowSession,
};
use flowguard::infrastructure::persistence::sqlite::schema::apply_migrations;
use flowguard::infrastructure::persistence::sqlite::{
    SqliteDb, SqliteSessionRepository, SqliteWorkflowRepository,
};

#[test]
fn test_sqlite_migrations_idempotent() {
    let db = SqliteDb::open_in_memory().expect("Failed to create in-memory SQLite DB");
    let mut conn = db.lock().unwrap();

    // Re-running migrations should succeed without error or duplicate table conflicts
    let res = apply_migrations(&mut conn);
    assert!(res.is_ok());
}

#[test]
fn test_sqlite_workflow_crud_and_cascade() {
    let db = SqliteDb::open_in_memory().expect("Failed to create in-memory SQLite DB");
    let mut repo = SqliteWorkflowRepository::new(db.clone());

    let wf_id = WorkflowId::new("wf-backend");
    let mut workflow = Workflow::new(wf_id.clone(), "Backend Engineering")
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-ide"),
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
            id: RuleId::new("rule-github-repo"),
            workflow_id: wf_id.clone(),
            description: "Allow Rust repo".into(),
            scope: RuleScope::ExactResource,
            matcher: RuleMatcher::Canonical(CanonicalResource::GitHubRepo {
                owner: "rust-lang".into(),
                repo: "rust".into(),
            }),
            effect: RuleEffect::Allow,
            priority_weight: 100,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        })
        .with_rule(WorkflowRule {
            id: RuleId::new("rule-block-distraction"),
            workflow_id: wf_id.clone(),
            description: "Restrict Twitter".into(),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern("x.com".into()),
            effect: RuleEffect::Restrict,
            priority_weight: 50,
            custom_grace_seconds: Some(20),
            is_enabled: true,
            expires_at: None,
        });

    workflow.commitment_contract = CommitmentContract::AccountableBreakable {
        min_reason_characters: 25,
    };
    workflow.target_duration_minutes = Some(90);

    // 1. Save workflow
    repo.save(&workflow).expect("Failed to save workflow");

    // 2. Retrieve workflow and verify fields & rules
    let retrieved = repo
        .get(&wf_id)
        .expect("Failed to get workflow")
        .expect("Workflow not found");
    assert_eq!(retrieved.id, wf_id);
    assert_eq!(retrieved.name, "Backend Engineering");
    assert_eq!(retrieved.target_duration_minutes, Some(90));
    assert_eq!(retrieved.default_grace_period_seconds, 30);
    assert_eq!(
        retrieved.commitment_contract,
        CommitmentContract::AccountableBreakable {
            min_reason_characters: 25,
        }
    );
    assert_eq!(retrieved.rules.len(), 3);

    // Rules should be ordered by priority_weight DESC
    assert_eq!(retrieved.rules[0].id.0, "rule-github-repo");
    assert_eq!(retrieved.rules[0].scope, RuleScope::ExactResource);
    assert_eq!(
        retrieved.rules[0].matcher,
        RuleMatcher::Canonical(CanonicalResource::GitHubRepo {
            owner: "rust-lang".into(),
            repo: "rust".into(),
        })
    );
    assert_eq!(retrieved.rules[1].id.0, "rule-block-distraction");
    assert_eq!(retrieved.rules[1].custom_grace_seconds, Some(20));
    assert_eq!(retrieved.rules[2].id.0, "rule-ide");

    // 3. List workflows
    let list = repo.list().expect("Failed to list workflows");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, wf_id);

    // 4. Update workflow (remove 1 rule, change name)
    let mut updated_wf = retrieved;
    updated_wf.name = "Full-Stack Development".into();
    updated_wf.rules.pop(); // remove rule-ide
    repo.save(&updated_wf).expect("Failed to update workflow");

    let retrieved_updated = repo.get(&wf_id).unwrap().unwrap();
    assert_eq!(retrieved_updated.name, "Full-Stack Development");
    assert_eq!(retrieved_updated.rules.len(), 2);

    // 5. Delete workflow (CASCADE should clean up rules)
    let deleted = repo.delete(&wf_id).expect("Failed to delete workflow");
    assert!(deleted);
    assert!(repo.get(&wf_id).unwrap().is_none());

    let conn = db.lock().unwrap();
    let remaining_rules: i64 = conn
        .query_row("SELECT count(*) FROM workflow_rules", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        remaining_rules, 0,
        "Foreign key cascade failed to delete orphan rules"
    );
}

#[test]
fn test_sqlite_session_and_drift_logging() {
    let db = SqliteDb::open_in_memory().expect("Failed to create in-memory SQLite DB");
    let mut session_repo = SqliteSessionRepository::new(db.clone());

    let session_id = SessionId::new("sess_test_123");
    let workflow_id = WorkflowId::new("wf_study");
    let mut session = WorkflowSession::new(
        session_id.clone(),
        workflow_id.clone(),
        CommitmentContract::Unbreakable {
            target_duration_seconds: 3600,
        },
    );

    session.start().unwrap();
    session.total_focus_seconds = 1200;
    session.total_grace_seconds = 45;
    session.total_distraction_seconds = 60;
    session.pull_back_count = 2;
    session.intervention_count = 1;

    // 1. Save session
    session_repo.save(&session).expect("Failed to save session");

    // 2. Retrieve session
    let retrieved = session_repo
        .get(&session_id)
        .unwrap()
        .expect("Session not found");
    assert_eq!(retrieved.id, session_id);
    assert_eq!(retrieved.workflow_id, workflow_id);
    assert_eq!(retrieved.state, SessionState::Active);
    assert_eq!(retrieved.total_focus_seconds, 1200);
    assert_eq!(retrieved.total_grace_seconds, 45);
    assert_eq!(retrieved.total_distraction_seconds, 60);
    assert_eq!(retrieved.pull_back_count, 2);
    assert_eq!(retrieved.intervention_count, 1);
    assert_eq!(
        retrieved.commitment_contract,
        CommitmentContract::Unbreakable {
            target_duration_seconds: 3600,
        }
    );

    // 3. Record drift events (self-corrected pull-back & expired intervention)
    let now = Utc::now();
    let drift1 = DriftEvent {
        session_id: session_id.clone(),
        target_name: "reddit.com".into(),
        duration_seconds: 14,
        resolution: DriftResolution::SelfCorrected {
            recovery_seconds: 14,
        },
        timestamp: now,
    };
    let drift2 = DriftEvent {
        session_id: session_id.clone(),
        target_name: "youtube.com".into(),
        duration_seconds: 35,
        resolution: DriftResolution::Intervened,
        timestamp: now,
    };

    session_repo
        .record_drift(&drift1)
        .expect("Failed to record drift 1");
    session_repo
        .record_drift(&drift2)
        .expect("Failed to record drift 2");

    // 4. Retrieve drift list
    let drifts = session_repo
        .list_drifts(&session_id)
        .expect("Failed to list drifts");
    assert_eq!(drifts.len(), 2);
    assert_eq!(drifts[0].target_name, "reddit.com");
    assert_eq!(
        drifts[0].resolution,
        DriftResolution::SelfCorrected {
            recovery_seconds: 14,
        }
    );
    assert_eq!(drifts[1].target_name, "youtube.com");
    assert_eq!(drifts[1].resolution, DriftResolution::Intervened);

    // 5. Update session to completed
    session.complete().unwrap();
    session_repo.save(&session).unwrap();

    let completed_session = session_repo.get(&session_id).unwrap().unwrap();
    assert_eq!(completed_session.state, SessionState::Completed);
    assert!(completed_session.end_time.is_some());
}

#[test]
fn test_h11_corrupt_enum_and_timestamp_validation() {
    use flowguard::application::ports::RepositoryError;

    let db = SqliteDb::open_in_memory().expect("Failed to create in-memory SQLite DB");
    let session_repo = SqliteSessionRepository::new(db.clone());
    let workflow_repo = SqliteWorkflowRepository::new(db.clone());

    // 1. Corrupt session state
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, workflow_id, state, commitment_contract, start_time)
             VALUES ('sess_corrupt_state', 'wf1', 'InvalidState', '\"Standard\"', '2026-11-20T12:00:00Z')",
            [],
        ).unwrap();
    }
    let res = session_repo.get(&SessionId::new("sess_corrupt_state"));
    assert!(
        matches!(res, Err(RepositoryError::CorruptData(_))),
        "Expected CorruptData for invalid state, got: {:?}",
        res
    );

    // 2. Corrupt session timestamp
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, workflow_id, state, commitment_contract, start_time)
             VALUES ('sess_corrupt_time', 'wf1', 'Active', '\"Standard\"', 'not-a-valid-timestamp')",
            [],
        ).unwrap();
    }
    let res = session_repo.get(&SessionId::new("sess_corrupt_time"));
    assert!(
        matches!(res, Err(RepositoryError::CorruptData(_))),
        "Expected CorruptData for invalid start_time, got: {:?}",
        res
    );

    // 3. Corrupt workflow rule scope
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO workflows (id, name, description, default_grace_period_seconds, commitment_contract, default_effect, created_at, updated_at)
             VALUES ('wf_corrupt_scope', 'WF', 'Desc', 30, '\"Standard\"', 'Allow', '2026-11-20T12:00:00Z', '2026-11-20T12:00:00Z')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO workflow_rules (id, workflow_id, description, scope, matcher, effect, priority_weight)
             VALUES ('rule_corrupt_scope', 'wf_corrupt_scope', 'Desc', 9999, '{\"ApplicationName\":\"app.exe\"}', 'Allow', 10)",
            [],
        ).unwrap();
    }
    let res = workflow_repo.get(&WorkflowId::new("wf_corrupt_scope"));
    assert!(
        matches!(res, Err(RepositoryError::CorruptData(_))),
        "Expected CorruptData for invalid rule scope, got: {:?}",
        res
    );

    // 4. Corrupt workflow rule effect
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO workflows (id, name, description, default_grace_period_seconds, commitment_contract, default_effect, created_at, updated_at)
             VALUES ('wf_corrupt_effect', 'WF', 'Desc', 30, '\"Standard\"', 'Allow', '2026-11-20T12:00:00Z', '2026-11-20T12:00:00Z')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO workflow_rules (id, workflow_id, description, scope, matcher, effect, priority_weight)
             VALUES ('rule_corrupt_effect', 'wf_corrupt_effect', 'Desc', 200, '{\"ApplicationName\":\"app.exe\"}', 'InvalidEffect', 10)",
            [],
        ).unwrap();
    }
    let res = workflow_repo.get(&WorkflowId::new("wf_corrupt_effect"));
    assert!(
        matches!(res, Err(RepositoryError::CorruptData(_))),
        "Expected CorruptData for invalid rule effect, got: {:?}",
        res
    );

    // 5. Corrupt rule expires_at
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO workflows (id, name, description, default_grace_period_seconds, commitment_contract, default_effect, created_at, updated_at)
             VALUES ('wf_corrupt_exp', 'WF', 'Desc', 30, '\"Standard\"', 'Allow', '2026-11-20T12:00:00Z', '2026-11-20T12:00:00Z')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO workflow_rules (id, workflow_id, description, scope, matcher, effect, priority_weight, expires_at)
             VALUES ('rule_corrupt_exp', 'wf_corrupt_exp', 'Desc', 200, '{\"ApplicationName\":\"app.exe\"}', 'Allow', 10, 'corrupt-date')",
            [],
        ).unwrap();
    }
    let res = workflow_repo.get(&WorkflowId::new("wf_corrupt_exp"));
    assert!(
        matches!(res, Err(RepositoryError::CorruptData(_))),
        "Expected CorruptData for invalid rule expires_at, got: {:?}",
        res
    );

    // 6. Corrupt drift event timestamp
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, workflow_id, state, commitment_contract, start_time)
             VALUES ('sess_drift_test', 'wf1', 'Completed', '\"Standard\"', '2026-11-20T12:00:00Z')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO drift_events (session_id, target_name, duration_seconds, resolution, timestamp)
             VALUES ('sess_drift_test', 'bad.com', 10, '\"Intervened\"', 'bogus-timestamp')",
            [],
        ).unwrap();
    }
    let res = session_repo.list_drifts(&SessionId::new("sess_drift_test"));
    assert!(
        matches!(res, Err(RepositoryError::CorruptData(_))),
        "Expected CorruptData for invalid drift timestamp, got: {:?}",
        res
    );
}

#[test]
fn test_h12_crash_recovery_rfc3339() {
    let db = SqliteDb::open_in_memory().expect("Failed to create in-memory SQLite DB");
    let mut session_repo = SqliteSessionRepository::new(db.clone());

    // Insert an active session that did not get closed cleanly
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, workflow_id, state, commitment_contract, start_time)
             VALUES ('sess_unclean_1', 'wf1', 'Active', '\"Standard\"', '2026-11-20T10:00:00Z')",
            [],
        )
        .unwrap();
    }

    // Recover interrupted sessions
    let recovered = session_repo.recover_interrupted_sessions().unwrap();
    assert_eq!(recovered, 1);

    // Load recovered session
    let sess = session_repo
        .get(&SessionId::new("sess_unclean_1"))
        .unwrap()
        .unwrap();
    assert_eq!(sess.state, SessionState::Interrupted);
    assert!(
        sess.end_time.is_some(),
        "Recovered session must have end_time set"
    );

    // The end_time must be readable and parsed successfully as RFC3339
    let end_time = sess.end_time.unwrap();
    let diff = (Utc::now() - end_time).num_seconds().abs();
    assert!(
        diff < 5,
        "Recovered end_time must be close to now, diff was {}s",
        diff
    );
}

#[test]
fn test_h13_single_active_session_constraint() {
    let db = SqliteDb::open_in_memory().expect("Failed to create in-memory SQLite DB");
    let mut session_repo = SqliteSessionRepository::new(db.clone());

    let mut sess1 = WorkflowSession::new(
        SessionId::new("sess_unique_1"),
        WorkflowId::new("wf1"),
        CommitmentContract::Standard,
    );
    sess1.start().unwrap();
    session_repo
        .save(&sess1)
        .expect("Saving first active session must succeed");

    let mut sess2 = WorkflowSession::new(
        SessionId::new("sess_unique_2"),
        WorkflowId::new("wf2"),
        CommitmentContract::Standard,
    );
    sess2.start().unwrap();
    let res = session_repo.save(&sess2);
    assert!(
        res.is_err(),
        "Saving second active session must fail due to single active session constraint: {:?}",
        res
    );
}
