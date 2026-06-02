use agenticos_application::AppError;
use agenticos_domain::{ActionResult, ApprovedAction};

/// The only component permitted to mutate operating-system state.
///
/// Implementations must:
/// - Accept only `ApprovedAction` (already validated by Policy Kernel)
/// - Capture pre-mutation state for rollback
/// - Return `ActionResult` with duration, timestamp, and optional rollback token
/// - Never receive `Proposal` or `Decision` directly
pub trait ApprovedActionExecutor: Send + Sync {
    fn execute(&self, action: ApprovedAction) -> Result<ActionResult, AppError>;
}
