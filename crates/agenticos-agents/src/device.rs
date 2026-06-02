use agenticos_domain::{Agent, AgentId, AgentKind, CapabilitySet};

pub struct DeviceAgent {
    id: AgentId,
}

impl DeviceAgent {
    pub fn new(id: AgentId) -> Self {
        Self { id }
    }
}

impl Agent for DeviceAgent {
    fn id(&self) -> AgentId {
        self.id.clone()
    }

    fn kind(&self) -> AgentKind {
        AgentKind::Device
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::default()
    }
}
