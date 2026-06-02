use crate::{ActionRequest, AgentId, ObservationId, ProposalId};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Proposal {
    pub id: ProposalId,
    pub agent_id: AgentId,
    pub created_at: String,
    pub based_on: Vec<ObservationId>,
    pub requested_action: ActionRequest,
    pub rationale: String,
    pub confidence: Confidence,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Confidence(pub f32);
