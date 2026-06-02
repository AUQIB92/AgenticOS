use agenticos_domain::{Agent, AgentId, AgentKind, CapabilitySet};

pub struct FileAgent {
    id: AgentId,
}

impl FileAgent {
    pub fn new(id: AgentId) -> Self {
        Self { id }
    }
}

impl Agent for FileAgent {
    fn id(&self) -> AgentId {
        self.id.clone()
    }

    fn kind(&self) -> AgentKind {
        AgentKind::File
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::default()
    }
}
