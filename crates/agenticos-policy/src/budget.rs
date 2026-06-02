use agenticos_domain::AgentId;

pub struct AgentBudget {
    pub agent_id: AgentId,
    pub max_actions_per_minute: u32,
    pub max_reasoning_latency_ms: u64,
}
