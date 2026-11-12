use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::application::ports::{RepositoryError, SessionRepository, WorkflowRepository};
use crate::domain::workflow::{DriftEvent, SessionId, Workflow, WorkflowId, WorkflowSession};

/// Fast, thread-safe, in-memory implementation of [`WorkflowRepository`].
///
/// Ideal for unit testing, interactive CLI sessions, and ephemeral execution.
#[derive(Debug, Default, Clone)]
pub struct InMemoryWorkflowRepository {
    workflows: Arc<Mutex<HashMap<WorkflowId, Workflow>>>,
}

impl InMemoryWorkflowRepository {
    pub fn new() -> Self {
        Self {
            workflows: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl WorkflowRepository for InMemoryWorkflowRepository {
    fn get(&self, id: &WorkflowId) -> Result<Option<Workflow>, RepositoryError> {
        let lock = self
            .workflows
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(lock.get(id).cloned())
    }

    fn save(&mut self, workflow: &Workflow) -> Result<(), RepositoryError> {
        let mut lock = self
            .workflows
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        lock.insert(workflow.id.clone(), workflow.clone());
        Ok(())
    }

    fn list(&self) -> Result<Vec<Workflow>, RepositoryError> {
        let lock = self
            .workflows
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let mut list: Vec<Workflow> = lock.values().cloned().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }

    fn delete(&mut self, id: &WorkflowId) -> Result<bool, RepositoryError> {
        let mut lock = self
            .workflows
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(lock.remove(id).is_some())
    }
}

/// Fast, thread-safe, in-memory implementation of [`SessionRepository`].
#[derive(Debug, Default, Clone)]
pub struct InMemorySessionRepository {
    sessions: Arc<Mutex<HashMap<SessionId, WorkflowSession>>>,
    drifts: Arc<Mutex<HashMap<SessionId, Vec<DriftEvent>>>>,
}

impl InMemorySessionRepository {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            drifts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl SessionRepository for InMemorySessionRepository {
    fn get(&self, id: &SessionId) -> Result<Option<WorkflowSession>, RepositoryError> {
        let lock = self
            .sessions
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(lock.get(id).cloned())
    }

    fn save(&mut self, session: &WorkflowSession) -> Result<(), RepositoryError> {
        let mut lock = self
            .sessions
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        lock.insert(session.id.clone(), session.clone());
        Ok(())
    }

    fn record_drift(&mut self, drift: &DriftEvent) -> Result<(), RepositoryError> {
        let mut lock = self
            .drifts
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        lock.entry(drift.session_id.clone())
            .or_default()
            .push(drift.clone());
        Ok(())
    }

    fn list_drifts(&self, session_id: &SessionId) -> Result<Vec<DriftEvent>, RepositoryError> {
        let lock = self
            .drifts
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(lock.get(session_id).cloned().unwrap_or_default())
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<WorkflowSession>, RepositoryError> {
        let lock = self
            .sessions
            .lock()
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let mut list: Vec<WorkflowSession> = lock.values().cloned().collect();
        list.sort_by_key(|a| std::cmp::Reverse(a.start_time));
        list.truncate(limit);
        Ok(list)
    }
}
