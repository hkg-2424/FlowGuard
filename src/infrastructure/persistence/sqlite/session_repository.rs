use chrono::{DateTime, Utc};
use rusqlite::params;

use super::connection::SqliteDb;
use crate::application::ports::{RepositoryError, SessionRepository};
use crate::domain::workflow::{
    CommitmentContract, DriftEvent, DriftResolution, SessionId, SessionState, WorkflowId,
    WorkflowSession,
};

#[derive(Clone)]
pub struct SqliteSessionRepository {
    db: SqliteDb,
}

impl SqliteSessionRepository {
    pub fn new(db: SqliteDb) -> Self {
        Self { db }
    }
}

impl SessionRepository for SqliteSessionRepository {
    fn get(&self, id: &SessionId) -> Result<Option<WorkflowSession>, RepositoryError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut stmt = conn
            .prepare(
                "SELECT id, workflow_id, state, commitment_contract,
                        start_time, end_time, early_exit_reason,
                        total_focus_seconds, total_grace_seconds,
                        total_distraction_seconds, total_paused_seconds,
                        total_afk_seconds, pull_back_count, intervention_count
                 FROM sessions
                 WHERE id = ?1",
            )
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut rows = stmt
            .query(params![id.0])
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let row = match rows
            .next()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?
        {
            Some(r) => r,
            None => return Ok(None),
        };

        let sess_id: String = row
            .get(0)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let workflow_id_str: String = row
            .get(1)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let state_str: String = row
            .get(2)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let contract_json: String = row
            .get(3)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let start_time_str: String = row
            .get(4)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let end_time_str: Option<String> = row
            .get(5)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let early_exit_reason: Option<String> = row
            .get(6)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let total_focus_seconds: u64 = row
            .get(7)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let total_grace_seconds: u64 = row
            .get(8)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let total_distraction_seconds: u64 = row
            .get(9)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let total_paused_seconds: u64 = row
            .get(10)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let total_afk_seconds: u64 = row
            .get(11)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let pull_back_count: u32 = row
            .get(12)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let intervention_count: u32 = row
            .get(13)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let state = match state_str.as_str() {
            "Ready" => SessionState::Ready,
            "Active" => SessionState::Active,
            "Paused" => SessionState::Paused,
            "Completed" => SessionState::Completed,
            "Cancelled" => SessionState::Cancelled,
            "Interrupted" => SessionState::Interrupted,
            invalid => {
                return Err(RepositoryError::CorruptData(format!(
                    "Invalid session state in database for session '{}': {}",
                    sess_id, invalid
                )));
            }
        };

        let commitment_contract: CommitmentContract = serde_json::from_str(&contract_json)
            .map_err(|e| RepositoryError::Serialization(e.to_string()))?;

        let start_time = DateTime::parse_from_rfc3339(&start_time_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                RepositoryError::CorruptData(format!(
                    "Invalid RFC3339 start_time '{}' for session '{}': {}",
                    start_time_str, sess_id, e
                ))
            })?;

        let end_time = match end_time_str {
            Some(ref s) => {
                let dt = DateTime::parse_from_rfc3339(s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| {
                        RepositoryError::CorruptData(format!(
                            "Invalid RFC3339 end_time '{}' for session '{}': {}",
                            s, sess_id, e
                        ))
                    })?;
                Some(dt)
            }
            None => None,
        };

        Ok(Some(WorkflowSession {
            id: SessionId::new(sess_id),
            workflow_id: WorkflowId::new(workflow_id_str),
            state,
            commitment_contract,
            start_time,
            end_time,
            early_exit_reason,
            total_focus_seconds,
            total_grace_seconds,
            total_distraction_seconds,
            total_paused_seconds,
            total_afk_seconds,
            pull_back_count,
            intervention_count,
        }))
    }

    fn save(&mut self, session: &WorkflowSession) -> Result<(), RepositoryError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let contract_json = serde_json::to_string(&session.commitment_contract)
            .map_err(|e| RepositoryError::Serialization(e.to_string()))?;
        let state_str = format!("{:?}", session.state);
        let end_time_str = session.end_time.map(|dt| dt.to_rfc3339());

        conn.execute(
            "INSERT INTO sessions (
                id, workflow_id, state, commitment_contract,
                start_time, end_time, early_exit_reason,
                total_focus_seconds, total_grace_seconds,
                total_distraction_seconds, total_paused_seconds,
                total_afk_seconds, pull_back_count, intervention_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ON CONFLICT(id) DO UPDATE SET
                state = excluded.state,
                end_time = excluded.end_time,
                early_exit_reason = excluded.early_exit_reason,
                total_focus_seconds = excluded.total_focus_seconds,
                total_grace_seconds = excluded.total_grace_seconds,
                total_distraction_seconds = excluded.total_distraction_seconds,
                total_paused_seconds = excluded.total_paused_seconds,
                total_afk_seconds = excluded.total_afk_seconds,
                pull_back_count = excluded.pull_back_count,
                intervention_count = excluded.intervention_count",
            params![
                session.id.0,
                session.workflow_id.0,
                state_str,
                contract_json,
                session.start_time.to_rfc3339(),
                end_time_str,
                session.early_exit_reason,
                session.total_focus_seconds,
                session.total_grace_seconds,
                session.total_distraction_seconds,
                session.total_paused_seconds,
                session.total_afk_seconds,
                session.pull_back_count,
                session.intervention_count,
            ],
        )
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(())
    }

    fn record_drift(&mut self, drift: &DriftEvent) -> Result<(), RepositoryError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let res_json = serde_json::to_string(&drift.resolution)
            .map_err(|e| RepositoryError::Serialization(e.to_string()))?;

        conn.execute(
            "INSERT INTO drift_events (session_id, target_name, duration_seconds, resolution, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                drift.session_id.0,
                drift.target_name,
                drift.duration_seconds,
                res_json,
                drift.timestamp.to_rfc3339(),
            ],
        )
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(())
    }

    fn list_drifts(&self, session_id: &SessionId) -> Result<Vec<DriftEvent>, RepositoryError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut stmt = conn
            .prepare(
                "SELECT session_id, target_name, duration_seconds, resolution, timestamp
                 FROM drift_events
                 WHERE session_id = ?1
                 ORDER BY id ASC",
            )
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(params![session_id.0], |row| {
                let s_id: String = row.get(0)?;
                let target_name: String = row.get(1)?;
                let duration_seconds: u32 = row.get(2)?;
                let res_json: String = row.get(3)?;
                let timestamp_str: String = row.get(4)?;

                Ok((s_id, target_name, duration_seconds, res_json, timestamp_str))
            })
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut drifts = Vec::new();
        for r in rows {
            let (s_id, target_name, duration_seconds, res_json, timestamp_str) =
                r.map_err(|e| RepositoryError::Storage(e.to_string()))?;

            let resolution: DriftResolution = serde_json::from_str(&res_json)
                .map_err(|e| RepositoryError::Serialization(e.to_string()))?;

            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| {
                    RepositoryError::CorruptData(format!(
                        "Invalid RFC3339 timestamp '{}' for drift event in session '{}': {}",
                        timestamp_str, s_id, e
                    ))
                })?;

            drifts.push(DriftEvent {
                session_id: SessionId::new(s_id),
                target_name,
                duration_seconds,
                resolution,
                timestamp,
            });
        }

        Ok(drifts)
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<WorkflowSession>, RepositoryError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut stmt = conn
            .prepare(
                "SELECT id, workflow_id, state, commitment_contract,
                        start_time, end_time, early_exit_reason,
                        total_focus_seconds, total_grace_seconds,
                        total_distraction_seconds, total_paused_seconds,
                        total_afk_seconds, pull_back_count, intervention_count
                 FROM sessions
                 ORDER BY start_time DESC
                 LIMIT ?1",
            )
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                let sess_id: String = row.get(0)?;
                let workflow_id_str: String = row.get(1)?;
                let state_str: String = row.get(2)?;
                let contract_json: String = row.get(3)?;
                let start_time_str: String = row.get(4)?;
                let end_time_str: Option<String> = row.get(5)?;
                let early_exit_reason: Option<String> = row.get(6)?;
                let total_focus_seconds: u64 = row.get(7)?;
                let total_grace_seconds: u64 = row.get(8)?;
                let total_distraction_seconds: u64 = row.get(9)?;
                let total_paused_seconds: u64 = row.get(10)?;
                let total_afk_seconds: u64 = row.get(11)?;
                let pull_back_count: u32 = row.get(12)?;
                let intervention_count: u32 = row.get(13)?;

                Ok((
                    sess_id,
                    workflow_id_str,
                    state_str,
                    contract_json,
                    start_time_str,
                    end_time_str,
                    early_exit_reason,
                    total_focus_seconds,
                    total_grace_seconds,
                    total_distraction_seconds,
                    total_paused_seconds,
                    total_afk_seconds,
                    pull_back_count,
                    intervention_count,
                ))
            })
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut sessions = Vec::new();
        for r in rows {
            let (
                sess_id,
                workflow_id_str,
                state_str,
                contract_json,
                start_time_str,
                end_time_str,
                early_exit_reason,
                total_focus_seconds,
                total_grace_seconds,
                total_distraction_seconds,
                total_paused_seconds,
                total_afk_seconds,
                pull_back_count,
                intervention_count,
            ) = r.map_err(|e| RepositoryError::Storage(e.to_string()))?;

            let state = match state_str.as_str() {
                "Ready" => SessionState::Ready,
                "Active" => SessionState::Active,
                "Paused" => SessionState::Paused,
                "Completed" => SessionState::Completed,
                "Cancelled" => SessionState::Cancelled,
                "Interrupted" => SessionState::Interrupted,
                invalid => {
                    return Err(RepositoryError::CorruptData(format!(
                        "Invalid session state in database for session '{}': {}",
                        sess_id, invalid
                    )));
                }
            };

            let commitment_contract: CommitmentContract = serde_json::from_str(&contract_json)
                .map_err(|e| RepositoryError::Serialization(e.to_string()))?;

            let start_time = DateTime::parse_from_rfc3339(&start_time_str)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| {
                    RepositoryError::CorruptData(format!(
                        "Invalid RFC3339 start_time '{}' for session '{}': {}",
                        start_time_str, sess_id, e
                    ))
                })?;

            let end_time = match end_time_str {
                Some(ref s) => {
                    let dt = DateTime::parse_from_rfc3339(s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .map_err(|e| {
                            RepositoryError::CorruptData(format!(
                                "Invalid RFC3339 end_time '{}' for session '{}': {}",
                                s, sess_id, e
                            ))
                        })?;
                    Some(dt)
                }
                None => None,
            };

            sessions.push(WorkflowSession {
                id: SessionId::new(sess_id),
                workflow_id: WorkflowId::new(workflow_id_str),
                state,
                commitment_contract,
                start_time,
                end_time,
                early_exit_reason,
                total_focus_seconds,
                total_grace_seconds,
                total_distraction_seconds,
                total_paused_seconds,
                total_afk_seconds,
                pull_back_count,
                intervention_count,
            });
        }

        Ok(sessions)
    }

    fn recover_interrupted_sessions(&mut self) -> Result<usize, RepositoryError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let now_str = Utc::now().to_rfc3339();
        let affected = conn
            .execute(
                "UPDATE sessions SET state = 'Interrupted', end_time = ?1 WHERE state IN ('Active', 'Paused')",
                params![now_str],
            )
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(affected)
    }
}
