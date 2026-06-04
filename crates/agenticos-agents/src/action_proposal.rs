use agenticos_domain::{
    ActionGraph, ActionKind, ActionNode, ActionRequest, ActionSafetyLevel, AgentId, Confidence,
    ObservationId, Proposal, ProposalId,
};

/// Converts an ActionGraph into a sequence of Proposals for the
/// Policy → Safety → Executor pipeline.
///
/// This agent is purely advisory. It creates Proposals from ActionNodes,
/// assigning appropriate safety levels based on the action kind.
/// All Proposals must still pass through Policy and Safety before execution.
pub struct ActionProposalAgent {
    agent_id: AgentId,
}

impl ActionProposalAgent {
    pub fn new(agent_id: AgentId) -> Self {
        Self { agent_id }
    }

    /// Convert an ActionGraph into a vector of Proposals.
    ///
    /// One Proposal is created per ActionNode. The safety level is
    /// assigned based on the ActionKind:
    ///
    /// | ActionKind              | Safety Level |
    /// |-------------------------|--------------|
    /// | LaunchApplication       | MediumRisk   |
    /// | OpenUrl                 | LowRisk      |
    /// | RunCommand              | HighRisk     |
    /// | CreateDirectory         | MediumRisk   |
    /// | OpenFile                | LowRisk      |
    /// | CloneRepository         | MediumRisk   |
    /// | CreateProjectWorkspace  | LowRisk      |
    pub fn propose(&self, graph: &ActionGraph) -> Vec<Proposal> {
        graph
            .nodes
            .iter()
            .map(|node| self.node_to_proposal(node))
            .collect()
    }

    /// Propose a subset of nodes (e.g., only ready/prerequisite-satisfied nodes).
    pub fn propose_nodes(&self, nodes: &[ActionNode]) -> Vec<Proposal> {
        nodes.iter().map(|node| self.node_to_proposal(node)).collect()
    }

    fn node_to_proposal(&self, node: &ActionNode) -> Proposal {
        let safety_level = safety_level_for_kind(&node.kind);

        Proposal {
            id: ProposalId::new(),
            agent_id: self.agent_id.clone(),
            created_at: now_utc(),
            based_on: vec![ObservationId::from("action-proposal-agent")],
            requested_action: ActionRequest {
                id: node.id.clone(),
                kind: node.kind.clone(),
                safety_level,
            },
            rationale: format!(
                "action from plan step {}: {:?}",
                node.metadata.source_step, node.kind
            ),
            confidence: Confidence(0.9),
        }
    }
}

/// Map ActionKind to an appropriate safety level for governance.
pub fn safety_level_for_kind(kind: &ActionKind) -> ActionSafetyLevel {
    match kind {
        ActionKind::LaunchApplication { .. } => ActionSafetyLevel::MediumRisk,
        ActionKind::OpenUrl { .. } => ActionSafetyLevel::LowRisk,
        ActionKind::RunCommand { .. } => ActionSafetyLevel::HighRisk,
        ActionKind::CreateDirectory { .. } => ActionSafetyLevel::MediumRisk,
        ActionKind::OpenFile { .. } => ActionSafetyLevel::LowRisk,
        ActionKind::CloneRepository { .. } => ActionSafetyLevel::MediumRisk,
        ActionKind::CreateProjectWorkspace { .. } => ActionSafetyLevel::LowRisk,
        // Existing action kinds default to their existing classification
        ActionKind::CgroupCreate { .. } => ActionSafetyLevel::MediumRisk,
        ActionKind::CgroupSetCpuMax { .. } => ActionSafetyLevel::MediumRisk,
        ActionKind::CgroupSetCpuWeight { .. } => ActionSafetyLevel::MediumRisk,
        ActionKind::CgroupSetMemoryMax { .. } => ActionSafetyLevel::MediumRisk,
        ActionKind::CgroupMovePid { .. } => ActionSafetyLevel::HighRisk,
        ActionKind::ProcessFreezeGroup { .. } => ActionSafetyLevel::HighRisk,
        ActionKind::ProcessThawGroup { .. } => ActionSafetyLevel::HighRisk,
        ActionKind::ProcessTerminateGroup { .. } => ActionSafetyLevel::HighRisk,
        ActionKind::WorkloadClassifyRecommend { .. } => ActionSafetyLevel::ReadOnly,
        ActionKind::ObserveOnly => ActionSafetyLevel::ReadOnly,
    }
}

fn now_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()),
        Err(_) => "0.000000000Z".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use agenticos_domain::{
        ActionId, ActionMetadata, ActionStatus, IntentId, PlanId,
    };

    fn sample_node(step: u32, kind: ActionKind) -> ActionNode {
        ActionNode::new(
            ActionId::new(),
            kind,
            HashMap::new(),
            ActionMetadata {
                source_step: step,
                source_plan_id: PlanId::from_string("PlanId-1"),
                source_intent_id: IntentId::from_string("IntentId-1"),
                tool: None,
                capability: None,
            },
        )
    }

    fn sample_graph(nodes: Vec<ActionNode>) -> ActionGraph {
        ActionGraph::new(
            nodes,
            vec![],
            PlanId::from_string("PlanId-1"),
            IntentId::from_string("IntentId-1"),
        )
    }

    #[test]
    fn propose_single_launch_action() {
        let agent = ActionProposalAgent::new(AgentId::from("action-proposal-agent"));
        let node = sample_node(1, ActionKind::LaunchApplication {
            application: "firefox".into(),
        });
        let graph = sample_graph(vec![node]);
        let proposals = agent.propose(&graph);

        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].agent_id.as_str(), "action-proposal-agent");
        assert_eq!(
            proposals[0].requested_action.safety_level,
            ActionSafetyLevel::MediumRisk
        );
        assert!(
            (proposals[0].confidence.0 - 0.9).abs() < f32::EPSILON,
            "expected confidence 0.9, got {}",
            proposals[0].confidence.0
        );
    }

    #[test]
    fn propose_multiple_actions() {
        let agent = ActionProposalAgent::new(AgentId::from("agent"));
        let node1 = sample_node(1, ActionKind::LaunchApplication {
            application: "firefox".into(),
        });
        let node2 = sample_node(2, ActionKind::OpenUrl {
            url: "https://example.com".into(),
        });
        let graph = sample_graph(vec![node1, node2]);
        let proposals = agent.propose(&graph);

        assert_eq!(proposals.len(), 2);
        assert_eq!(
            proposals[0].requested_action.safety_level,
            ActionSafetyLevel::MediumRisk
        );
        assert_eq!(
            proposals[1].requested_action.safety_level,
            ActionSafetyLevel::LowRisk
        );
    }

    #[test]
    fn propose_creates_proposals_with_unique_ids() {
        let agent = ActionProposalAgent::new(AgentId::from("agent"));
        let node1 = sample_node(1, ActionKind::LaunchApplication {
            application: "code".into(),
        });
        let node2 = sample_node(2, ActionKind::CreateDirectory {
            path: "/tmp/test".into(),
        });
        let graph = sample_graph(vec![node1, node2]);
        let proposals = agent.propose(&graph);

        assert_ne!(proposals[0].id, proposals[1].id);
        assert!(proposals[0].rationale.contains("step 1"));
        assert!(proposals[1].rationale.contains("step 2"));
    }

    #[test]
    fn safety_level_mapping() {
        assert_eq!(
            safety_level_for_kind(&ActionKind::LaunchApplication { application: "x".into() }),
            ActionSafetyLevel::MediumRisk
        );
        assert_eq!(
            safety_level_for_kind(&ActionKind::OpenUrl { url: "x".into() }),
            ActionSafetyLevel::LowRisk
        );
        assert_eq!(
            safety_level_for_kind(&ActionKind::RunCommand { command: "x".into(), args: "".into() }),
            ActionSafetyLevel::HighRisk
        );
        assert_eq!(
            safety_level_for_kind(&ActionKind::CreateDirectory { path: "x".into() }),
            ActionSafetyLevel::MediumRisk
        );
        assert_eq!(
            safety_level_for_kind(&ActionKind::OpenFile { path: "x".into() }),
            ActionSafetyLevel::LowRisk
        );
        assert_eq!(
            safety_level_for_kind(&ActionKind::CloneRepository { url: "x".into(), directory: "".into() }),
            ActionSafetyLevel::MediumRisk
        );
        assert_eq!(
            safety_level_for_kind(&ActionKind::CreateProjectWorkspace { project_name: "x".into(), framework: "".into() }),
            ActionSafetyLevel::LowRisk
        );
    }

    #[test]
    fn propose_nodes_subset_works() {
        let agent = ActionProposalAgent::new(AgentId::from("agent"));
        let node1 = sample_node(1, ActionKind::LaunchApplication {
            application: "firefox".into(),
        });
        let _node2 = sample_node(2, ActionKind::OpenUrl {
            url: "https://example.com".into(),
        });
        let proposals = agent.propose_nodes(&[node1]);

        assert_eq!(proposals.len(), 1);
    }

    #[test]
    fn propose_empty_graph() {
        let agent = ActionProposalAgent::new(AgentId::from("agent"));
        let graph = sample_graph(vec![]);
        let proposals = agent.propose(&graph);
        assert!(proposals.is_empty());
    }
}
