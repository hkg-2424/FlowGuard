use chrono::{DateTime, Utc};
use rusqlite::params;

use super::connection::SqliteDb;
use crate::application::ports::{RepositoryError, WorkflowRepository};
use crate::domain::workflow::{
    CommitmentContract, RuleEffect, RuleId, RuleMatcher, RuleScope, Workflow, WorkflowId,
    WorkflowRule,
};

#[derive(Clone)]
pub struct SqliteWorkflowRepository {
    db: SqliteDb,
}

impl SqliteWorkflowRepository {
    pub fn new(db: SqliteDb) -> Self {
        Self { db }
    }

    fn load_rules(
        conn: &rusqlite::Connection,
        workflow_id: &WorkflowId,
    ) -> Result<Vec<WorkflowRule>, RepositoryError> {
        let mut stmt = conn
            .prepare(
                "SELECT id, description, scope, matcher, effect, priority_weight,
                        custom_grace_seconds, is_enabled, expires_at
                 FROM workflow_rules
                 WHERE workflow_id = ?1
                 ORDER BY priority_weight DESC",
            )
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let rule_rows = stmt
            .query_map(params![workflow_id.0], |row| {
                let id: String = row.get(0)?;
                let description: String = row.get(1)?;
                let scope_val: u32 = row.get(2)?;
                let matcher_json: String = row.get(3)?;
                let effect_str: String = row.get(4)?;
                let priority_weight: u32 = row.get(5)?;
                let custom_grace_seconds: Option<u32> = row.get(6)?;
                let is_enabled: bool = row.get(7)?;
                let expires_at_str: Option<String> = row.get(8)?;

                Ok((
                    id,
                    description,
                    scope_val,
                    matcher_json,
                    effect_str,
                    priority_weight,
                    custom_grace_seconds,
                    is_enabled,
                    expires_at_str,
                ))
            })
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut rules = Vec::new();
        for res in rule_rows {
            let (
                id,
                description,
                scope_val,
                matcher_json,
                effect_str,
                priority_weight,
                custom_grace_seconds,
                is_enabled,
                expires_at_str,
            ) = res.map_err(|e| RepositoryError::Storage(e.to_string()))?;

            let scope = match scope_val {
                0 => RuleScope::GlobalDefault,
                100 => RuleScope::WorkflowDefault,
                200 => RuleScope::Application,
                400 => RuleScope::Domain,
                600 => RuleScope::Origin,
                800 => RuleScope::ExactUrl,
                1000 => RuleScope::ExactResource,
                invalid => {
                    return Err(RepositoryError::CorruptData(format!(
                        "Invalid rule scope in database for rule '{}': {}",
                        id, invalid
                    )));
                }
            };

            let matcher: RuleMatcher = serde_json::from_str(&matcher_json)
                .map_err(|e| RepositoryError::Serialization(e.to_string()))?;

            let effect = match effect_str.as_str() {
                "Allow" => RuleEffect::Allow,
                "Warn" => RuleEffect::Warn,
                "Restrict" => RuleEffect::Restrict,
                "Ignore" => RuleEffect::Ignore,
                "Monitor" => RuleEffect::Monitor,
                invalid => {
                    return Err(RepositoryError::CorruptData(format!(
                        "Invalid rule effect in database for rule '{}': {}",
                        id, invalid
                    )));
                }
            };

            let expires_at = match expires_at_str {
                Some(ref s) => {
                    let dt = DateTime::parse_from_rfc3339(s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .map_err(|e| {
                            RepositoryError::CorruptData(format!(
                                "Invalid RFC3339 expires_at '{}' for rule '{}': {}",
                                s, id, e
                            ))
                        })?;
                    Some(dt)
                }
                None => None,
            };

            rules.push(WorkflowRule {
                id: RuleId::new(id),
                workflow_id: workflow_id.clone(),
                description,
                scope,
                matcher,
                effect,
                priority_weight,
                custom_grace_seconds,
                is_enabled,
                expires_at,
            });
        }

        Ok(rules)
    }
}

impl WorkflowRepository for SqliteWorkflowRepository {
    fn get(&self, id: &WorkflowId) -> Result<Option<Workflow>, RepositoryError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, target_duration_minutes,
                        default_grace_period_seconds, commitment_contract,
                        default_effect, created_at, updated_at
                 FROM workflows
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

        let wf_id: String = row
            .get(0)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let name: String = row
            .get(1)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let description: String = row
            .get(2)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let target_duration_minutes: Option<u32> = row
            .get(3)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let default_grace_period_seconds: u32 = row
            .get(4)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let contract_json: String = row
            .get(5)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let default_effect_str: String = row
            .get(6)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let created_at_str: String = row
            .get(7)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let updated_at_str: String = row
            .get(8)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let commitment_contract: CommitmentContract = serde_json::from_str(&contract_json)
            .map_err(|e| RepositoryError::Serialization(e.to_string()))?;

        let default_effect = match default_effect_str.as_str() {
            "Allow" => RuleEffect::Allow,
            "Warn" => RuleEffect::Warn,
            "Restrict" => RuleEffect::Restrict,
            "Ignore" => RuleEffect::Ignore,
            "Monitor" => RuleEffect::Monitor,
            invalid => {
                return Err(RepositoryError::CorruptData(format!(
                    "Invalid default_effect in database for workflow '{}': {}",
                    wf_id, invalid
                )));
            }
        };

        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                RepositoryError::CorruptData(format!(
                    "Invalid RFC3339 created_at '{}' for workflow '{}': {}",
                    created_at_str, wf_id, e
                ))
            })?;
        let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                RepositoryError::CorruptData(format!(
                    "Invalid RFC3339 updated_at '{}' for workflow '{}': {}",
                    updated_at_str, wf_id, e
                ))
            })?;

        let rules = Self::load_rules(&conn, id)?;

        Ok(Some(Workflow {
            id: WorkflowId::new(wf_id),
            name,
            description,
            target_duration_minutes,
            default_grace_period_seconds,
            commitment_contract,
            rules,
            default_effect,
            created_at,
            updated_at,
        }))
    }

    fn save(&mut self, workflow: &Workflow) -> Result<(), RepositoryError> {
        let mut conn = self
            .db
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let contract_json = serde_json::to_string(&workflow.commitment_contract)
            .map_err(|e| RepositoryError::Serialization(e.to_string()))?;
        let default_effect_str = format!("{:?}", workflow.default_effect);

        tx.execute(
            "INSERT INTO workflows (
                id, name, description, target_duration_minutes,
                default_grace_period_seconds, commitment_contract,
                default_effect, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                target_duration_minutes = excluded.target_duration_minutes,
                default_grace_period_seconds = excluded.default_grace_period_seconds,
                commitment_contract = excluded.commitment_contract,
                default_effect = excluded.default_effect,
                updated_at = excluded.updated_at",
            params![
                workflow.id.0,
                workflow.name,
                workflow.description,
                workflow.target_duration_minutes,
                workflow.default_grace_period_seconds,
                contract_json,
                default_effect_str,
                workflow.created_at.to_rfc3339(),
                workflow.updated_at.to_rfc3339(),
            ],
        )
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        // Re-sync rules atomically: delete existing and re-insert
        tx.execute(
            "DELETE FROM workflow_rules WHERE workflow_id = ?1",
            params![workflow.id.0],
        )
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        for rule in &workflow.rules {
            let matcher_json = serde_json::to_string(&rule.matcher)
                .map_err(|e| RepositoryError::Serialization(e.to_string()))?;
            let effect_str = format!("{:?}", rule.effect);
            let scope_val = rule.scope as u32;
            let expires_at_str = rule.expires_at.map(|dt| dt.to_rfc3339());

            tx.execute(
                "INSERT INTO workflow_rules (
                    id, workflow_id, description, scope, matcher,
                    effect, priority_weight, custom_grace_seconds,
                    is_enabled, expires_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    rule.id.0,
                    workflow.id.0,
                    rule.description,
                    scope_val,
                    matcher_json,
                    effect_str,
                    rule.priority_weight,
                    rule.custom_grace_seconds,
                    rule.is_enabled,
                    expires_at_str,
                ],
            )
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    fn list(&self) -> Result<Vec<Workflow>, RepositoryError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let mut stmt = conn
            .prepare("SELECT id FROM workflows ORDER BY name ASC")
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| RepositoryError::Storage(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        drop(stmt);
        drop(conn);

        let mut list = Vec::new();
        for id in ids {
            if let Some(wf) = self.get(&WorkflowId::new(id))? {
                list.push(wf);
            }
        }

        Ok(list)
    }

    fn delete(&mut self, id: &WorkflowId) -> Result<bool, RepositoryError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let affected = conn
            .execute("DELETE FROM workflows WHERE id = ?1", params![id.0])
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(affected > 0)
    }
}
