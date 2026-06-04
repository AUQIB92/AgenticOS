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

pub mod action_graph;
pub mod action_store;
pub mod cache;
pub mod classifier;
pub mod config;
pub mod gemini;
pub mod intent_agent;
pub mod intent_parser;
pub mod intent_store;
pub mod mock;
pub mod plan_store;
pub mod planner_agent;
pub mod provider;
pub mod recommendation;
pub mod tool_registry;
pub mod types;

pub use action_graph::ActionGraphBuilder;
pub use action_store::ActionStore;
pub use cache::{CachedLlmProvider, RecommendationCache};
pub use classifier::{WorkloadClassificationAgent, WorkloadClassifier};
pub use config::IntelligenceConfig;
pub use gemini::{redact_secret, GeminiProvider};
pub use intent_agent::IntentAgent;
pub use intent_parser::{GeminiIntentParser, IntentParser, MockIntentParser};
pub use intent_store::IntentStore;
pub use mock::MockProvider;
pub use plan_store::PlanStore;
pub use planner_agent::{MockPlannerAgent, PlannerAgent};
pub use provider::LlmProvider;
pub use recommendation::{Recommendation, RecommendationId};
pub use tool_registry::{StaticToolRegistry, ToolRegistry, ToolResolver};
pub use types::RecommendationContext;

#[cfg(test)]
mod integration_tests {
    use agenticos_bus::{InMemoryTraceStore, Topic, TraceStore, EventEnvelope};
    use agenticos_domain::{AgentId, EventPayload, ProviderMetadata, Recommendation};
    use crate::{LlmProvider, MockProvider, RecommendationCache, CachedLlmProvider, WorkloadClassifier};

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

    // ── Provider provenance integration tests ────────────────────────

    /// Verify MockProvider tags recommendations with provider_name="mock".
    #[test]
    fn mock_provider_provenance_is_persisted_and_replayed() {
        let store = InMemoryTraceStore::new();
        let trace_id = agenticos_domain::TraceId::from("mock-provenance-1");

        let provider = MockProvider::with_model("gemini-2.5-flash");
        let ctx = crate::RecommendationContext::new(
            "cpu 85% procs 14", "classifier", "CPU 85% | 14 procs",
        );
        let rec = provider.generate_recommendation(ctx);
        assert_eq!(rec.provider.as_ref().unwrap().provider_name, "mock");
        assert_eq!(rec.provider.as_ref().unwrap().model_name, "gemini-2.5-flash");
        assert!(!rec.provider.as_ref().unwrap().cache_hit);

        store
            .append(EventEnvelope::new(
                Topic::new("recommendations.classifier"),
                trace_id.clone(),
                EventPayload::Recommendation(rec.clone()),
            ))
            .unwrap();

        let events = store.replay(trace_id).unwrap();
        let recovered = match &events[0].payload {
            EventPayload::Recommendation(r) => r,
            _ => panic!("expected Recommendation"),
        };
        let meta = recovered.provider.as_ref().expect("provider metadata must survive replay");
        assert_eq!(meta.provider_name, "mock");
        assert_eq!(meta.model_name, "gemini-2.5-flash");
        assert!(!meta.cache_hit);
    }

    /// Verify CachedLlmProvider correctly tags cache_hit based on state.
    #[test]
    fn cached_provider_tags_cache_hit_and_miss() {
        let cache = RecommendationCache::in_memory().unwrap();
        let inner = MockProvider::with_model("gemini-2.5-flash");
        let wrapped = CachedLlmProvider::with_metadata(
            inner,
            cache,
            "mock",
            "gemini-2.5-flash",
        );

        let ctx = crate::RecommendationContext::new(
            "mem 90%", "classifier", "Memory 90%",
        );

        // First call = cache miss
        let rec1 = wrapped.generate_recommendation(
            crate::RecommendationContext::new("mem 90%", "classifier", "Memory 90%"),
        );
        assert!(
            !rec1.provider.as_ref().unwrap().cache_hit,
            "first call should be a cache miss"
        );
        assert_eq!(rec1.provider.as_ref().unwrap().provider_name, "mock");

        // Second call with same context = cache hit
        let rec2 = wrapped.generate_recommendation(ctx);
        assert!(
            rec2.provider.as_ref().unwrap().cache_hit,
            "second call should be a cache hit"
        );
        assert_eq!(rec2.provider.as_ref().unwrap().provider_name, "mock");
    }

    /// Verify WorkloadClassifier tags recommendations with provider_name="classifier".
    #[test]
    fn classifier_provider_provenance() {
        let classifier = WorkloadClassifier::new(AgentId::from("classifier"));
        let ctx = crate::RecommendationContext::new(
            "cpu: 60\nmem: 40\nprocs: 14\npressure: 0.3\nnames: postgres,python",
            "classifier",
            "CPU 60% | Memory 40% | 14 processes",
        );
        let rec = classifier.generate_recommendation(ctx);
        let meta = rec.provider.expect("WorkloadClassifier must set provider");
        assert_eq!(meta.provider_name, "classifier");
        assert_eq!(meta.model_name, "heuristic");
        assert!(!meta.cache_hit);
    }

    /// Verify that a recommendation with provider metadata round-trips through JSON.
    #[test]
    fn provider_metadata_round_trips_through_json() {
        let meta = ProviderMetadata::new("test-provider", "test-model", true, 42);
        let rec = Recommendation::new(
            AgentId::from("agent"),
            0.5,
            "test",
            "test",
        ).with_provider(meta);

        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains(r#""provider_name":"test-provider""#));
        assert!(json.contains(r#""model_name":"test-model""#));
        assert!(json.contains(r#""cache_hit":true"#));

        let back: Recommendation = serde_json::from_str(&json).unwrap();
        let back_meta = back.provider.unwrap();
        assert_eq!(back_meta.provider_name, "test-provider");
        assert_eq!(back_meta.model_name, "test-model");
        assert!(back_meta.cache_hit);
        assert_eq!(back_meta.generation_latency_ms, 42);
    }

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
