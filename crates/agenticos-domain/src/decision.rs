use crate::{AgentId, DecisionId, ProposalId};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Decision {
    pub id: DecisionId,
    pub proposal_id: ProposalId,
    pub decided_at: String,
    pub decided_by: AgentId,
    pub outcome: DecisionOutcome,
    pub explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DecisionOutcome {
    Approved,
    Denied { reason: DenialReason },
    RequiresApproval,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DenialReason {
    MissingCapability,
    BudgetExceeded,
    InvariantViolation,
    UnsafeAction,
    MalformedProposal,
    Unknown,
}
