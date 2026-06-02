use std::time::{SystemTime, UNIX_EPOCH};

use agenticos_application::AppError;
use agenticos_domain::{ActionResult, ActionStatus, ApprovedAction};

use crate::traits::ApprovedActionExecutor;

/// Default executor. Records the action as performed without actually executing it.
/// All mutations are reported as `DryRun`.
pub struct DryRunExecutor;

impl DryRunExecutor {
    pub fn new() -> Self {
        Self
    }

    fn dry_run_result(&self, action: &ApprovedAction) -> ActionResult {
        let now = timestamp();
        ActionResult {
            action_id: action.request.id.clone(),
            status: ActionStatus::DryRun,
            message: format!("dry-run: {:?} (safety={:?})", action.request.kind, action.request.safety_level),
            executed_at: now,
            duration_ms: 0,
            rollback: None,
        }
    }
}

impl Default for DryRunExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl ApprovedActionExecutor for DryRunExecutor {
    fn execute(&self, action: ApprovedAction) -> Result<ActionResult, AppError> {
        Ok(self.dry_run_result(&action))
    }
}

fn timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()),
        Err(_) => "0.000000000Z".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{ActionId, ActionKind, ActionRequest, ActionSafetyLevel, DecisionId};

    #[test]
    fn dry_run_returns_dry_run_status() {
        let executor = DryRunExecutor::new();
        let action = ApprovedAction {
            request: ActionRequest {
                id: ActionId::from("test-action"),
                kind: ActionKind::CgroupCreate { name: "bench".into() },
                safety_level: ActionSafetyLevel::LowRisk,
            },
            decision_id: DecisionId::from("dec-1"),
        };

        let result = executor.execute(action).unwrap();
        assert_eq!(result.status, ActionStatus::DryRun);
        assert_eq!(result.action_id.as_str(), "test-action");
        assert!(result.message.contains("dry-run"));
        assert!(result.rollback.is_none());
    }
}
