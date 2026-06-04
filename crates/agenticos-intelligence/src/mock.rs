//! Deterministic mock implementation of `LlmProvider`.
//!
//! `MockProvider` produces a fixed recommendation based on `agent_name`:
//! - `"cpu-agent"` → workload classification with CPU reasoning
//! - `"memory-agent"` → workload classification with memory reasoning
//! - anything else → generic recommendation
//!
//! Deterministic: same input always produces identical output.
//! No randomness, no external APIs, no LLM dependency.

use agenticos_domain::{AgentId, ProviderMetadata, Recommendation};

use crate::provider::LlmProvider;
use crate::types::RecommendationContext;

/// Deterministic mock LLM provider for testing and development.
///
/// Generates recommendations based purely on agent name with
/// a fixed confidence of 0.90. No randomness or external calls.
pub struct MockProvider {
    model_name: String,
}

impl MockProvider {
    pub fn new() -> Self {
        Self {
            model_name: "heuristic".into(),
        }
    }

    pub fn with_model(model_name: impl Into<String>) -> Self {
        Self {
            model_name: model_name.into(),
        }
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmProvider for MockProvider {
    fn generate_recommendation(&self, context: RecommendationContext) -> Recommendation {
        let (summary, reasoning) = match context.agent_name.as_str() {
            "cpu-agent" => (
                "Workload classified as database",
                format!(
                    "High CPU utilization with low process count. {}",
                    context.observation_summary
                ),
            ),
            "memory-agent" => (
                "Workload classified as cache-heavy",
                format!(
                    "Elevated memory pressure with stable CPU. {}",
                    context.observation_summary
                ),
            ),
            _ => (
                "Workload classification requires more data",
                format!(
                    "System state: {}. Agent: {}. Insufficient patterns.",
                    context.system_state_summary, context.agent_name
                ),
            ),
        };

        Recommendation::new(
            AgentId::from(context.agent_name),
            0.90,
            summary,
            reasoning,
        )
        .with_provider(ProviderMetadata::new("mock", &self.model_name, false, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu_context() -> RecommendationContext {
        RecommendationContext::new("cpu 95%", "cpu-agent", "stable")
    }

    fn mem_context() -> RecommendationContext {
        RecommendationContext::new("mem 80%", "memory-agent", "stable")
    }

    #[test]
    fn mock_is_deterministic() {
        let provider = MockProvider::new();
        let r1 = provider.generate_recommendation(cpu_context());
        let r2 = provider.generate_recommendation(cpu_context());
        assert_eq!(r1.summary, r2.summary);
        assert_eq!(r1.reasoning, r2.reasoning);
        assert_eq!(r1.confidence, r2.confidence);
    }

    #[test]
    fn mock_generates_cpu_recommendation() {
        let provider = MockProvider::new();
        let r = provider.generate_recommendation(cpu_context());
        assert!(r.summary.contains("database"));
        assert!(r.reasoning.contains("cpu 95%"));
        assert_eq!(r.confidence, 0.90);
    }

    #[test]
    fn mock_generates_memory_recommendation() {
        let provider = MockProvider::new();
        let r = provider.generate_recommendation(mem_context());
        assert!(r.summary.contains("cache-heavy"));
        assert!(r.reasoning.contains("mem 80%"));
    }

    #[test]
    fn mock_generates_generic_recommendation_for_unknown_agent() {
        let provider = MockProvider::new();
        let ctx = RecommendationContext::new("disk 50%", "unknown-agent", "normal load");
        let r = provider.generate_recommendation(ctx);
        assert!(r.summary.contains("more data"));
        assert!(r.reasoning.contains("normal load"));
    }

    #[test]
    fn mock_recommendation_has_valid_confidence() {
        let provider = MockProvider::new();
        let r = provider.generate_recommendation(cpu_context());
        assert!((0.0..=1.0).contains(&r.confidence));
    }

    #[test]
    fn mock_sets_provider_provenance() {
        let provider = MockProvider::new();
        let r = provider.generate_recommendation(cpu_context());
        let meta = r.provider.expect("MockProvider must set provider metadata");
        assert_eq!(meta.provider_name, "mock");
        assert_eq!(meta.cache_hit, false);
    }

    #[test]
    fn mock_provider_name_serialized_in_json() {
        let provider = MockProvider::with_model("gemini-2.5-flash");
        let r = provider.generate_recommendation(cpu_context());
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"provider_name\":\"mock\""));
        assert!(json.contains("\"model_name\":\"gemini-2.5-flash\""));
        assert!(json.contains("\"cache_hit\":false"));
    }
}
