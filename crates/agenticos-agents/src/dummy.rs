use std::time::{SystemTime, UNIX_EPOCH};

use agenticos_domain::{
    ActionId, ActionKind, ActionRequest, ActionSafetyLevel, Agent, AgentId, AgentKind,
    CapabilitySet, Confidence, Observation, ObservationPayload, Proposal, ProposalId,
};

/// DummyAgentA proposes a conservative cgroup memory limit increase
/// whenever it sees a non-zero memory observation.
pub struct DummyAgentA {
    id: AgentId,
}

impl DummyAgentA {
    pub fn new(id: AgentId) -> Self {
        Self { id }
    }
}

impl Agent for DummyAgentA {
    fn id(&self) -> AgentId {
        self.id.clone()
    }

    fn kind(&self) -> AgentKind {
        AgentKind::Memory
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::default()
    }

    fn propose(&self, observations: &[Observation]) -> Vec<Proposal> {
        let mut proposals = Vec::new();
        let now = timestamp();

        for obs in observations {
            let mem = match &obs.payload {
                ObservationPayload::Memory(m) => m,
                _ => continue,
            };

            if mem.used_bytes == 0 {
                continue;
            }

            let new_max = (mem.used_bytes as f64 * 1.1) as u64;

            proposals.push(Proposal {
                id: ProposalId::new(),
                agent_id: self.id.clone(),
                created_at: now.clone(),
                based_on: vec![obs.id.clone()],
                requested_action: ActionRequest {
                    id: ActionId::new(),
                    kind: ActionKind::CgroupSetMemoryMax {
                        group: "agenticos".into(),
                        bytes: new_max,
                    },
                    safety_level: ActionSafetyLevel::MediumRisk,
                },
                rationale: format!(
                    "DummyAgentA: conservative increase to {}B",
                    new_max
                ),
                confidence: Confidence(0.85),
            });
        }

        proposals
    }
}

/// DummyAgentB proposes an aggressive cgroup memory limit increase
/// whenever it sees a non-zero memory observation — used to test
/// policy arbitration between competing proposals.
pub struct DummyAgentB {
    id: AgentId,
}

impl DummyAgentB {
    pub fn new(id: AgentId) -> Self {
        Self { id }
    }
}

impl Agent for DummyAgentB {
    fn id(&self) -> AgentId {
        self.id.clone()
    }

    fn kind(&self) -> AgentKind {
        AgentKind::Memory
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::default()
    }

    fn propose(&self, observations: &[Observation]) -> Vec<Proposal> {
        let mut proposals = Vec::new();
        let now = timestamp();

        for obs in observations {
            let mem = match &obs.payload {
                ObservationPayload::Memory(m) => m,
                _ => continue,
            };

            if mem.used_bytes == 0 {
                continue;
            }

            let new_max = (mem.used_bytes as f64 * 1.5) as u64;

            proposals.push(Proposal {
                id: ProposalId::new(),
                agent_id: self.id.clone(),
                created_at: now.clone(),
                based_on: vec![obs.id.clone()],
                requested_action: ActionRequest {
                    id: ActionId::new(),
                    kind: ActionKind::CgroupSetMemoryMax {
                        group: "agenticos".into(),
                        bytes: new_max,
                    },
                    safety_level: ActionSafetyLevel::MediumRisk,
                },
                rationale: format!(
                    "DummyAgentB: aggressive increase to {}B",
                    new_max
                ),
                confidence: Confidence(0.95),
            });
        }

        proposals
    }
}

fn timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()),
        Err(_) => "0.000000000Z".to_owned(),
    }
}
