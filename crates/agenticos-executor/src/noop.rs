use std::time::{SystemTime, UNIX_EPOCH};
use std::time::Instant;

use agenticos_application::AppError;
use agenticos_domain::{
    ActionId, ActionResult, ActionStatus, ApprovedAction, RollbackToken,
};

use crate::traits::{ApprovedActionExecutor, RollbackManager};

/// No-op executor for non-Linux platforms. Reports all actions as DryRun.
pub struct NoopExecutor;

impl NoopExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl ApprovedActionExecutor for NoopExecutor {
    fn execute(&self, action: ApprovedAction) -> Result<ActionResult, AppError> {
        let start = Instant::now();
        let now = timestamp();
        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ActionResult {
            action_id: action.request.id.clone(),
            status: ActionStatus::DryRun,
            message: format!("noop (non-Linux): {:?}", action.request.kind),
            executed_at: now,
            duration_ms,
            rollback: None,
        })
    }
}

pub struct NoopRollbackManager;

impl NoopRollbackManager {
    pub fn new() -> Self {
        Self
    }
}

impl RollbackManager for NoopRollbackManager {
    fn rollback(&self, _token: RollbackToken) -> Result<ActionResult, AppError> {
        let now = timestamp();
        Ok(ActionResult {
            action_id: ActionId::from("rollback"),
            status: ActionStatus::DryRun,
            message: "noop rollback (non-Linux)".into(),
            executed_at: now,
            duration_ms: 0,
            rollback: None,
        })
    }
}

fn timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()),
        Err(_) => "0.000000000Z".to_owned(),
    }
}
