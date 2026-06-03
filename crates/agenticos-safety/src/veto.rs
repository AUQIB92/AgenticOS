use agenticos_domain::{DecisionId, ProposalId};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VetoDecision {
    pub decision_id: DecisionId,
    pub proposal_id: ProposalId,
    pub reason: VetoReason,
    pub explanation: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum VetoReason {
    InvalidProposal,
    ConflictingProposals,
    IncidentTriggered,
    SelectiveVeto,
    GovernanceInvariantViolation,
    ResourceLimitsExceeded,
    ActionNotPermitted,
}

impl VetoReason {
    pub fn as_ref(&self) -> &'static str {
        match self {
            Self::InvalidProposal => "invalid-proposal",
            Self::ConflictingProposals => "conflicting-proposals",
            Self::IncidentTriggered => "incident-triggered",
            Self::SelectiveVeto => "selective-veto",
            Self::GovernanceInvariantViolation => "governance-invariant-violation",
            Self::ResourceLimitsExceeded => "resource-limits-exceeded",
            Self::ActionNotPermitted => "action-not-permitted",
        }
    }
}
