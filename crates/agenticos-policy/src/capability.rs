use agenticos_domain::{AgentId, CapabilitySet};

pub struct CapabilityGrant {
    pub agent_id: AgentId,
    pub capabilities: CapabilitySet,
}
