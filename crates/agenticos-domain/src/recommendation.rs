use std::collections::HashMap;

use crate::{AgentId, RecommendationId};

/// Metadata about the provider that generated a recommendation.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderMetadata {
    pub provider_name: String,
    pub model_name: String,
    pub cache_hit: bool,
    pub generation_latency_ms: u64,
    pub extra: HashMap<String, String>,
}

impl ProviderMetadata {
    pub fn new(
        provider_name: impl Into<String>,
        model_name: impl Into<String>,
        cache_hit: bool,
        generation_latency_ms: u64,
    ) -> Self {
        Self {
            provider_name: provider_name.into(),
            model_name: model_name.into(),
            cache_hit,
            generation_latency_ms,
            extra: HashMap::new(),
        }
    }

    pub fn with_extra(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra.insert(key.into(), value.into());
        self
    }
}

/// A non-executable recommendation produced by an intelligence provider.
///
/// Recommendations may:
/// - Classify workload characteristics
/// - Suggest configuration changes
/// - Explain system behaviour
///
/// Recommendations may NOT:
/// - Trigger OS mutations directly
/// - Bypass Policy or Safety Governor
/// - Create executable actions
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Recommendation {
    pub id: RecommendationId,
    pub source_agent: AgentId,
    pub timestamp: String,
    pub confidence: f64,
    pub summary: String,
    pub reasoning: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderMetadata>,
}

impl Recommendation {
    pub fn new(
        source_agent: AgentId,
        confidence: f64,
        summary: impl Into<String>,
        reasoning: impl Into<String>,
    ) -> Self {
        assert!(
            (0.0..=1.0).contains(&confidence),
            "recommendation confidence must be in [0.0, 1.0]"
        );
        Self {
            id: RecommendationId::new(),
            source_agent,
            timestamp: now_utc(),
            confidence,
            summary: summary.into(),
            reasoning: reasoning.into(),
            provider: None,
        }
    }

    pub fn with_provider(mut self, provider: ProviderMetadata) -> Self {
        self.provider = Some(provider);
        self
    }
}

fn now_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()))
        .unwrap_or_else(|_| "0.000000000Z".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_new_validates_confidence() {
        let r = Recommendation::new(
            AgentId::from("test-agent"),
            0.85,
            "Workload classified as batch",
            "High CPU with low memory pressure",
        );
        assert_eq!(r.confidence, 0.85);
        assert_eq!(r.source_agent.as_str(), "test-agent");
        assert_eq!(r.summary, "Workload classified as batch");
        assert!(!r.timestamp.is_empty());
    }

    #[test]
    #[should_panic(expected = "confidence must be in")]
    fn recommendation_panics_on_bad_confidence() {
        Recommendation::new(AgentId::from("test"), 1.5, "bad", "bad");
    }

    #[test]
    fn recommendation_round_trips_via_json() {
        let r = Recommendation::new(
            AgentId::from("agent-1"),
            0.5,
            "test summary",
            "test reasoning",
        );
        let json = serde_json::to_string(&r).unwrap();
        let back: Recommendation = serde_json::from_str(&json).unwrap();
        assert_eq!(r.id, back.id);
        assert_eq!(r.summary, back.summary);
        assert_eq!(r.confidence, back.confidence);
    }
}
