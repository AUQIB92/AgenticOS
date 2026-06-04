use agenticos_domain::{
    ActionEdge, ActionGraph, ActionId, ActionKind, ActionMetadata, ActionNode, ActionStatus,
    PlanStep, TaskPlan,
};

use crate::tool_registry::ToolResolver;

/// Converts a deterministic TaskPlan into an executable ActionGraph.
///
/// This builder is purely structural:
/// - It does NOT execute actions
/// - It does NOT create proposals
/// - It does NOT invoke policy or safety
/// - It does NOT mutate any OS resources
///
/// Each PlanStep is mapped to one ActionNode. Sequential steps are connected
/// via dependency edges (prerequisite → dependent).
pub struct ActionGraphBuilder {
    resolver: ToolResolver,
}

impl ActionGraphBuilder {
    pub fn new(resolver: ToolResolver) -> Self {
        Self { resolver }
    }

    /// Build an ActionGraph from a TaskPlan.
    ///
    /// Returns `None` if the plan has no steps.
    pub fn build(&self, plan: &TaskPlan) -> Option<ActionGraph> {
        if plan.steps.is_empty() {
            return None;
        }

        let mut nodes: Vec<agenticos_domain::ActionNode> = Vec::with_capacity(plan.steps.len());
        let mut edges = Vec::new();

        for (i, step) in plan.steps.iter().enumerate() {
            let action_kind = step_to_action_kind(step);
            let tool = self.resolver.resolve(&action_kind);

            let node = ActionNode::new(
                ActionId::new(),
                action_kind,
                step.parameters.clone(),
                ActionMetadata {
                    source_step: step.order,
                    source_plan_id: plan.id.clone(),
                    source_intent_id: plan.source_intent_id.clone(),
                    tool: tool.clone(),
                    capability: Some(step.action.clone()),
                },
            );

            // Create dependency edge from the previous step to this one.
            if i > 0 {
                edges.push(ActionEdge {
                    prerequisite_id: nodes[i - 1].id.clone(),
                    dependent_id: node.id.clone(),
                    reason: format!(
                        "step {} must complete before step {}",
                        nodes[i - 1].metadata.source_step,
                        step.order
                    ),
                });
            }

            nodes.push(node);
        }

        Some(ActionGraph::new(
            nodes,
            edges,
            plan.id.clone(),
            plan.source_intent_id.clone(),
        ))
    }

    /// Update the status of an action node within the graph.
    pub fn update_status(
        _graph: &mut ActionGraph,
        node_id: &ActionId,
        status: ActionStatus,
    ) -> Option<()> {
        let node = _graph.nodes.iter_mut().find(|n| n.id == *node_id)?;
        node.status = status;
        Some(())
    }
}

/// Map a PlanStep's action string to an ActionKind variant.
fn step_to_action_kind(step: &PlanStep) -> ActionKind {
    match step.action.as_str() {
        "launch_application" => ActionKind::LaunchApplication {
            application: step
                .parameters
                .get("application")
                .cloned()
                .unwrap_or_default(),
        },
        "open_url" => ActionKind::OpenUrl {
            url: step.parameters.get("url").cloned().unwrap_or_default(),
        },
        "run_command" => ActionKind::RunCommand {
            command: step
                .parameters
                .get("command")
                .cloned()
                .unwrap_or_default(),
            args: step.parameters.get("args").cloned().unwrap_or_default(),
        },
        "create_directory" => ActionKind::CreateDirectory {
            path: step.parameters.get("path").cloned().unwrap_or_default(),
        },
        "open_file" => ActionKind::OpenFile {
            path: step.parameters.get("path").cloned().unwrap_or_default(),
        },
        "clone_repository" => ActionKind::CloneRepository {
            url: step.parameters.get("url").cloned().unwrap_or_default(),
            directory: step
                .parameters
                .get("directory")
                .cloned()
                .unwrap_or_default(),
        },
        "initialize_project" => ActionKind::CreateProjectWorkspace {
            project_name: step
                .parameters
                .get("project_name")
                .cloned()
                .unwrap_or_default(),
            framework: step
                .parameters
                .get("framework")
                .cloned()
                .unwrap_or_default(),
        },
        other => panic!("unknown plan step action: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use agenticos_domain::{IntentId, PlanStep, TaskPlan};
    use crate::tool_registry::{StaticToolRegistry, ToolResolver};

    fn builder() -> ActionGraphBuilder {
        let reg = StaticToolRegistry::new();
        let resolver = ToolResolver::new(Box::new(reg));
        ActionGraphBuilder::new(resolver)
    }

    fn make_plan(steps: Vec<PlanStep>) -> TaskPlan {
        TaskPlan::new(IntentId::new(), steps, "pending")
    }

    #[test]
    fn build_single_step_plan() {
        let mut params = HashMap::new();
        params.insert("application".into(), "firefox".into());
        let step = PlanStep::new(1, "launch_application", params);
        let plan = make_plan(vec![step]);

        let graph = builder().build(&plan).unwrap();
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);
        assert_eq!(graph.nodes[0].status, ActionStatus::Pending);
        match &graph.nodes[0].kind {
            ActionKind::LaunchApplication { application } => {
                assert_eq!(application, "firefox");
            }
            _ => panic!("expected LaunchApplication"),
        }
    }

    #[test]
    fn build_two_step_plan_with_dependency() {
        let step1 = PlanStep::new(1, "launch_application", {
            let mut p = HashMap::new();
            p.insert("application".into(), "firefox".into());
            p
        });
        let step2 = PlanStep::new(2, "open_url", {
            let mut p = HashMap::new();
            p.insert("url".into(), "https://github.com".into());
            p
        });
        let plan = make_plan(vec![step1, step2]);

        let graph = builder().build(&plan).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
        assert_eq!(
            graph.edges[0].prerequisite_id,
            graph.nodes[0].id
        );
        assert_eq!(
            graph.edges[0].dependent_id,
            graph.nodes[1].id
        );
    }

    #[test]
    fn build_project_plan_two_steps() {
        let step1 = PlanStep::new(1, "create_directory", {
            let mut p = HashMap::new();
            p.insert("path".into(), "myproject".into());
            p
        });
        let step2 = PlanStep::new(2, "initialize_project", {
            let mut p = HashMap::new();
            p.insert("project_name".into(), "myproject".into());
            p.insert("framework".into(), "nextjs".into());
            p
        });
        let plan = make_plan(vec![step1, step2]);

        let graph = builder().build(&plan).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
        match &graph.nodes[0].kind {
            ActionKind::CreateDirectory { path } => assert_eq!(path, "myproject"),
            _ => panic!("expected CreateDirectory"),
        }
        match &graph.nodes[1].kind {
            ActionKind::CreateProjectWorkspace {
                project_name,
                framework,
            } => {
                assert_eq!(project_name, "myproject");
                assert_eq!(framework, "nextjs");
            }
            _ => panic!("expected CreateProjectWorkspace"),
        }
    }

    #[test]
    fn build_returns_none_for_empty_plan() {
        let plan = TaskPlan {
            id: agenticos_domain::PlanId::new(),
            source_intent_id: IntentId::new(),
            steps: vec![],
            status: "pending".into(),
            timestamp: "0.000000000Z".into(),
        };
        assert!(builder().build(&plan).is_none());
    }

    #[test]
    fn graph_source_ids_match_plan() {
        let step = PlanStep::new(1, "launch_application", {
            let mut p = HashMap::new();
            p.insert("application".into(), "code".into());
            p
        });
        let plan = make_plan(vec![step]);
        let plan_id = plan.id.clone();
        let intent_id = plan.source_intent_id.clone();

        let graph = builder().build(&plan).unwrap();
        assert_eq!(graph.source_plan_id, plan_id);
        assert_eq!(graph.source_intent_id, intent_id);
    }

    #[test]
    fn update_status_modifies_node() {
        let step = PlanStep::new(1, "launch_application", HashMap::new());
        let plan = make_plan(vec![step]);
        let mut graph = builder().build(&plan).unwrap();

        let node_id = graph.nodes[0].id.clone();
        let result = ActionGraphBuilder::update_status(
            &mut graph,
            &node_id,
            ActionStatus::Executing,
        );
        assert!(result.is_some());
        assert_eq!(graph.nodes[0].status, ActionStatus::Executing);
    }

    #[test]
    fn tool_resolved_in_metadata() {
        let step = PlanStep::new(1, "launch_application", {
            let mut p = HashMap::new();
            p.insert("application".into(), "firefox".into());
            p
        });
        let plan = make_plan(vec![step]);

        let graph = builder().build(&plan).unwrap();
        // The ActionGraphBuilder resolves firefox → tool "firefox"
        assert_eq!(graph.nodes[0].metadata.tool.as_deref(), Some("firefox"));
    }

    #[test]
    fn builder_is_deterministic() {
        let step = PlanStep::new(1, "launch_application", {
            let mut p = HashMap::new();
            p.insert("application".into(), "vscode".into());
            p
        });
        let plan = make_plan(vec![step]);

        let g1 = builder().build(&plan).unwrap();
        let g2 = builder().build(&plan).unwrap();
        // ActionIds will differ (they are generated), but kinds and params
        // should be the same.
        assert_eq!(g1.nodes.len(), g2.nodes.len());
        assert_eq!(
            format!("{:?}", g1.nodes[0].kind),
            format!("{:?}", g2.nodes[0].kind)
        );
    }
}
