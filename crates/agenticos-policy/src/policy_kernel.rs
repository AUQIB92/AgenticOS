use std::time::{SystemTime, UNIX_EPOCH};

use agenticos_application::AppError;
use agenticos_domain::{
    ActionKind, ActionRequest, ActionSafetyLevel, AgentId, ApprovedAction, Confidence, Decision,
    DecisionId, DecisionOutcome, DenialReason, Proposal,
};

use crate::PolicyInput;

pub trait DeterministicPolicyKernel: Send + Sync {
    /// Evaluate all proposals in the context of a full tick snapshot.
    ///
    /// The kernel receives a stable `PolicyInput` snapshot containing all
    /// observations, proposals, incidents, prior decisions, and metrics
    /// for the current tick. Returns one `Decision` per proposal, in the
    /// same order as `input.proposals`.
    fn evaluate_tick(&self, input: &PolicyInput) -> Result<Vec<Decision>, AppError>;

    fn validate_action(
        &self,
        proposal: &Proposal,
        decision: &Decision,
    ) -> Result<Option<ApprovedAction>, AppError>;
}

#[derive(Clone, Debug)]
pub struct DefaultPolicyKernel {
    config: PolicyKernelConfig,
}

#[derive(Clone, Debug)]
pub struct PolicyKernelConfig {
    pub kernel_agent_id: AgentId,
    pub allowed_actions: Vec<ActionKindClass>,
    pub allow_medium_risk: bool,
    pub allow_high_risk: bool,
    pub minimum_confidence: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionKindClass {
    CgroupCreate,
    CgroupSetCpuMax,
    CgroupSetCpuWeight,
    CgroupSetMemoryMax,
    CgroupMovePid,
    ProcessFreezeGroup,
    ProcessThawGroup,
    ProcessTerminateGroup,
    WorkloadClassifyRecommend,
    ObserveOnly,
}

impl DefaultPolicyKernel {
    pub fn new(config: PolicyKernelConfig) -> Self {
        Self { config }
    }

    pub fn safe_local() -> Self {
        Self::new(PolicyKernelConfig {
            kernel_agent_id: AgentId::from("policy-kernel"),
            allowed_actions: vec![ActionKindClass::ObserveOnly],
            allow_medium_risk: false,
            allow_high_risk: false,
            minimum_confidence: 0.0,
        })
    }

    pub fn benchmark() -> Self {
        Self::new(PolicyKernelConfig {
            kernel_agent_id: AgentId::from("policy-kernel"),
            allowed_actions: vec![
                ActionKindClass::ObserveOnly,
                ActionKindClass::CgroupCreate,
                ActionKindClass::CgroupSetCpuMax,
                ActionKindClass::CgroupSetCpuWeight,
                ActionKindClass::CgroupSetMemoryMax,
                ActionKindClass::CgroupMovePid,
                ActionKindClass::ProcessFreezeGroup,
                ActionKindClass::ProcessThawGroup,
                ActionKindClass::WorkloadClassifyRecommend,
            ],
            allow_medium_risk: true,
            allow_high_risk: false,
            minimum_confidence: 0.0,
        })
    }

    fn decision(
        &self,
        proposal: &Proposal,
        outcome: DecisionOutcome,
        explanation: impl Into<String>,
    ) -> Decision {
        Decision {
            id: DecisionId::new(),
            proposal_id: proposal.id.clone(),
            decided_at: unix_timestamp_string(),
            decided_by: self.config.kernel_agent_id.clone(),
            outcome,
            explanation: explanation.into(),
        }
    }

    fn evaluate_action(&self, request: &ActionRequest, confidence: Confidence) -> DecisionOutcome {
        if !(0.0..=1.0).contains(&confidence.0) {
            return DecisionOutcome::Denied {
                reason: DenialReason::MalformedProposal,
            };
        }

        if confidence.0 < self.config.minimum_confidence {
            return DecisionOutcome::Denied {
                reason: DenialReason::MalformedProposal,
            };
        }

        if !self
            .config
            .allowed_actions
            .contains(&ActionKindClass::from(&request.kind))
        {
            return DecisionOutcome::Denied {
                reason: DenialReason::MissingCapability,
            };
        }

        match request.safety_level {
            ActionSafetyLevel::ReadOnly | ActionSafetyLevel::LowRisk => DecisionOutcome::Approved,
            ActionSafetyLevel::MediumRisk if self.config.allow_medium_risk => {
                DecisionOutcome::Approved
            }
            ActionSafetyLevel::HighRisk if self.config.allow_high_risk => DecisionOutcome::Approved,
            _ => DecisionOutcome::Denied {
                reason: DenialReason::UnsafeAction,
            },
        }
    }
}

impl DeterministicPolicyKernel for DefaultPolicyKernel {
    fn evaluate_tick(&self, input: &PolicyInput) -> Result<Vec<Decision>, AppError> {
        input
            .proposals
            .iter()
            .map(|proposal| {
                let outcome =
                    self.evaluate_action(&proposal.requested_action, proposal.confidence);
                let explanation = match &outcome {
                    DecisionOutcome::Approved => "proposal satisfies configured policy",
                    DecisionOutcome::Denied {
                        reason: DenialReason::MalformedProposal,
                    } => "proposal is malformed or below confidence threshold",
                    DecisionOutcome::Denied {
                        reason: DenialReason::MissingCapability,
                    } => "requested action is not allowed by this policy",
                    DecisionOutcome::Denied {
                        reason: DenialReason::UnsafeAction,
                    } => "requested action exceeds configured safety level",
                    DecisionOutcome::Denied { .. } => "proposal denied by policy",
                    DecisionOutcome::RequiresApproval => "proposal requires external approval",
                };
                Ok(self.decision(proposal, outcome, explanation))
            })
            .collect()
    }

    fn validate_action(
        &self,
        proposal: &Proposal,
        decision: &Decision,
    ) -> Result<Option<ApprovedAction>, AppError> {
        if decision.proposal_id != proposal.id {
            return Err(AppError::Message(
                "decision does not belong to proposal".to_owned(),
            ));
        }

        match decision.outcome {
            DecisionOutcome::Approved => Ok(Some(ApprovedAction {
                request: proposal.requested_action.clone(),
                decision_id: decision.id.clone(),
            })),
            DecisionOutcome::Denied { .. } | DecisionOutcome::RequiresApproval => Ok(None),
        }
    }
}

impl From<&ActionKind> for ActionKindClass {
    fn from(value: &ActionKind) -> Self {
        match value {
            ActionKind::CgroupCreate { .. } => Self::CgroupCreate,
            ActionKind::CgroupSetCpuMax { .. } => Self::CgroupSetCpuMax,
            ActionKind::CgroupSetCpuWeight { .. } => Self::CgroupSetCpuWeight,
            ActionKind::CgroupSetMemoryMax { .. } => Self::CgroupSetMemoryMax,
            ActionKind::CgroupMovePid { .. } => Self::CgroupMovePid,
            ActionKind::ProcessFreezeGroup { .. } => Self::ProcessFreezeGroup,
            ActionKind::ProcessThawGroup { .. } => Self::ProcessThawGroup,
            ActionKind::ProcessTerminateGroup { .. } => Self::ProcessTerminateGroup,
            ActionKind::WorkloadClassifyRecommend { .. } => Self::WorkloadClassifyRecommend,
            ActionKind::ObserveOnly => Self::ObserveOnly,
        }
    }
}

fn unix_timestamp_string() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("{}.{:09}Z", duration.as_secs(), duration.subsec_nanos()),
        Err(_) => "0.000000000Z".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{
        ActionId, MetricCollection, ObservationId, ProposalId,
    };

    fn single_proposal_input(proposal: Proposal) -> PolicyInput {
        PolicyInput {
            tick: 1,
            observations: vec![],
            proposals: vec![proposal],
            incidents: vec![],
            prior_decisions: vec![],
            metrics: MetricCollection {
                source: "test".into(),
                samples: vec![],
            },
        }
    }

    #[test]
    fn safe_local_approves_observe_only() {
        let kernel = DefaultPolicyKernel::safe_local();
        let proposal = proposal(ActionRequest {
            id: ActionId::from("action-1"),
            kind: ActionKind::ObserveOnly,
            safety_level: ActionSafetyLevel::ReadOnly,
        });

        let decisions = kernel
            .evaluate_tick(&single_proposal_input(proposal.clone()))
            .unwrap();
        let decision = &decisions[0];
        let approved = kernel.validate_action(&proposal, decision).unwrap();

        assert_eq!(decision.outcome, DecisionOutcome::Approved);
        assert!(approved.is_some());
    }

    #[test]
    fn safe_local_denies_cgroup_mutation() {
        let kernel = DefaultPolicyKernel::safe_local();
        let proposal = proposal(ActionRequest {
            id: ActionId::from("action-1"),
            kind: ActionKind::CgroupSetMemoryMax {
                group: "bench".to_owned(),
                bytes: 1024,
            },
            safety_level: ActionSafetyLevel::MediumRisk,
        });

        let decisions = kernel
            .evaluate_tick(&single_proposal_input(proposal.clone()))
            .unwrap();
        let decision = &decisions[0];
        let approved = kernel.validate_action(&proposal, decision).unwrap();

        assert_eq!(
            decision.outcome,
            DecisionOutcome::Denied {
                reason: DenialReason::MissingCapability
            }
        );
        assert!(approved.is_none());
    }

    #[test]
    fn benchmark_policy_allows_medium_risk_cgroup_change() {
        let kernel = DefaultPolicyKernel::benchmark();
        let proposal = proposal(ActionRequest {
            id: ActionId::from("action-1"),
            kind: ActionKind::CgroupSetMemoryMax {
                group: "bench".to_owned(),
                bytes: 1024,
            },
            safety_level: ActionSafetyLevel::MediumRisk,
        });

        let decisions = kernel
            .evaluate_tick(&single_proposal_input(proposal))
            .unwrap();
        let decision = &decisions[0];

        assert_eq!(decision.outcome, DecisionOutcome::Approved);
    }

    #[test]
    fn benchmark_policy_denies_high_risk_termination() {
        let kernel = DefaultPolicyKernel::benchmark();
        let proposal = proposal(ActionRequest {
            id: ActionId::from("action-1"),
            kind: ActionKind::ProcessTerminateGroup {
                group: "bench".to_owned(),
            },
            safety_level: ActionSafetyLevel::HighRisk,
        });

        let decisions = kernel
            .evaluate_tick(&single_proposal_input(proposal))
            .unwrap();
        let decision = &decisions[0];

        assert_eq!(
            decision.outcome,
            DecisionOutcome::Denied {
                reason: DenialReason::MissingCapability
            }
        );
    }

    #[test]
    fn policy_denies_allowed_action_when_risk_is_too_high() {
        let kernel = DefaultPolicyKernel::new(PolicyKernelConfig {
            kernel_agent_id: AgentId::from("policy-kernel"),
            allowed_actions: vec![ActionKindClass::ProcessTerminateGroup],
            allow_medium_risk: true,
            allow_high_risk: false,
            minimum_confidence: 0.0,
        });
        let proposal = proposal(ActionRequest {
            id: ActionId::from("action-1"),
            kind: ActionKind::ProcessTerminateGroup {
                group: "bench".to_owned(),
            },
            safety_level: ActionSafetyLevel::HighRisk,
        });

        let decisions = kernel
            .evaluate_tick(&single_proposal_input(proposal))
            .unwrap();
        let decision = &decisions[0];

        assert_eq!(
            decision.outcome,
            DecisionOutcome::Denied {
                reason: DenialReason::UnsafeAction
            }
        );
    }

    fn proposal(requested_action: ActionRequest) -> Proposal {
        Proposal {
            id: ProposalId::from("proposal-1"),
            agent_id: AgentId::from("agent-1"),
            created_at: "0.000000000Z".to_owned(),
            based_on: vec![ObservationId::from("observation-1")],
            requested_action,
            rationale: "test proposal".to_owned(),
            confidence: Confidence(1.0),
        }
    }
}
