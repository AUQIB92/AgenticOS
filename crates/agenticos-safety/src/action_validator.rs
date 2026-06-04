use std::collections::{HashMap, HashSet};

use agenticos_domain::{ActionKind, ActionRequest};

/// Risk level assigned to a command for safety veto decisions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandRiskLevel {
    /// Low-risk commands that are always safe (e.g., ls, echo, cargo build).
    Safe,
    /// Moderate-risk commands allowed with caution (e.g., git status, npm install).
    Moderate,
    /// Dangerous commands vetoed by safety (e.g., rm, dd, mkfs).
    Dangerous,
    /// Critical commands vetoed by safety (e.g., shutdown, reboot, poweroff).
    Critical,
}

/// Performs safety validation on action-level proposals before execution.

/// Unlike the DefaultSafetyGovernor (which handles proposal-level governance
/// and incident-triggered vetoes), this validator focuses on the specific
/// parameter values of desktop/productivity actions.
pub struct SafetyActionValidator {
    /// Allowed application names.
    pub allowed_applications: HashSet<String>,
    /// Max URL length (chars) to prevent abuse.
    pub max_url_length: usize,
    /// Allowed URL schemes.
    pub allowed_url_schemes: HashSet<String>,
    /// Max directory depth to prevent deep traversal.
    pub max_directory_depth: usize,
    /// Max command args length.
    pub max_command_args_length: usize,
    /// Whether to allow dotfile paths (hidden files/dirs).
    pub allow_dotfiles: bool,
    /// Per-command risk level for safety veto decisions.
    pub command_risk: HashMap<String, CommandRiskLevel>,
}

impl Default for SafetyActionValidator {
    fn default() -> Self {
        Self {
            allowed_applications: ["firefox", "vscode", "code", "chrome", "edge", "terminal"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            max_url_length: 2048,
            allowed_url_schemes: ["https", "http"].iter().map(|s| s.to_string()).collect(),
            max_directory_depth: 10,
            max_command_args_length: 1024,
            allow_dotfiles: false,
            command_risk: [
                ("ls", CommandRiskLevel::Safe),
                ("echo", CommandRiskLevel::Safe),
                ("cat", CommandRiskLevel::Safe),
                ("pwd", CommandRiskLevel::Safe),
                ("date", CommandRiskLevel::Safe),
                ("whoami", CommandRiskLevel::Safe),
                ("cargo", CommandRiskLevel::Safe),
                ("npm", CommandRiskLevel::Safe),
                ("git", CommandRiskLevel::Moderate),
                ("docker", CommandRiskLevel::Moderate),
                ("rm", CommandRiskLevel::Dangerous),
                ("dd", CommandRiskLevel::Dangerous),
                ("mkfs", CommandRiskLevel::Dangerous),
                ("chmod", CommandRiskLevel::Dangerous),
                ("shutdown", CommandRiskLevel::Critical),
                ("reboot", CommandRiskLevel::Critical),
                ("poweroff", CommandRiskLevel::Critical),
                ("init", CommandRiskLevel::Critical),
                ("kill", CommandRiskLevel::Critical),
            ]
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        }
    }
}

impl SafetyActionValidator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate an action request against safety rules.
    /// Returns Ok(()) if safe, Err(reason) if unsafe.
    pub fn validate(&self, request: &ActionRequest) -> Result<(), String> {
        match &request.kind {
            ActionKind::LaunchApplication { application } => {
                self.validate_launch_application(application)
            }
            ActionKind::OpenUrl { url } => self.validate_open_url(url),
            ActionKind::RunCommand { command, args } => {
                self.validate_run_command(command, args)
            }
            ActionKind::CreateDirectory { path } => self.validate_path_safety(path),
            ActionKind::OpenFile { path } => self.validate_path_safety(path),
            ActionKind::CloneRepository { url, directory } => {
                self.validate_clone_repository(url, directory)
            }
            ActionKind::CreateProjectWorkspace { project_name, .. } => {
                self.validate_project_name(project_name)
            }
            _ => Ok(()),
        }
    }

    fn validate_launch_application(&self, application: &str) -> Result<(), String> {
        if application.is_empty() {
            return Err("application name is empty".into());
        }
        if application.contains("..") || application.contains('/') || application.contains('\\') {
            return Err(format!(
                "application name '{application}' contains path traversal characters"
            ));
        }
        if !self.allowed_applications.contains(application) {
            return Err(format!(
                "application '{application}' not in safety allowlist"
            ));
        }
        Ok(())
    }

    fn validate_open_url(&self, url: &str) -> Result<(), String> {
        if url.is_empty() {
            return Err("URL is empty".into());
        }
        if url.len() > self.max_url_length {
            return Err(format!(
                "URL length {} exceeds max {}",
                url.len(),
                self.max_url_length
            ));
        }
        let scheme = url.split(':').next().unwrap_or("");
        if !self.allowed_url_schemes.contains(scheme) {
            return Err(format!(
                "URL scheme '{scheme}' not allowed (must be https or http)"
            ));
        }
        Ok(())
    }

    fn validate_run_command(&self, command: &str, args: &str) -> Result<(), String> {
        if command.is_empty() {
            return Err("command is empty".into());
        }
        if args.len() > self.max_command_args_length {
            return Err(format!(
                "command args length {} exceeds max {}",
                args.len(),
                self.max_command_args_length
            ));
        }
        let risk = self
            .command_risk
            .get(command)
            .cloned()
            .unwrap_or(CommandRiskLevel::Moderate);
        match risk {
            CommandRiskLevel::Safe | CommandRiskLevel::Moderate => Ok(()),
            CommandRiskLevel::Dangerous => Err(format!(
                "command '{command}' is dangerous (vetoed by safety)"
            )),
            CommandRiskLevel::Critical => Err(format!(
                "command '{command}' is critical (vetoed by safety)"
            )),
        }
    }

    fn validate_path_safety(&self, path: &str) -> Result<(), String> {
        if path.is_empty() {
            return Err("path is empty".into());
        }

        // Check for path traversal (e.g., ../../../etc)
        if path.contains("..") {
            return Err(format!(
                "path '{path}' contains parent directory traversal '..'"
            ));
        }

        // Check depth
        let depth = path.split(|c| c == '/' || c == '\\').count();
        if depth > self.max_directory_depth {
            return Err(format!(
                "path depth {depth} exceeds max {}",
                self.max_directory_depth
            ));
        }

        // Check for dotfiles
        if !self.allow_dotfiles {
            for segment in path.split(|c| c == '/' || c == '\\') {
                if segment.starts_with('.') && !segment.is_empty() {
                    return Err(format!(
                        "path references hidden file/directory '{segment}'"
                    ));
                }
            }
        }

        Ok(())
    }

    fn validate_clone_repository(&self, url: &str, directory: &str) -> Result<(), String> {
        if url.is_empty() {
            return Err("repository URL is empty".into());
        }
        if directory.is_empty() {
            return Err("clone directory is empty".into());
        }
        // Validate URL scheme
        if url.starts_with("https://") || url.starts_with("http://") || url.starts_with("git@") {
            Ok(())
        } else {
            Err(format!(
                "repository URL '{url}' uses unsupported protocol"
            ))
        }
    }

    fn validate_project_name(&self, project_name: &str) -> Result<(), String> {
        if project_name.is_empty() {
            return Err("project name is empty".into());
        }
        if project_name.contains(' ') {
            return Err("project name contains spaces".into());
        }
        if project_name.contains("..") {
            return Err("project name contains path traversal".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{ActionId, ActionRequest, ActionSafetyLevel};

    fn request(kind: ActionKind) -> ActionRequest {
        ActionRequest {
            id: ActionId::from("test"),
            kind,
            safety_level: ActionSafetyLevel::LowRisk,
        }
    }

    #[test]
    fn valid_application_passes() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::LaunchApplication {
            application: "firefox".into(),
        });
        assert!(validator.validate(&req).is_ok());
    }

    #[test]
    fn unknown_application_fails() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::LaunchApplication {
            application: "malware".into(),
        });
        assert!(validator.validate(&req).is_err());
    }

    #[test]
    fn application_with_path_traversal_fails() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::LaunchApplication {
            application: "../../evil.sh".into(),
        });
        assert!(validator.validate(&req).is_err());
    }

    #[test]
    fn valid_url_passes() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::OpenUrl {
            url: "https://github.com".into(),
        });
        assert!(validator.validate(&req).is_ok());
    }

    #[test]
    fn empty_url_fails() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::OpenUrl {
            url: "".into(),
        });
        assert!(validator.validate(&req).is_err());
    }

    #[test]
    fn url_with_bad_scheme_fails() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::OpenUrl {
            url: "javascript:alert(1)".into(),
        });
        assert!(validator.validate(&req).is_err());
    }

    #[test]
    fn dangerous_command_fails() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::RunCommand {
            command: "rm".into(),
            args: "-rf /".into(),
        });
        assert!(validator.validate(&req).is_err());
    }

    #[test]
    fn safe_command_passes() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::RunCommand {
            command: "ls".into(),
            args: "-la".into(),
        });
        assert!(validator.validate(&req).is_ok());
    }

    #[test]
    fn cargo_build_safety_passes() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::RunCommand {
            command: "cargo".into(),
            args: "build".into(),
        });
        assert!(validator.validate(&req).is_ok());
    }

    #[test]
    fn git_clone_safety_passes_moderate() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::RunCommand {
            command: "git".into(),
            args: "clone".into(),
        });
        assert!(validator.validate(&req).is_ok());
    }

    #[test]
    fn dangerous_command_vetoed() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::RunCommand {
            command: "rm".into(),
            args: "-rf /".into(),
        });
        let err = validator.validate(&req).unwrap_err();
        assert!(err.contains("dangerous"));
    }

    #[test]
    fn critical_command_vetoed() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::RunCommand {
            command: "shutdown".into(),
            args: "".into(),
        });
        let err = validator.validate(&req).unwrap_err();
        assert!(err.contains("critical"));
    }

    #[test]
    fn chmod_vetoed() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::RunCommand {
            command: "chmod".into(),
            args: "-R 777 /".into(),
        });
        assert!(validator.validate(&req).is_err());
    }

    #[test]
    fn unknown_command_defaults_moderate() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::RunCommand {
            command: "some-unknown-tool".into(),
            args: "--help".into(),
        });
        assert!(validator.validate(&req).is_ok());
    }

    #[test]
    fn path_traversal_fails() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::CreateDirectory {
            path: "/tmp/../../../etc/passwd".into(),
        });
        assert!(validator.validate(&req).is_err());
    }

    #[test]
    fn valid_directory_passes() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::CreateDirectory {
            path: "/tmp/test".into(),
        });
        assert!(validator.validate(&req).is_ok());
    }

    #[test]
    fn dotfile_path_fails_by_default() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::OpenFile {
            path: "/home/user/.ssh/config".into(),
        });
        assert!(validator.validate(&req).is_err());
    }

    #[test]
    fn valid_repository_url_passes() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::CloneRepository {
            url: "https://github.com/user/repo.git".into(),
            directory: "/tmp/repo".into(),
        });
        assert!(validator.validate(&req).is_ok());
    }

    #[test]
    fn empty_repository_url_fails() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::CloneRepository {
            url: "".into(),
            directory: "/tmp/repo".into(),
        });
        assert!(validator.validate(&req).is_err());
    }

    #[test]
    fn unsupported_git_protocol_fails() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::CloneRepository {
            url: "ftp://example.com/repo.git".into(),
            directory: "/tmp/repo".into(),
        });
        assert!(validator.validate(&req).is_err());
    }

    #[test]
    fn valid_project_name_passes() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::CreateProjectWorkspace {
            project_name: "myproject".into(),
            framework: "nextjs".into(),
        });
        assert!(validator.validate(&req).is_ok());
    }

    #[test]
    fn project_name_with_spaces_fails() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::CreateProjectWorkspace {
            project_name: "my project".into(),
            framework: "nextjs".into(),
        });
        assert!(validator.validate(&req).is_err());
    }

    #[test]
    fn empty_project_name_fails() {
        let validator = SafetyActionValidator::default();
        let req = request(ActionKind::CreateProjectWorkspace {
            project_name: "".into(),
            framework: "".into(),
        });
        assert!(validator.validate(&req).is_err());
    }
}
