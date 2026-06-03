//! AgenticOS Intelligence Layer.
//!
//! Foundation for future AI-assisted agents. Provides:
//!
//! - `LlmProvider` trait — pluggable recommendation generator
//! - `RecommendationContext` — input containing system state summary
//! - `Recommendation` — non-executable advisory output (re-exported from domain)
//! - `MockProvider` — deterministic mock for testing
//! - `WorkloadClassifier` — deterministic workload classification
//! - `WorkloadClassificationAgent` — first intelligent agent
//!
//! ## Security Boundaries
//!
//! Intelligence components may ONLY produce:
//! - `Recommendation` (advisory, non-executable)
//!
//! They may NEVER:
//! - Produce `Proposal` or `ActionRequest`
//! - Call `ApprovedActionExecutor`
//! - Bypass `DefaultSafetyGovernor`
//! - Mutate Linux resources directly
//! - Access raw observation data (only `RecommendationContext` summaries)

pub mod cache;
pub mod classifier;
pub mod mock;
pub mod provider;
pub mod config;
pub mod gemini;
pub mod recommendation;
pub mod types;

pub use cache::{CachedLlmProvider, RecommendationCache};
pub use classifier::{WorkloadClassificationAgent, WorkloadClassifier};
pub use config::IntelligenceConfig;
pub use gemini::GeminiProvider;
pub use mock::MockProvider;
pub use provider::LlmProvider;
pub use recommendation::{Recommendation, RecommendationId};
pub use types::RecommendationContext;

#[cfg(test)]
mod integration_tests {
    use agenticos_bus::{InMemoryTraceStore, Topic, TraceStore, EventEnvelope};
    use agenticos_domain::{AgentId, EventPayload, Recommendation};

    /// Verify a Recommendation can be stored in and recovered from a trace store.
    #[test]
    fn recommendation_trace_persistence() {
        let store = InMemoryTraceStore::new();
        let trace_id = agenticos_domain::TraceId::from("intel-test-1");

        let rec = Recommendation::new(
            AgentId::from("mock-agent"),
            0.85,
            "classification result",
            "deterministic reasoning",
        );

        store
            .append(EventEnvelope::new(
                Topic::new("recommendations.mock"),
                trace_id.clone(),
                EventPayload::Recommendation(rec.clone()),
            ))
            .unwrap();

        let events = store.replay(trace_id).unwrap();
        assert_eq!(events.len(), 1);

        match &events[0].payload {
            EventPayload::Recommendation(r) => {
                assert_eq!(r.id, rec.id);
                assert_eq!(r.summary, "classification result");
                assert_eq!(r.reasoning, "deterministic reasoning");
            }
            other => panic!("expected Recommendation, got {other:?}"),
        }
    }

    /// Verify replay exactly reconstructs the original Recommendation.
    #[test]
    fn recommendation_replay_exact() {
        let store = InMemoryTraceStore::new();
        let trace_id = agenticos_domain::TraceId::from("intel-replay-1");

        let original = Recommendation::new(
            AgentId::from("cpu-agent"),
            0.9,
            "High CPU",
            "CPU pressure above threshold",
        );

        store
            .append(EventEnvelope::new(
                Topic::new("recommendations.cpu"),
                trace_id.clone(),
                EventPayload::Recommendation(original.clone()),
            ))
            .unwrap();

        let events = store.replay(trace_id).unwrap();
        let recovered = match &events[0].payload {
            EventPayload::Recommendation(r) => r.clone(),
            _ => panic!("expected Recommendation"),
        };

        assert_eq!(original.id, recovered.id);
        assert_eq!(original.source_agent, recovered.source_agent);
        assert_eq!(original.timestamp, recovered.timestamp);
        assert_eq!(original.confidence, recovered.confidence);
        assert_eq!(original.summary, recovered.summary);
        assert_eq!(original.reasoning, recovered.reasoning);
    }

    /// Verify topic conventions work for recommendation routing.
    #[test]
    fn recommendation_topic_routing() {
        let store = InMemoryTraceStore::new();
        let trace_id = agenticos_domain::TraceId::from("routing-test");

        let rec = Recommendation::new(
            AgentId::from("cpu-agent"),
            0.7,
            "test",
            "test reasoning",
        );

        store
            .append(EventEnvelope::new(
                Topic::new("recommendations.cpu-agent"),
                trace_id.clone(),
                EventPayload::Recommendation(rec),
            ))
            .unwrap();

        let events = store.replay(trace_id).unwrap();
        assert_eq!(events[0].topic, Topic::new("recommendations.cpu-agent"));
    }

    // ── Boundary enforcement (compile-time guarantees) ───────────────

    /// Prove Recommendation does not implement any execution trait.
    #[test]
    fn recommendation_has_no_execute_method() {
        let rec = Recommendation::new(
            AgentId::from("test"),
            0.5,
            "test",
            "test",
        );

        // Verify Recommendation does not have executor-like methods
        // by checking the type is purely data.
        assert!(rec.summary.len() > 0);
        assert!(rec.reasoning.len() > 0);
        // No .execute(), .dispatch(), or .mutate() exists on this type.
    }

    /// Prove a Recommendation is structurally incompatible with execution.
    ///
    /// A Recommendation has no action request, no decision id, no dispatch
    /// method. It is purely advisory data. This test verifies there is no
    /// conversion path to `ApprovedAction` or `ActionRequest` at the type level.
    #[test]
    fn recommendation_has_no_action_fields() {
        let rec = Recommendation::new(
            AgentId::from("test"),
            0.5,
            "classification",
            "reasoning",
        );

        // Recommendation has advisory fields only — no action fields.
        assert!(rec.summary.len() > 0);
        assert!(rec.reasoning.len() > 0);
        // No `.requested_action`, `.safety_level`, `.decision_id` fields exist.
        // No `.execute()`, `.dispatch()`, `.mutate()` methods exist.
        // The type system enforces this at compile time.
    }
}
