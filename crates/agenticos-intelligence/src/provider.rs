//! Provider trait for the Intelligence Layer.
//!
//! `LlmProvider` is the sole interface for generating recommendations from
//! system state. Implementations must be:
//!
//! - **Deterministic**: same `RecommendationContext` → same `Recommendation`
//! - **Isolated**: no access to executor, policy, or OS mutation
//! - **Traceable**: output must be serializable through `agenticos-bus`

use crate::types::RecommendationContext;
use agenticos_domain::Recommendation;

/// A provider of intelligence-layer recommendations.
///
/// Implementations may use LLMs, heuristics, or mock data, but MUST:
/// - Accept only `RecommendationContext` (no raw observations, no actions)
/// - Return only `Recommendation` (no `Proposal`, no `Incident`)
/// - Be deterministic for the same context
/// - Never bypass Policy or Safety Governor
pub trait LlmProvider: Send + Sync {
    /// Generate a recommendation based on the given context.
    ///
    /// The returned `Recommendation` is:
    /// - Non-executable (cannot become an OS mutation)
    /// - Purely advisory
    /// - Traceable through the event bus
    fn generate_recommendation(&self, context: RecommendationContext) -> Recommendation;
}

// Allow `Box<dyn LlmProvider>` to be used as a provider (required by CachedLlmProvider).
impl LlmProvider for Box<dyn LlmProvider + '_> {
    fn generate_recommendation(&self, context: RecommendationContext) -> Recommendation {
        (**self).generate_recommendation(context)
    }
}
