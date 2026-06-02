use agenticos_domain::{AgentId, AgentKind};

pub struct AgentRegistration {
    pub agent_id: AgentId,
    pub kind: AgentKind,
}
