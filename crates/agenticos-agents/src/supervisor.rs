use agenticos_domain::{Agent, AgentId, AgentKind, CapabilitySet};

pub struct SupervisorAgent {
    id: AgentId,
}

impl SupervisorAgent {
    pub fn new(id: AgentId) -> Self {
        Self { id }
    }
}

impl Agent for SupervisorAgent {
    fn id(&self) -> AgentId {
        self.id.clone()
    }

    fn kind(&self) -> AgentKind {
        AgentKind::Supervisor
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::default()
    }
}
