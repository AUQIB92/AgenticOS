use agenticos_application::AppError;
use agenticos_domain::{ActionResult, RollbackToken};

/// Capability to undo a previously executed action.
pub trait RollbackManager: Send + Sync {
    fn rollback(&self, token: RollbackToken) -> Result<ActionResult, AppError>;
}
