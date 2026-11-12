use rusqlite::{Connection, Result};
use tracing::info;

const MIGRATIONS: &[&str] = &[
    // Migration 1: Initial schema for workflows, rules, sessions, and drift events
    r#"
    CREATE TABLE IF NOT EXISTS workflows (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        description TEXT NOT NULL,
        target_duration_minutes INTEGER,
        default_grace_period_seconds INTEGER NOT NULL,
        commitment_contract TEXT NOT NULL,
        default_effect TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS workflow_rules (
        id TEXT PRIMARY KEY NOT NULL,
        workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
        description TEXT NOT NULL,
        scope INTEGER NOT NULL,
        matcher TEXT NOT NULL,
        effect TEXT NOT NULL,
        priority_weight INTEGER NOT NULL,
        custom_grace_seconds INTEGER,
        is_enabled INTEGER NOT NULL DEFAULT 1,
        expires_at TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_workflow_rules_wf ON workflow_rules(workflow_id);

    CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY NOT NULL,
        workflow_id TEXT NOT NULL,
        state TEXT NOT NULL,
        commitment_contract TEXT NOT NULL,
        start_time TEXT NOT NULL,
        end_time TEXT,
        early_exit_reason TEXT,
        total_focus_seconds INTEGER NOT NULL DEFAULT 0,
        total_grace_seconds INTEGER NOT NULL DEFAULT 0,
        total_distraction_seconds INTEGER NOT NULL DEFAULT 0,
        total_paused_seconds INTEGER NOT NULL DEFAULT 0,
        total_afk_seconds INTEGER NOT NULL DEFAULT 0,
        pull_back_count INTEGER NOT NULL DEFAULT 0,
        intervention_count INTEGER NOT NULL DEFAULT 0
    );

    CREATE INDEX IF NOT EXISTS idx_sessions_wf ON sessions(workflow_id);
    CREATE INDEX IF NOT EXISTS idx_sessions_state ON sessions(state);

    CREATE TABLE IF NOT EXISTS drift_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        target_name TEXT NOT NULL,
        duration_seconds INTEGER NOT NULL,
        resolution TEXT NOT NULL,
        timestamp TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_drift_session ON drift_events(session_id);
    "#,
];

/// Applies all pending schema migrations inside an atomic transaction.
pub fn apply_migrations(conn: &mut Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            applied_at TEXT NOT NULL
        );
        "#,
    )?;

    let current_version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;

    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let version = (idx + 1) as i64;
        if version > current_version {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, datetime('now'))",
                [version],
            )?;
            tx.commit()?;
            info!("Applied FlowGuard SQLite database migration v{}", version);
        }
    }

    Ok(())
}
