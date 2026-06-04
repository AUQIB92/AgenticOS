use std::time::{SystemTime, UNIX_EPOCH, Instant};

use agenticos_application::AppError;
use agenticos_domain::{ActionKind, ActionResult, ActionStatus, ApprovedAction, RollbackToken};

use crate::traits::ApprovedActionExecutor;

/// Executes LaunchApplication actions (noop implementation).
pub struct NoopLaunchAppExecutor;

impl NoopLaunchAppExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl ApprovedActionExecutor for NoopLaunchAppExecutor {
    fn execute(&self, action: ApprovedAction) -> Result<ActionResult, AppError> {
        let start = Instant::now();
        let application = match &action.request.kind {
            ActionKind::LaunchApplication { application } => application.clone(),
            _ => return Err(AppError::Message("expected LaunchApplication".into())),
        };
        let result = ActionResult {
            action_id: action.request.id.clone(),
            status: ActionStatus::Succeeded,
            message: format!("noop launch application: {application}"),
            executed_at: timestamp(),
            duration_ms: start.elapsed().as_millis() as u64,
            rollback: Some(RollbackToken {
                token: format!("kill-{application}"),
            }),
        };
        Ok(result)
    }
}

/// Executes OpenUrl actions (noop implementation).
pub struct NoopOpenUrlExecutor;

impl NoopOpenUrlExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl ApprovedActionExecutor for NoopOpenUrlExecutor {
    fn execute(&self, action: ApprovedAction) -> Result<ActionResult, AppError> {
        let start = Instant::now();
        let url = match &action.request.kind {
            ActionKind::OpenUrl { url } => url.clone(),
            _ => return Err(AppError::Message("expected OpenUrl".into())),
        };
        Ok(ActionResult {
            action_id: action.request.id.clone(),
            status: ActionStatus::Succeeded,
            message: format!("noop open url: {url}"),
            executed_at: timestamp(),
            duration_ms: start.elapsed().as_millis() as u64,
            rollback: None,
        })
    }
}

/// Executes CreateDirectory actions (noop implementation).
pub struct NoopCreateDirectoryExecutor;

impl NoopCreateDirectoryExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl ApprovedActionExecutor for NoopCreateDirectoryExecutor {
    fn execute(&self, action: ApprovedAction) -> Result<ActionResult, AppError> {
        let start = Instant::now();
        let path = match &action.request.kind {
            ActionKind::CreateDirectory { path } => path.clone(),
            _ => return Err(AppError::Message("expected CreateDirectory".into())),
        };
        Ok(ActionResult {
            action_id: action.request.id.clone(),
            status: ActionStatus::Succeeded,
            message: format!("noop create directory: {path}"),
            executed_at: timestamp(),
            duration_ms: start.elapsed().as_millis() as u64,
            rollback: Some(RollbackToken {
                token: format!("rmdir-{path}"),
            }),
        })
    }
}

/// Executes CloneRepository actions (noop implementation).
pub struct NoopCloneRepositoryExecutor;

impl NoopCloneRepositoryExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl ApprovedActionExecutor for NoopCloneRepositoryExecutor {
    fn execute(&self, action: ApprovedAction) -> Result<ActionResult, AppError> {
        let start = Instant::now();
        let (url, directory) = match &action.request.kind {
            ActionKind::CloneRepository { url, directory } => (url.clone(), directory.clone()),
            _ => return Err(AppError::Message("expected CloneRepository".into())),
        };
        Ok(ActionResult {
            action_id: action.request.id.clone(),
            status: ActionStatus::Succeeded,
            message: format!("noop clone repository: {url} -> {directory}"),
            executed_at: timestamp(),
            duration_ms: start.elapsed().as_millis() as u64,
            rollback: Some(RollbackToken {
                token: format!("remove-{directory}"),
            }),
        })
    }
}

/// Executes RunCommand actions (noop implementation).
pub struct NoopRunCommandExecutor;

impl NoopRunCommandExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl ApprovedActionExecutor for NoopRunCommandExecutor {
    fn execute(&self, action: ApprovedAction) -> Result<ActionResult, AppError> {
        let start = Instant::now();
        let (command, args) = match &action.request.kind {
            ActionKind::RunCommand { command, args } => (command.clone(), args.clone()),
            _ => return Err(AppError::Message("expected RunCommand".into())),
        };
        Ok(ActionResult {
            action_id: action.request.id.clone(),
            status: ActionStatus::Succeeded,
            message: format!("noop run command: {command} {args}"),
            executed_at: timestamp(),
            duration_ms: start.elapsed().as_millis() as u64,
            rollback: None,
        })
    }
}

/// Routes approved actions to the correct noop executor by ActionKind.
pub struct DefaultActionExecutor {
    launch_app: NoopLaunchAppExecutor,
    open_url: NoopOpenUrlExecutor,
    create_directory: NoopCreateDirectoryExecutor,
    clone_repository: NoopCloneRepositoryExecutor,
    run_command: NoopRunCommandExecutor,
}

impl DefaultActionExecutor {
    pub fn new() -> Self {
        Self {
            launch_app: NoopLaunchAppExecutor::new(),
            open_url: NoopOpenUrlExecutor::new(),
            create_directory: NoopCreateDirectoryExecutor::new(),
            clone_repository: NoopCloneRepositoryExecutor::new(),
            run_command: NoopRunCommandExecutor::new(),
        }
    }
}

impl Default for DefaultActionExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl ApprovedActionExecutor for DefaultActionExecutor {
    fn execute(&self, action: ApprovedAction) -> Result<ActionResult, AppError> {
        match &action.request.kind {
            ActionKind::LaunchApplication { .. } => self.launch_app.execute(action),
            ActionKind::OpenUrl { .. } => self.open_url.execute(action),
            ActionKind::CreateDirectory { .. } => self.create_directory.execute(action),
            ActionKind::CloneRepository { .. } => self.clone_repository.execute(action),
            ActionKind::RunCommand { .. } => self.run_command.execute(action),
            ActionKind::OpenFile { .. } | ActionKind::CreateProjectWorkspace { .. } => {
                let start = Instant::now();
                let kind_str = format!("{:?}", action.request.kind);
                Ok(ActionResult {
                    action_id: action.request.id,
                    status: ActionStatus::Succeeded,
                    message: format!("noop: {kind_str}"),
                    executed_at: timestamp(),
                    duration_ms: start.elapsed().as_millis() as u64,
                    rollback: None,
                })
            }
            _ => Err(AppError::Message(format!(
                "DefaultActionExecutor: unsupported action kind {:?}",
                action.request.kind
            ))),
        }
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
    use agenticos_domain::{ActionId, ActionRequest, ActionSafetyLevel, DecisionId};

    fn approved_action(kind: ActionKind) -> ApprovedAction {
        ApprovedAction {
            request: ActionRequest {
                id: ActionId::from("test-action"),
                kind,
                safety_level: ActionSafetyLevel::LowRisk,
            },
            decision_id: DecisionId::from("test-decision"),
        }
    }

    #[test]
    fn launch_app_executor_succeeds() {
        let executor = NoopLaunchAppExecutor::new();
        let action = approved_action(ActionKind::LaunchApplication {
            application: "firefox".into(),
        });
        let result = executor.execute(action).unwrap();
        assert_eq!(result.status, ActionStatus::Succeeded);
        assert!(result.message.contains("firefox"));
        assert!(result.rollback.is_some());
    }

    #[test]
    fn open_url_executor_succeeds() {
        let executor = NoopOpenUrlExecutor::new();
        let action = approved_action(ActionKind::OpenUrl {
            url: "https://example.com".into(),
        });
        let result = executor.execute(action).unwrap();
        assert_eq!(result.status, ActionStatus::Succeeded);
        assert!(result.message.contains("example.com"));
    }

    #[test]
    fn create_directory_executor_succeeds() {
        let executor = NoopCreateDirectoryExecutor::new();
        let action = approved_action(ActionKind::CreateDirectory {
            path: "/tmp/test".into(),
        });
        let result = executor.execute(action).unwrap();
        assert_eq!(result.status, ActionStatus::Succeeded);
        assert!(result.rollback.is_some());
    }

    #[test]
    fn clone_repository_executor_succeeds() {
        let executor = NoopCloneRepositoryExecutor::new();
        let action = approved_action(ActionKind::CloneRepository {
            url: "https://github.com/user/repo.git".into(),
            directory: "/tmp/repo".into(),
        });
        let result = executor.execute(action).unwrap();
        assert_eq!(result.status, ActionStatus::Succeeded);
        assert!(result.rollback.is_some());
    }

    #[test]
    fn run_command_executor_succeeds() {
        let executor = NoopRunCommandExecutor::new();
        let action = approved_action(ActionKind::RunCommand {
            command: "ls".into(),
            args: "-la".into(),
        });
        let result = executor.execute(action).unwrap();
        assert_eq!(result.status, ActionStatus::Succeeded);
        assert!(result.message.contains("ls"));
    }

    #[test]
    fn default_executor_routes_correctly() {
        let executor = DefaultActionExecutor::new();

        let launch = executor
            .execute(approved_action(ActionKind::LaunchApplication {
                application: "vscode".into(),
            }))
            .unwrap();
        assert_eq!(launch.status, ActionStatus::Succeeded);
        assert!(launch.message.contains("vscode"));

        let open = executor
            .execute(approved_action(ActionKind::OpenUrl {
                url: "https://github.com".into(),
            }))
            .unwrap();
        assert_eq!(open.status, ActionStatus::Succeeded);
        assert!(open.message.contains("github.com"));

        let mkdir = executor
            .execute(approved_action(ActionKind::CreateDirectory {
                path: "/tmp/dir".into(),
            }))
            .unwrap();
        assert_eq!(mkdir.status, ActionStatus::Succeeded);
        assert!(mkdir.message.contains("tmp/dir"));

        let clone = executor
            .execute(approved_action(ActionKind::CloneRepository {
                url: "https://github.com/user/repo.git".into(),
                directory: "/tmp/repo".into(),
            }))
            .unwrap();
        assert_eq!(clone.status, ActionStatus::Succeeded);
        assert!(clone.message.contains("repo.git"));

        let cmd = executor
            .execute(approved_action(ActionKind::RunCommand {
                command: "ls".into(),
                args: "-la".into(),
            }))
            .unwrap();
        assert_eq!(cmd.status, ActionStatus::Succeeded);
        assert!(cmd.message.contains("ls"));
    }

    #[test]
    fn default_executor_handles_open_file() {
        let executor = DefaultActionExecutor::new();
        let result = executor
            .execute(approved_action(ActionKind::OpenFile {
                path: "/tmp/test.txt".into(),
            }))
            .unwrap();
        assert_eq!(result.status, ActionStatus::Succeeded);
    }

    #[test]
    fn default_executor_handles_create_project_workspace() {
        let executor = DefaultActionExecutor::new();
        let result = executor
            .execute(approved_action(ActionKind::CreateProjectWorkspace {
                project_name: "myproject".into(),
                framework: "nextjs".into(),
            }))
            .unwrap();
        assert_eq!(result.status, ActionStatus::Succeeded);
    }
}
