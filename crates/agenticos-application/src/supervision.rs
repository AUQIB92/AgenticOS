use agenticos_domain::AgentId;

pub struct AgentHealth {
    pub agent_id: AgentId,
    pub state: AgentState,
}

pub enum AgentState {
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
