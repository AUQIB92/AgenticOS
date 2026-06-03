# ADR 0012: Intelligence Layer Boundary

**Status:** Accepted  
**Date:** 2026-06-02  
**Deciders:** Research team

## Context

AgenticOS has a validated governance pipeline:

```
Observation → Proposal → Policy → Safety → Executor
```

This pipeline is stable, deterministic, and replayable. All existing agents (ProcessAgent, MemoryAgent, SecurityAgent) operate within this model: they produce `Proposal` or `Incident` values that flow through Policy and Safety before reaching the Executor.

Future agents will need intelligence capabilities: the ability to **classify**, **recommend**, **explain**, and **reason** about system state. These capabilities are distinct from governance and execution.

The risk is that intelligence components could:

1. Bypass the governance pipeline entirely
2. Produce executable actions directly
3. Introduce non-determinism (via LLM calls, random sampling, external APIs)
4. Leak raw observation data to external providers
5. Create a path from external input to OS mutation

## Decision

### 1. The Intelligence Layer Produces Only Recommendations

Intelligence components (LLM providers, heuristic classifiers, mock providers) produce a single output type: `Recommendation`.

```rust
pub struct Recommendation {
    pub id: RecommendationId,
    pub source_agent: AgentId,
    pub timestamp: String,
    pub confidence: f64,
    pub summary: String,
    pub reasoning: String,
}
```

A `Recommendation` is:

- **Non-executable.** It has no `requested_action`, `safety_level`, or `decision_id` fields. It cannot be converted into an `ApprovedAction` or `ActionRequest`.
- **Advisory.** It carries no authority to mutate OS state.
- **Traceable.** It is a first-class `EventPayload` variant (`EventPayload::Recommendation`), persistable in `SqliteTraceStore` and replayable.

### 2. LLM Providers Accept Only RecommendationContext

The `LlmProvider` trait has a single method:

```rust
pub trait LlmProvider: Send + Sync {
    fn generate_recommendation(&self, context: RecommendationContext) -> Recommendation;
}
```

`RecommendationContext` is a deliberately limited input type:

```rust
pub struct RecommendationContext {
    pub observation_summary: String,
    pub agent_name: String,
    pub system_state_summary: String,
}
```

The provider never receives:

- Raw `Observation` values
- `Proposal` or `Decision` values
- File descriptors, process IDs, or cgroup paths
- Any reference to the Executor, Policy Kernel, or Safety Governor

### 3. Recommendation Flows Through the Event Bus

```
Agent ──► RecommendationContext
               │
               ▼
    LlmProvider::generate_recommendation()
               │
               ▼
         Recommendation
               │
               ▼
    EventEnvelope { topic: "recommendations.*", payload: Recommendation }
               │
               ├──► EventBus (in-process pub/sub)
               │
               └──► TraceStore (SQLite persistence)
```

Recommendations are **never** routed to the Executor, Policy Kernel, or Safety Governor. They are purely observational data flowing through the event bus for:

- Dashboard display
- Trace replay
- Audit logging
- Future agent consumption (Phase 6.1+)

### 4. The Governance Pipeline Remains Unchanged

No existing crate (`agenticos-policy`, `agenticos-safety`, `agenticos-executor`, `agenticos-daemon`) is modified by this ADR. The Intelligence Layer is an additive concern:

- **No new variants** in `DecisionOutcome` or `DenialReason`
- **No new safety invariants** in `DefaultSafetyGovernor`
- **No new execution paths** in `LinuxCgroupExecutor`
- **No new daemon service logic**

### 5. Determinism Is Preserved

The `MockProvider` produces deterministic output: same `RecommendationContext` → same `Recommendation`. Real LLM providers (Phase 6.1) may introduce non-determinism, but:

- The `Recommendation` itself is always serializable and persistable
- The non-determinism is confined to the provider implementation
- The event bus and trace store operate on the serialized `Recommendation` regardless of how it was generated

### 6. Traceability and Replayability

`EventPayload::Recommendation(Recommendation)` is a variant of the existing event enum. This means:

- All recommendations are stored in the same SQLite `traces` table
- Replay via `TraceStore::replay()` reconstructs the exact `Recommendation` (same id, summary, reasoning, confidence)
- Recommendations are grouped by `trace_id` and ordered by arrival time
- Topic routing follows the existing `"domain.subdomain"` convention (e.g., `"recommendations.cpu-agent"`)

## Consequences

### Positive

- **Clear security boundary.** No path exists from intelligence output to OS mutation. The type system enforces this.
- **Additive change.** No existing governance behavior changes. All 100+ tests pass without modification.
- **Deterministic by default.** The `MockProvider` provides a baseline implementation for testing.
- **Replayable.** Recommendations are first-class events in the trace store.
- **Future-ready.** The `LlmProvider` trait provides a clean extension point for real LLM integration in Phase 6.1.

### Negative

- **No action path.** Recommendations are purely advisory. An intelligence-to-action bridge (e.g., "if recommendation confidence > 0.9, generate a proposal") requires a future ADR.
- **Limited context.** `RecommendationContext` contains only string summaries. Providers that need raw data (e.g., exact CPU percentages, process lists) will require a context extension in a future phase.
- **No feedback loop.** There is no mechanism for the system to tell a provider whether its recommendation was useful. This is deliberate for Phase 6.0 but limits learning.
- **Single-method trait.** `LlmProvider` has one method. Future providers may need batch generation, streaming, or interactive classification, which would require trait changes.

## References

- ADR-0006: Executor Authority Boundary (only ApprovedActionExecutor may mutate OS state)
- ADR-0009: Security Agent Authority (advisory-only agents)
- `crates/agenticos-intelligence/` — Intelligence Layer implementation
- `crates/agenticos-intelligence/src/provider.rs` — `LlmProvider` trait
- `crates/agenticos-intelligence/src/recommendation.rs` — `Recommendation` type
- `crates/agenticos-intelligence/src/types.rs` — `RecommendationContext`
- `crates/agenticos-intelligence/src/mock.rs` — `MockProvider`
- `crates/agenticos-domain/src/recommendation.rs` — Domain-level `Recommendation` definition
- `crates/agenticos-domain/src/event.rs` — `EventPayload::Recommendation` variant
