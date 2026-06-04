use std::collections::{HashMap, HashSet};

use agenticos_domain::{ActionKind, ActionRequest};

/// Policy rules for action-level proposals (desktop/productivity actions).
///
/// These policies are evaluated during the proposal pipeline to determine
/// whether a given action kind and its parameters are allowed.
pub trait ActionProposalPolicy: Send + Sync {
    /// Check whether the action request is allowed.
    /// Returns Ok(true) if allowed, Ok(false) if denied, Err if policy error.
    fn check(&self, request: &ActionRequest) -> Result<bool, String>;

    /// Provide a human-readable explanation of why an action was denied.
    fn explain_denial(&self, request: &ActionRequest) -> String;
}

/// Default allowlist-based implementation of ActionProposalPolicy.
///
/// Each action kind has a dedicated allowlist that controls which parameter
/// values are permitted.
pub struct DefaultActionProposalPolicy {
    /// Allowed application names for LaunchApplication.
    pub allowed_applications: HashSet<String>,
    /// Allowed URL patterns (prefix match) for OpenUrl.
    pub allowed_url_prefixes: Vec<String>,
    /// Allowed directory paths (prefix match) for CreateDirectory.
    pub allowed_directory_prefixes: Vec<String>,
    /// Allowed repository hostnames for CloneRepository.
    pub allowed_repository_hosts: HashSet<String>,
    /// Allowed command names for RunCommand.
    pub allowed_commands: HashSet<String>,
    /// Per-command allowed first args (command → allowed first args).
    /// If a command is in `allowed_commands` but NOT in this map,
    /// any args combination is allowed. If it IS in this map, only the
    /// listed first args pass (e.g., cargo→[build, test]).
    pub command_args_allowlist: HashMap<String, Vec<String>>,
}

impl Default for DefaultActionProposalPolicy {
    fn default() -> Self {
        Self {
            allowed_applications: ["firefox", "vscode", "code", "chrome", "edge", "terminal"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            allowed_url_prefixes: vec![
                "https://".to_string(),
                "http://".to_string(),
            ],
            allowed_directory_prefixes: vec![
                "/tmp/".to_string(),
                "/home/".to_string(),
                "/workspace/".to_string(),
                "C:\\".to_string(),
                "D:\\".to_string(),
            ],
            allowed_repository_hosts: [
                "github.com",
                "gitlab.com",
                "bitbucket.org",
                "dev.azure.com",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            allowed_commands: ["ls", "echo", "cat", "pwd", "date", "whoami", "cargo", "git", "npm"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            command_args_allowlist: [
                ("cargo", vec!["build".into(), "test".into()]),
                ("git", vec!["status".into(), "clone".into()]),
                ("npm", vec!["install".into()]),
            ]
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        }
    }
}

impl ActionProposalPolicy for DefaultActionProposalPolicy {
    fn check(&self, request: &ActionRequest) -> Result<bool, String> {
        match &request.kind {
            ActionKind::LaunchApplication { application } => {
                Ok(self.allowed_applications.contains(application.as_str()))
            }
            ActionKind::OpenUrl { url } => {
                Ok(self.allowed_url_prefixes.iter().any(|prefix| url.starts_with(prefix)))
            }
            ActionKind::CreateDirectory { path } => {
                Ok(self.allowed_directory_prefixes.iter().any(|prefix| path.starts_with(prefix)))
            }
            ActionKind::OpenFile { path } => {
                Ok(self.allowed_directory_prefixes.iter().any(|prefix| path.starts_with(prefix)))
            }
            ActionKind::RunCommand { command, args } => {
                if !self.allowed_commands.contains(command.as_str()) {
                    return Ok(false);
                }
                if let Some(allowed_args) = self.command_args_allowlist.get(command) {
                    let first_arg = args.split_whitespace().next().unwrap_or("");
                    Ok(allowed_args.iter().any(|a| a == first_arg))
                } else {
                    Ok(true)
                }
            }
            ActionKind::CloneRepository { url, .. } => {
                let hostname = extract_hostname(url);
                Ok(hostname.map_or(false, |h| self.allowed_repository_hosts.contains(h.as_str())))
            }
            ActionKind::CreateProjectWorkspace { .. } => Ok(true),
            _ => Ok(true),
        }
    }

    fn explain_denial(&self, request: &ActionRequest) -> String {
        match &request.kind {
            ActionKind::LaunchApplication { application } => {
                format!(
                    "application '{application}' not in allowlist: {:?}",
                    self.allowed_applications
                )
            }
            ActionKind::OpenUrl { url } => {
                format!("URL '{url}' does not match any allowed prefix")
            }
            ActionKind::CreateDirectory { path } => {
                format!("directory path '{path}' not in allowed prefixes")
            }
            ActionKind::OpenFile { path } => {
                format!("file path '{path}' not in allowed prefixes")
            }
            ActionKind::RunCommand { command, .. } => {
                format!(
                    "command '{command}' not in allowlist: {:?}",
                    self.allowed_commands
                )
            }
            ActionKind::CloneRepository { url, .. } => {
                format!("repository URL '{url}' not from a trusted host")
            }
            _ => "action denied by policy".into(),
        }
    }
}

fn extract_hostname(url: &str) -> Option<String> {
    // Simple URL hostname extraction without url crate dependency
    let after_protocol = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("git@"))?;

    let hostname = if url.starts_with("git@") {
        after_protocol.split(':').next()?
    } else {
        after_protocol.split('/').next()?
    };

    Some(hostname.to_lowercase())
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
    fn allows_known_application() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::LaunchApplication {
            application: "firefox".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn denies_unknown_application() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::LaunchApplication {
            application: "malware".into(),
        });
        assert!(!policy.check(&req).unwrap());
    }

    #[test]
    fn allows_https_url() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::OpenUrl {
            url: "https://github.com".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn denies_unknown_protocol_url() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::OpenUrl {
            url: "ftp://evil.com".into(),
        });
        assert!(!policy.check(&req).unwrap());
    }

    #[test]
    fn allows_cargo_build() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::RunCommand {
            command: "cargo".into(),
            args: "build".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn allows_cargo_test() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::RunCommand {
            command: "cargo".into(),
            args: "test".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn denies_cargo_run() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::RunCommand {
            command: "cargo".into(),
            args: "run".into(),
        });
        assert!(!policy.check(&req).unwrap());
    }

    #[test]
    fn allows_git_status() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::RunCommand {
            command: "git".into(),
            args: "status".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn allows_git_clone() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::RunCommand {
            command: "git".into(),
            args: "clone https://github.com/user/repo.git".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn allows_npm_install() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::RunCommand {
            command: "npm".into(),
            args: "install".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn denies_rm_policy() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::RunCommand {
            command: "rm".into(),
            args: "-rf /".into(),
        });
        assert!(!policy.check(&req).unwrap());
    }

    #[test]
    fn allows_ls_with_any_args() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::RunCommand {
            command: "ls".into(),
            args: "-la".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn allows_known_command() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::RunCommand {
            command: "ls".into(),
            args: "-la".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn denies_unknown_command() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::RunCommand {
            command: "rm".into(),
            args: "-rf /".into(),
        });
        assert!(!policy.check(&req).unwrap());
    }

    #[test]
    fn allows_directory_in_allowed_prefix() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::CreateDirectory {
            path: "/tmp/test".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn denies_directory_outside_allowed_prefix() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::CreateDirectory {
            path: "/etc/passwd".into(),
        });
        assert!(!policy.check(&req).unwrap());
    }

    #[test]
    fn allows_known_repository_host() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::CloneRepository {
            url: "https://github.com/user/repo.git".into(),
            directory: "/tmp/repo".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn denies_unknown_repository_host() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::CloneRepository {
            url: "https://evil.com/repo.git".into(),
            directory: "/tmp/repo".into(),
        });
        assert!(!policy.check(&req).unwrap());
    }

    #[test]
    fn extract_hostname_https() {
        assert_eq!(
            extract_hostname("https://github.com/user/repo"),
            Some("github.com".into())
        );
    }

    #[test]
    fn extract_hostname_git_ssh() {
        assert_eq!(
            extract_hostname("git@github.com:user/repo.git"),
            Some("github.com".into())
        );
    }

    #[test]
    fn allows_create_project_workspace() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::CreateProjectWorkspace {
            project_name: "test".into(),
            framework: "nextjs".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn open_file_allowed_path() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::OpenFile {
            path: "/home/user/readme.md".into(),
        });
        assert!(policy.check(&req).unwrap());
    }

    #[test]
    fn open_file_denied_path() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::OpenFile {
            path: "/etc/shadow".into(),
        });
        assert!(!policy.check(&req).unwrap());
    }

    #[test]
    fn explain_denial_message() {
        let policy = DefaultActionProposalPolicy::default();
        let req = request(ActionKind::LaunchApplication {
            application: "malware".into(),
        });
        let msg = policy.explain_denial(&req);
        assert!(msg.contains("malware"));
        assert!(msg.contains("not in allowlist"));
    }
}
