//! Shared types for the Intelligence Layer.

/// Context provided to an `LlmProvider` when generating a recommendation.
///
/// Contains only information the provider needs to reason about system state.
/// No executable state, no mutation capabilities, no raw observations.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RecommendationContext {
    /// Human-readable summary of relevant observations.
    pub observation_summary: String,
    /// Name of the agent requesting the recommendation.
    pub agent_name: String,
    /// Human-readable summary of overall system state.
    pub system_state_summary: String,
}

impl RecommendationContext {
    pub fn new(
        observation_summary: impl Into<String>,
        agent_name: impl Into<String>,
        system_state_summary: impl Into<String>,
    ) -> Self {
        Self {
            observation_summary: observation_summary.into(),
            agent_name: agent_name.into(),
            system_state_summary: system_state_summary.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_round_trips_via_json() {
        let ctx = RecommendationContext::new("cpu 80%", "cpu-agent", "normal");
        let json = serde_json::to_string(&ctx).unwrap();
        let back: RecommendationContext = serde_json::from_str(&json).unwrap();
        assert_eq!(back.observation_summary, "cpu 80%");
        assert_eq!(back.agent_name, "cpu-agent");
        assert_eq!(back.system_state_summary, "normal");
    }
}
