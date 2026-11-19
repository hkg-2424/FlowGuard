use tracing::info;

use crate::application::ports::WorkflowRepository;
use crate::domain::workflow::{
    CommitmentContract, RuleEffect, RuleId, RuleMatcher, RuleScope, Workflow, WorkflowId,
    WorkflowRule,
};
use crate::infrastructure::persistence::SqliteWorkflowRepository;

pub fn seed_default_workflows_if_empty(repo: &mut SqliteWorkflowRepository) {
    let existing = repo.list().unwrap_or_default();
    if !existing.is_empty() {
        return;
    }

    info!("Seeding default workflows for novice user...");

    // 1. Study Workflow
    let study_id = WorkflowId::new("wf-study");
    let mut study = Workflow::new(study_id.clone(), "Study".to_string());
    study.description = "Academic study, reading papers, and exam preparation".to_string();
    study.target_duration_minutes = Some(45);
    study.commitment_contract = CommitmentContract::AccountableBreakable {
        min_reason_characters: 10,
    };
    study.default_grace_period_seconds = 30;

    let study_apps = vec![
        "acrobat.exe",
        "foxitreader.exe",
        "winword.exe",
        "onenote.exe",
        "notepad.exe",
    ];
    for (i, app) in study_apps.into_iter().enumerate() {
        study.rules.push(WorkflowRule {
            id: RuleId::new(format!("study-app-{}", i)),
            workflow_id: study_id.clone(),
            description: format!("Allow {}", app),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName(app.to_string()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });
    }

    let study_domains = vec![
        "wikipedia.org",
        "jstor.org",
        "khanacademy.org",
        "coursera.org",
        "scholar.google.com",
        "ankiweb.net",
    ];
    for (i, dom) in study_domains.into_iter().enumerate() {
        study.rules.push(WorkflowRule {
            id: RuleId::new(format!("study-dom-{}", i)),
            workflow_id: study_id.clone(),
            description: format!("Allow domain {}", dom),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern(dom.to_string()),
            effect: RuleEffect::Allow,
            priority_weight: 20,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });
    }
    let _ = repo.save(&study);

    // 2. Deep Work Workflow
    let deep_id = WorkflowId::new("wf-deep-work");
    let mut deep = Workflow::new(deep_id.clone(), "Deep Work".to_string());
    deep.description = "Uninterrupted creative and strategic thinking".to_string();
    deep.target_duration_minutes = Some(50);
    deep.commitment_contract = CommitmentContract::AccountableBreakable {
        min_reason_characters: 10,
    };
    deep.default_grace_period_seconds = 30;

    let deep_apps = vec![
        "code.exe",
        "antigravity",
        "obsidian.exe",
        "notion.exe",
        "notepad.exe",
        "windowsterminal.exe",
    ];
    for (i, app) in deep_apps.into_iter().enumerate() {
        deep.rules.push(WorkflowRule {
            id: RuleId::new(format!("deep-app-{}", i)),
            workflow_id: deep_id.clone(),
            description: format!("Allow {}", app),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName(app.to_string()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });
    }

    let deep_domains = vec![
        "github.com",
        "docs.google.com",
        "overleaf.com",
        "linear.app",
    ];
    for (i, dom) in deep_domains.into_iter().enumerate() {
        deep.rules.push(WorkflowRule {
            id: RuleId::new(format!("deep-dom-{}", i)),
            workflow_id: deep_id.clone(),
            description: format!("Allow domain {}", dom),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern(dom.to_string()),
            effect: RuleEffect::Allow,
            priority_weight: 20,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });
    }
    let _ = repo.save(&deep);

    // 3. Coding Workflow
    let coding_id = WorkflowId::new("wf-coding");
    let mut coding = Workflow::new(coding_id.clone(), "Coding".to_string());
    coding.description = "Software engineering, terminal, and documentation".to_string();
    coding.target_duration_minutes = Some(25);
    coding.commitment_contract = CommitmentContract::Standard;
    coding.default_grace_period_seconds = 30;

    let coding_apps = vec![
        "code.exe",
        "antigravity",
        "windowsterminal.exe",
        "cmd.exe",
        "powershell.exe",
        "devenv.exe",
        "idea64.exe",
        "notepad.exe",
    ];
    for (i, app) in coding_apps.into_iter().enumerate() {
        coding.rules.push(WorkflowRule {
            id: RuleId::new(format!("coding-app-{}", i)),
            workflow_id: coding_id.clone(),
            description: format!("Allow {}", app),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName(app.to_string()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });
    }

    let coding_domains = vec![
        "github.com",
        "doc.rust-lang.org",
        "docs.rs",
        "stackoverflow.com",
        "crates.io",
        "gitlab.com",
    ];
    for (i, dom) in coding_domains.into_iter().enumerate() {
        coding.rules.push(WorkflowRule {
            id: RuleId::new(format!("coding-dom-{}", i)),
            workflow_id: coding_id.clone(),
            description: format!("Allow domain {}", dom),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern(dom.to_string()),
            effect: RuleEffect::Allow,
            priority_weight: 20,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });
    }
    let _ = repo.save(&coding);

    // 4. Reading Workflow
    let reading_id = WorkflowId::new("wf-reading");
    let mut reading = Workflow::new(reading_id.clone(), "Reading".to_string());
    reading.description = "Articles, technical documentation, and long-form reading".to_string();
    reading.target_duration_minutes = Some(30);
    reading.commitment_contract = CommitmentContract::Standard;
    reading.default_grace_period_seconds = 30;

    let reading_apps = vec!["acrobat.exe", "calibre.exe", "kindle.exe", "notepad.exe"];
    for (i, app) in reading_apps.into_iter().enumerate() {
        reading.rules.push(WorkflowRule {
            id: RuleId::new(format!("reading-app-{}", i)),
            workflow_id: reading_id.clone(),
            description: format!("Allow {}", app),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName(app.to_string()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });
    }

    let reading_domains = vec![
        "medium.com",
        "substack.com",
        "gutenberg.org",
        "archive.org",
        "wikipedia.org",
    ];
    for (i, dom) in reading_domains.into_iter().enumerate() {
        reading.rules.push(WorkflowRule {
            id: RuleId::new(format!("reading-dom-{}", i)),
            workflow_id: reading_id.clone(),
            description: format!("Allow domain {}", dom),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern(dom.to_string()),
            effect: RuleEffect::Allow,
            priority_weight: 20,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });
    }
    let _ = repo.save(&reading);

    // 5. Online Learning & Video Lectures Workflow
    let learning_id = WorkflowId::new("wf-learning");
    let mut learning = Workflow::new(learning_id.clone(), "Online Learning".to_string());
    learning.description =
        "Udemy, Coursera, YouTube lectures, ChatGPT, and documentation".to_string();
    learning.target_duration_minutes = Some(40);
    learning.commitment_contract = CommitmentContract::Standard;
    learning.default_grace_period_seconds = 30;

    let learning_apps = vec![
        "code.exe",
        "antigravity",
        "obsidian.exe",
        "notion.exe",
        "windowsterminal.exe",
        "notepad.exe",
    ];
    for (i, app) in learning_apps.into_iter().enumerate() {
        learning.rules.push(WorkflowRule {
            id: RuleId::new(format!("learning-app-{}", i)),
            workflow_id: learning_id.clone(),
            description: format!("Allow {}", app),
            scope: RuleScope::Application,
            matcher: RuleMatcher::ApplicationName(app.to_string()),
            effect: RuleEffect::Allow,
            priority_weight: 10,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });
    }

    let learning_domains = vec![
        "udemy.com",
        "coursera.org",
        "youtube.com",
        "chatgpt.com",
        "claude.ai",
        "github.com",
        "wikipedia.org",
        "docs.rs",
    ];
    for (i, dom) in learning_domains.into_iter().enumerate() {
        learning.rules.push(WorkflowRule {
            id: RuleId::new(format!("learning-dom-{}", i)),
            workflow_id: learning_id.clone(),
            description: format!("Allow domain {}", dom),
            scope: RuleScope::Domain,
            matcher: RuleMatcher::DomainPattern(dom.to_string()),
            effect: RuleEffect::Allow,
            priority_weight: 20,
            custom_grace_seconds: None,
            is_enabled: true,
            expires_at: None,
        });
    }
    let _ = repo.save(&learning);

    info!("Seeded default workflows: Study, Deep Work, Coding, Reading, Online Learning.");
}
