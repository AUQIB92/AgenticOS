use agenticos_domain::{AgentId, Proposal};

pub struct SubmitProposalCommand {
    pub proposal: Proposal,
}

pub struct StartAgentCommand {
    pub agent_id: AgentId,
}

pub struct StopAgentCommand {
    pub agent_id: AgentId,
}
