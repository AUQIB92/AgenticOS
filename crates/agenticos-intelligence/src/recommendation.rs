//! Recommendation domain model.
//!
//! Re-exports the canonical `Recommendation` and `RecommendationId` types
//! from `agenticos-domain` and provides the validated constructor.

pub use agenticos_domain::{Recommendation, RecommendationId};

#[cfg(test)]
mod tests {
    use agenticos_domain::{AgentId, Recommendation};

    #[test]
    fn recommendation_constructs_and_validates() {
        let r = Recommendation::new(
            AgentId::from("intel-agent"),
            0.75,
            "High disk I/O detected",
            "Write throughput exceeds 100MB/s for 30s",
        );
        assert_eq!(r.confidence, 0.75);
        assert_eq!(r.source_agent.as_str(), "intel-agent");
        assert!(r.id.as_str().starts_with("RecommendationId-"));
    }

    #[test]
    fn recommendation_serde_round_trip() {
        let r = Recommendation::new(
            AgentId::from("a1"),
            0.5,
            "summary",
            "reasoning",
        );
        let json = serde_json::to_string(&r).unwrap();
        let back: Recommendation = serde_json::from_str(&json).unwrap();
        assert_eq!(r.id, back.id);
        assert_eq!(r.summary, back.summary);
        assert_eq!(r.reasoning, back.reasoning);
    }
}
