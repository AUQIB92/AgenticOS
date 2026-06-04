use std::collections::HashMap;

use agenticos_domain::{ActionKind, CapabilityDescriptor, ToolMetadata};

/// Maps tool names to their capabilities.
///
/// A "tool" is a named external program or resource (e.g. "firefox",
/// "vscode", "git", "browser"). Each tool provides one or more capabilities
/// (e.g. LaunchApplication, OpenUrl, CloneRepository).
pub trait ToolRegistry: Send + Sync {
    /// Look up a tool by name and return its metadata.
    fn lookup(&self, name: &str) -> Option<&ToolMetadata>;

    /// Return all registered tool names.
    fn list_tools(&self) -> Vec<String>;

    /// Find tools that provide a specific capability (action kind).
    fn find_by_capability(&self, kind: &ActionKind) -> Vec<String>;
}

/// A static, hardcoded registry that maps common tools to capabilities.
///
/// This is the default registry used by the ActionGraphBuilder. It covers
/// the tool-action mappings needed by the MockPlannerAgent.
pub struct StaticToolRegistry {
    tools: HashMap<String, ToolMetadata>,
}

impl StaticToolRegistry {
    pub fn new() -> Self {
        let mut tools: HashMap<String, ToolMetadata> = HashMap::new();

        // firefox → LaunchApplication
        tools.insert(
            "firefox".into(),
            ToolMetadata {
                name: "firefox".into(),
                version: Some("latest".into()),
                description: "Mozilla Firefox web browser".into(),
                capabilities: vec![CapabilityDescriptor {
                    tool: "firefox".into(),
                    action_kind: ActionKind::LaunchApplication {
                        application: "firefox".into(),
                    },
                    description: "Launch the Firefox web browser".into(),
                }],
            },
        );

        // vscode / code → LaunchApplication
        tools.insert(
            "vscode".into(),
            ToolMetadata {
                name: "vscode".into(),
                version: Some("latest".into()),
                description: "Visual Studio Code editor".into(),
                capabilities: vec![CapabilityDescriptor {
                    tool: "code".into(),
                    action_kind: ActionKind::LaunchApplication {
                        application: "vscode".into(),
                    },
                    description: "Launch VS Code editor".into(),
                }],
            },
        );

        // browser → OpenUrl
        tools.insert(
            "browser".into(),
            ToolMetadata {
                name: "browser".into(),
                version: None,
                description: "Default web browser".into(),
                capabilities: vec![CapabilityDescriptor {
                    tool: "browser".into(),
                    action_kind: ActionKind::OpenUrl {
                        url: String::new(),
                    },
                    description: "Open a URL in the default browser".into(),
                }],
            },
        );

        // git → CloneRepository
        tools.insert(
            "git".into(),
            ToolMetadata {
                name: "git".into(),
                version: Some("2.x".into()),
                description: "Git version control system".into(),
                capabilities: vec![CapabilityDescriptor {
                    tool: "git".into(),
                    action_kind: ActionKind::CloneRepository {
                        url: String::new(),
                        directory: String::new(),
                    },
                    description: "Clone a git repository".into(),
                }],
            },
        );

        // shell → RunCommand
        tools.insert(
            "shell".into(),
            ToolMetadata {
                name: "shell".into(),
                version: None,
                description: "System shell for running commands".into(),
                capabilities: vec![CapabilityDescriptor {
                    tool: "shell".into(),
                    action_kind: ActionKind::RunCommand {
                        command: String::new(),
                        args: String::new(),
                    },
                    description: "Execute a shell command".into(),
                }],
            },
        );

        // filesystem → CreateDirectory, OpenFile
        tools.insert(
            "filesystem".into(),
            ToolMetadata {
                name: "filesystem".into(),
                version: None,
                description: "Local filesystem operations".into(),
                capabilities: vec![
                    CapabilityDescriptor {
                        tool: "filesystem".into(),
                        action_kind: ActionKind::CreateDirectory {
                            path: String::new(),
                        },
                        description: "Create a directory".into(),
                    },
                    CapabilityDescriptor {
                        tool: "filesystem".into(),
                        action_kind: ActionKind::OpenFile {
                            path: String::new(),
                        },
                        description: "Open a file".into(),
                    },
                ],
            },
        );

        Self { tools }
    }
}

impl Default for StaticToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry for StaticToolRegistry {
    fn lookup(&self, name: &str) -> Option<&ToolMetadata> {
        self.tools.get(name)
    }

    fn list_tools(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.keys().cloned().collect();
        names.sort();
        names
    }

    fn find_by_capability(&self, kind: &ActionKind) -> Vec<String> {
        let mut result = Vec::new();
        for (name, meta) in &self.tools {
            for cap in &meta.capabilities {
                if std::mem::discriminant(&cap.action_kind)
                    == std::mem::discriminant(kind)
                {
                    result.push(name.clone());
                    break;
                }
            }
        }
        result.sort();
        result
    }
}

/// Convenience wrapper around a ToolRegistry for use in builders.
pub struct ToolResolver {
    registry: Box<dyn ToolRegistry>,
}

impl ToolResolver {
    pub fn new(registry: Box<dyn ToolRegistry>) -> Self {
        Self { registry }
    }

    /// Resolve which tool provides a given capability (action kind).
    ///
    /// For application-launch actions, prefers the tool whose name matches the
    /// application parameter over generic tool-capability matching.
    pub fn resolve(&self, kind: &ActionKind) -> Option<String> {
        // Try exact-name matching first for LaunchApplication
        if let ActionKind::LaunchApplication { application } = kind {
            if !application.is_empty() {
                let app_lower = application.to_lowercase();
                for tool_name in self.registry.list_tools() {
                    if tool_name == app_lower {
                        return Some(tool_name);
                    }
                }
            }
        }
        // Fall back to generic capability matching
        let tools = self.registry.find_by_capability(kind);
        tools.first().cloned()
    }

    pub fn registry(&self) -> &dyn ToolRegistry {
        self.registry.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::ActionKind;

    #[test]
    fn static_registry_lists_known_tools() {
        let reg = StaticToolRegistry::new();
        let tools = reg.list_tools();
        assert!(tools.contains(&"firefox".to_string()));
        assert!(tools.contains(&"vscode".to_string()));
        assert!(tools.contains(&"browser".to_string()));
        assert!(tools.contains(&"git".to_string()));
        assert!(tools.contains(&"shell".to_string()));
        assert!(tools.contains(&"filesystem".to_string()));
    }

    #[test]
    fn static_registry_lookup_firefox() {
        let reg = StaticToolRegistry::new();
        let meta = reg.lookup("firefox").unwrap();
        assert_eq!(meta.name, "firefox");
        assert_eq!(meta.capabilities.len(), 1);
    }

    #[test]
    fn static_registry_lookup_unknown_returns_none() {
        let reg = StaticToolRegistry::new();
        assert!(reg.lookup("nonexistent").is_none());
    }

    #[test]
    fn find_by_capability_launch_app() {
        let reg = StaticToolRegistry::new();
        let matching = reg.find_by_capability(&ActionKind::LaunchApplication {
            application: "anything".into(),
        });
        assert!(matching.contains(&"firefox".to_string()));
        assert!(matching.contains(&"vscode".to_string()));
    }

    #[test]
    fn find_by_capability_open_url() {
        let reg = StaticToolRegistry::new();
        let matching = reg.find_by_capability(&ActionKind::OpenUrl {
            url: "https://example.com".into(),
        });
        assert!(matching.contains(&"browser".to_string()));
    }

    #[test]
    fn tool_resolver_resolves_correctly() {
        let reg = StaticToolRegistry::new();
        let resolver = ToolResolver::new(Box::new(reg));
        let tool = resolver.resolve(&ActionKind::CloneRepository {
            url: "https://example.com/repo".into(),
            directory: "/tmp/repo".into(),
        });
        assert_eq!(tool, Some("git".to_string()));
    }

    #[test]
    fn tool_resolver_unknown_returns_none() {
        // Cgroup actions are not in the static registry
        let reg = StaticToolRegistry::new();
        let resolver = ToolResolver::new(Box::new(reg));
        let tool = resolver.resolve(&ActionKind::CgroupCreate {
            name: "test".into(),
        });
        assert!(tool.is_none());
    }

    #[test]
    fn registry_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<StaticToolRegistry>();
        assert_sync::<StaticToolRegistry>();
    }
}
