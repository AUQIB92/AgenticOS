use crate::{AgentId, Incident, Observation, Proposal};

pub trait Agent: Send + Sync {
    fn id(&self) -> AgentId;
    fn kind(&self) -> AgentKind;
    fn capabilities(&self) -> CapabilitySet;

    /// Examine observations and return zero or more proposals.
    ///
    /// Default implementation returns an empty vec — agents that
    /// only observe and never propose do not need to override.
    fn propose(&self, _observations: &[Observation]) -> Vec<Proposal> {
        Vec::new()
    }

    /// Examine observations and return zero or more incidents.
    ///
    /// Incidents are governance events (security concerns, policy
    /// violations, component failures). They are not proposals and
    /// never trigger actions directly.
    ///
    /// Default implementation returns an empty vec — agents that
    /// do not emit incidents do not need to override.
    fn collect_incidents(&self, _observations: &[Observation]) -> Vec<Incident> {
        Vec::new()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AgentKind {
    Supervisor,
    Memory,
    Process,
    Security,
    File,
    Device,
    Network,
    Benchmark,
    Custom(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CapabilitySet {
    pub capabilities: Vec<Capability>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Capability {
    pub name: String,
    pub scope: CapabilityScope,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CapabilityScope {
    ReadOnly,
    ProposalOnly,
    ApprovedAction,
}
