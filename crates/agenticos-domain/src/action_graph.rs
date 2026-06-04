use std::collections::HashMap;

use crate::{ActionId, ActionKind, ActionStatus, PlanId, IntentId};

/// Phase of an action within the execution pipeline.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ActionPhase {
    Pending,
    Proposed,
    Approved,
    Denied,
    Executing,
    Succeeded,
    Failed,
    RolledBack,
}

/// A single actionable node within an ActionGraph.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ActionNode {
    pub id: ActionId,
    pub kind: ActionKind,
    pub params: HashMap<String, String>,
    pub status: ActionStatus,
    pub metadata: ActionMetadata,
}

impl ActionNode {
    pub fn new(
        id: ActionId,
        kind: ActionKind,
        params: HashMap<String, String>,
        metadata: ActionMetadata,
    ) -> Self {
        Self {
            id,
            kind,
            params,
            status: ActionStatus::Pending,
            metadata,
        }
    }
}

/// A directed dependency edge between two action nodes.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ActionEdge {
    pub prerequisite_id: ActionId,
    pub dependent_id: ActionId,
    pub reason: String,
}

/// Traceability metadata attached to an action node.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ActionMetadata {
    pub source_step: u32,
    pub source_plan_id: PlanId,
    pub source_intent_id: IntentId,
    pub tool: Option<String>,
    pub capability: Option<String>,
}

/// Describes what capability a registered tool provides.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CapabilityDescriptor {
    pub tool: String,
    pub action_kind: ActionKind,
    pub description: String,
}

/// Metadata about a registered tool.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ToolMetadata {
    pub name: String,
    pub version: Option<String>,
    pub description: String,
    pub capabilities: Vec<CapabilityDescriptor>,
}

/// A directed acyclic graph of actions derived from a TaskPlan.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ActionGraph {
    pub nodes: Vec<ActionNode>,
    pub edges: Vec<ActionEdge>,
    pub source_plan_id: PlanId,
    pub source_intent_id: IntentId,
}

impl ActionGraph {
    pub fn new(
        nodes: Vec<ActionNode>,
        edges: Vec<ActionEdge>,
        source_plan_id: PlanId,
        source_intent_id: IntentId,
    ) -> Self {
        Self {
            nodes,
            edges,
            source_plan_id,
            source_intent_id,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Return all prerequisites for a given action node id.
    pub fn prerequisites_of(&self, node_id: &ActionId) -> Vec<&ActionNode> {
        let prereq_ids: Vec<&ActionId> = self
            .edges
            .iter()
            .filter(|e| &e.dependent_id == node_id)
            .map(|e| &e.prerequisite_id)
            .collect();
        self.nodes
            .iter()
            .filter(|n| prereq_ids.contains(&&n.id))
            .collect()
    }

    /// Return all dependents of a given action node id.
    pub fn dependents_of(&self, node_id: &ActionId) -> Vec<&ActionNode> {
        let dep_ids: Vec<&ActionId> = self
            .edges
            .iter()
            .filter(|e| &e.prerequisite_id == node_id)
            .map(|e| &e.dependent_id)
            .collect();
        self.nodes
            .iter()
            .filter(|n| dep_ids.contains(&&n.id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample_graph() -> ActionGraph {
        let plan_id = PlanId::from_string("PlanId-1");
        let intent_id = IntentId::from_string("IntentId-1");

        let node1 = ActionNode::new(
            ActionId::from_string("ActionId-1"),
            ActionKind::LaunchApplication {
                application: "firefox".into(),
            },
            {
                let mut p = HashMap::new();
                p.insert("application".into(), "firefox".into());
                p
            },
            ActionMetadata {
                source_step: 1,
                source_plan_id: plan_id.clone(),
                source_intent_id: intent_id.clone(),
                tool: Some("firefox".into()),
                capability: Some("launch_application".into()),
            },
        );

        let node2 = ActionNode::new(
            ActionId::from_string("ActionId-2"),
            ActionKind::OpenUrl {
                url: "https://github.com".into(),
            },
            {
                let mut p = HashMap::new();
                p.insert("url".into(), "https://github.com".into());
                p
            },
            ActionMetadata {
                source_step: 2,
                source_plan_id: plan_id.clone(),
                source_intent_id: intent_id.clone(),
                tool: Some("browser".into()),
                capability: Some("open_url".into()),
            },
        );

        let edge = ActionEdge {
            prerequisite_id: ActionId::from_string("ActionId-1"),
            dependent_id: ActionId::from_string("ActionId-2"),
            reason: "launch browser before opening URL".into(),
        };

        ActionGraph::new(vec![node1, node2], vec![edge], plan_id, intent_id)
    }

    #[test]
    fn action_graph_constructs() {
        let graph = sample_graph();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn action_node_has_pending_status() {
        let graph = sample_graph();
        assert_eq!(graph.nodes[0].status, ActionStatus::Pending);
    }

    #[test]
    fn prerequisites_of_returns_correct_nodes() {
        let graph = sample_graph();
        let deps = graph.prerequisites_of(&ActionId::from_string("ActionId-2"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].id.as_str(), "ActionId-1");
    }

    #[test]
    fn dependents_of_returns_correct_nodes() {
        let graph = sample_graph();
        let deps = graph.dependents_of(&ActionId::from_string("ActionId-1"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].id.as_str(), "ActionId-2");
    }

    #[test]
    fn action_graph_round_trips_via_json() {
        let graph = sample_graph();
        let json = serde_json::to_string(&graph).unwrap();
        let back: ActionGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(back.node_count(), 2);
        assert_eq!(back.edge_count(), 1);
    }

    #[test]
    fn action_node_kind_serialization() {
        let kind = ActionKind::LaunchApplication {
            application: "vscode".into(),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains("LaunchApplication"));
        assert!(json.contains("vscode"));
        let back: ActionKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, back);
    }

    #[test]
    fn action_edge_constructs() {
        let edge = ActionEdge {
            prerequisite_id: ActionId::from_string("ActionId-1"),
            dependent_id: ActionId::from_string("ActionId-2"),
            reason: "depends".into(),
        };
        assert_eq!(edge.prerequisite_id.as_str(), "ActionId-1");
        assert_eq!(edge.dependent_id.as_str(), "ActionId-2");
    }

    #[test]
    fn action_graph_is_empty() {
        let graph = ActionGraph::new(
            vec![],
            vec![],
            PlanId::from_string("P-1"),
            IntentId::from_string("I-1"),
        );
        assert!(graph.is_empty());
    }

    #[test]
    fn capability_descriptor_constructs() {
        let cap = CapabilityDescriptor {
            tool: "git".into(),
            action_kind: ActionKind::CloneRepository {
                url: "example.com/repo".into(),
                directory: "/tmp/repo".into(),
            },
            description: "Clone a git repository".into(),
        };
        assert_eq!(cap.tool, "git");
        let json = serde_json::to_string(&cap).unwrap();
        assert!(json.contains("CloneRepository"));
    }

    #[test]
    fn tool_metadata_constructs() {
        let cap = CapabilityDescriptor {
            tool: "vscode".into(),
            action_kind: ActionKind::LaunchApplication {
                application: "vscode".into(),
            },
            description: "Launch VS Code".into(),
        };
        let meta = ToolMetadata {
            name: "vscode".into(),
            version: Some("1.85".into()),
            description: "VS Code editor".into(),
            capabilities: vec![cap],
        };
        assert_eq!(meta.name, "vscode");
        assert_eq!(meta.capabilities.len(), 1);
    }
}
