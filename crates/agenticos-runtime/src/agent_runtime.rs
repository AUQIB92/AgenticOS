use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use agenticos_application::AppError;
use agenticos_domain::{Agent, AgentId, AgentKind, Incident, Observation, Proposal};

use crate::{AgentLifecycle, LifecycleState};

pub trait AgentRuntime: Send + Sync {
    fn register(&self, agent: Box<dyn Agent>) -> Result<(), AppError>;
    fn start(&self, agent_id: AgentId) -> Result<(), AppError>;
    fn stop(&self, agent_id: AgentId) -> Result<(), AppError>;
    fn collect_proposals(&self, observations: &[Observation]) -> Result<Vec<Proposal>, AppError>;
    fn collect_incidents(&self, observations: &[Observation]) -> Result<Vec<Incident>, AppError>;
}

#[derive(Clone, Default)]
pub struct InMemoryAgentRuntime {
    state: Arc<Mutex<RuntimeState>>,
}

#[derive(Default)]
struct RuntimeState {
    /// Insertion-order list of agent IDs.
    order: Vec<AgentId>,
    /// Agent lookup by ID.
    agents: HashMap<AgentId, RegisteredAgent>,
    lifecycles: HashMap<AgentId, LifecycleState>,
}

struct RegisteredAgent {
    kind: AgentKind,
    agent: Box<dyn Agent>,
}

impl InMemoryAgentRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lifecycle(&self, agent_id: &AgentId) -> Result<Option<AgentLifecycle>, AppError> {
        let state = self.state()?;
        Ok(state
            .lifecycles
            .get(agent_id)
            .cloned()
            .map(|lifecycle_state| AgentLifecycle {
                agent_id: agent_id.clone(),
                state: lifecycle_state,
            }))
    }

    pub fn registered_agents(&self) -> Result<Vec<(AgentId, AgentKind)>, AppError> {
        let state = self.state()?;
        Ok(state
            .order
            .iter()
            .filter_map(|id| {
                state
                    .agents
                    .get(id)
                    .map(|agent| (id.clone(), agent.kind.clone()))
            })
            .collect())
    }

    fn state(&self) -> Result<std::sync::MutexGuard<'_, RuntimeState>, AppError> {
        self.state
            .lock()
            .map_err(|_| AppError::Message("agent runtime lock poisoned".to_owned()))
    }
}

impl AgentRuntime for InMemoryAgentRuntime {
    fn register(&self, agent: Box<dyn Agent>) -> Result<(), AppError> {
        let agent_id = agent.id();
        let kind = agent.kind();
        let mut state = self.state()?;

        if state.agents.contains_key(&agent_id) {
            return Err(AppError::Message(format!(
                "agent already registered: {agent_id}"
            )));
        }

        state.order.push(agent_id.clone());
        state
            .agents
            .insert(agent_id.clone(), RegisteredAgent { kind, agent });
        state
            .lifecycles
            .insert(agent_id, LifecycleState::Registered);

        Ok(())
    }

    fn start(&self, agent_id: AgentId) -> Result<(), AppError> {
        let mut state = self.state()?;

        if !state.agents.contains_key(&agent_id) {
            return Err(AppError::Message(format!("unknown agent: {agent_id}")));
        }

        state.lifecycles.insert(agent_id, LifecycleState::Idle);
        Ok(())
    }

    fn stop(&self, agent_id: AgentId) -> Result<(), AppError> {
        let mut state = self.state()?;

        if !state.agents.contains_key(&agent_id) {
            return Err(AppError::Message(format!("unknown agent: {agent_id}")));
        }

        state
            .lifecycles
            .insert(agent_id, LifecycleState::Terminated);
        Ok(())
    }

    fn collect_proposals(&self, observations: &[Observation]) -> Result<Vec<Proposal>, AppError> {
        let state = self.state()?;
        let mut all_proposals = Vec::new();

        for id in &state.order {
            if let Some(registered) = state.agents.get(id) {
                let proposals = registered.agent.propose(observations);
                all_proposals.extend(proposals);
            }
        }

        Ok(all_proposals)
    }

    fn collect_incidents(&self, observations: &[Observation]) -> Result<Vec<Incident>, AppError> {
        let state = self.state()?;
        let mut all_incidents = Vec::new();

        for id in &state.order {
            if let Some(registered) = state.agents.get(id) {
                let incidents = registered.agent.collect_incidents(observations);
                all_incidents.extend(incidents);
            }
        }

        Ok(all_incidents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{CapabilitySet, ObservationId, ObservationPayload, ObservationSource};

    struct TestAgent {
        id: AgentId,
        kind: AgentKind,
    }

    impl Agent for TestAgent {
        fn id(&self) -> AgentId {
            self.id.clone()
        }

        fn kind(&self) -> AgentKind {
            self.kind.clone()
        }

        fn capabilities(&self) -> CapabilitySet {
            CapabilitySet::default()
        }
    }

    #[test]
    fn registers_and_transitions_agent_lifecycle() {
        let runtime = InMemoryAgentRuntime::new();
        let agent_id = AgentId::from("memory-agent");

        runtime
            .register(Box::new(TestAgent {
                id: agent_id.clone(),
                kind: AgentKind::Memory,
            }))
            .unwrap();

        assert_eq!(
            runtime.lifecycle(&agent_id).unwrap().unwrap().state,
            LifecycleState::Registered
        );

        runtime.start(agent_id.clone()).unwrap();
        assert_eq!(
            runtime.lifecycle(&agent_id).unwrap().unwrap().state,
            LifecycleState::Idle
        );

        runtime.stop(agent_id.clone()).unwrap();
        assert_eq!(
            runtime.lifecycle(&agent_id).unwrap().unwrap().state,
            LifecycleState::Terminated
        );
    }

    #[test]
    fn rejects_duplicate_registration() {
        let runtime = InMemoryAgentRuntime::new();
        let agent_id = AgentId::from("memory-agent");

        runtime
            .register(Box::new(TestAgent {
                id: agent_id.clone(),
                kind: AgentKind::Memory,
            }))
            .unwrap();

        let result = runtime.register(Box::new(TestAgent {
            id: agent_id,
            kind: AgentKind::Memory,
        }));

        assert!(result.is_err());
    }

    #[test]
    fn collect_proposals_from_default_agent_returns_empty() {
        let runtime = InMemoryAgentRuntime::new();
        let agent_id = AgentId::from("silent-agent");

        runtime
            .register(Box::new(TestAgent {
                id: agent_id,
                kind: AgentKind::Security,
            }))
            .unwrap();

        let obs = Observation {
            id: ObservationId::new(),
            source: ObservationSource::Memory,
            observed_at: "0.000000000Z".into(),
            collection_duration_ms: 0,
            payload: ObservationPayload::Empty,
        };

        let proposals = runtime.collect_proposals(&[obs]).unwrap();
        assert!(proposals.is_empty());
    }
}
