use agenticos_domain::AgentId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentLifecycle {
    pub agent_id: AgentId,
    pub state: LifecycleState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleState {
    Registered,
    Initialized,
    Observing,
    Reasoning,
    Proposing,
    AwaitingPolicyDecision,
    EvaluatingResult,
    Idle,
    Degraded,
    Terminated,
}
