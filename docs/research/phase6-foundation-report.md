# Phase 6.0 — Intelligence Layer Foundation Report

**Date:** 2026-06-02  
**Status:** Complete  
**Crate:** `agenticos-intelligence`

---

## 1. Architecture

```
                          ┌──────────────────────────────────────┐
                          │          RecommendationContext        │
                          │  (observation_summary, agent_name,   │
                          │   system_state_summary)              │
                          └─────────────┬────────────────────────┘
                                        │
                          ┌─────────────▼────────────────────────┐
                          │         LlmProvider trait             │
                          │  generate_recommendation(context)     │
                          │         → Recommendation             │
                          └─────────────┬────────────────────────┘
                                        │
                          ┌─────────────▼────────────────────────┐
                          │            Recommendation             │
                          │  (id, source_agent, timestamp,        │
                          │   confidence, summary, reasoning)     │
                          └─────────────┬────────────────────────┘
                                        │
               ┌────────────────────────┼────────────────────────┐
               │                        │                        │
               ▼                        ▼                        ▼
     ┌──────────────────┐   ┌──────────────────┐   ┌──────────────────┐
     │   EventBus        │   │   TraceStore     │   │   Dashboard      │
     │ (in-process       │   │ (SQLite,         │   │ (future)         │
     │  pub/sub)         │   │  replayable)     │   │                  │
     └──────────────────┘   └──────────────────┘   └──────────────────┘
```

### 1.1 Boundary Diagram

```
Governance Pipeline (unchanged):        Intelligence Layer (new):
                                         
Observation                              RecommendationContext
    │                                         │
    ▼                                         ▼
Proposal ──► Policy ──► Safety ──► Executor   LlmProvider
    │                                         │
    │                                         ▼
    │                                    Recommendation
    │                                         │
    │                                         │ (event bus only)
    │                                         ▼
    │                                    TraceStore
    │
    └──► No path from Recommendation to Proposal, Policy, Safety, or Executor
```

### 1.2 Crate Dependency Graph

```
agenticos-domain
    │
    ├── agenticos-bus
    │       │
    │       └── agenticos-intelligence (dev-dependency only)
    │
    └── agenticos-intelligence
            │
            └── (no external crate dependencies besides domain)
```

---

## 2. Files Changed

### New Files

| File | Lines | Purpose |
|------|-------|---------|
| `crates/agenticos-intelligence/Cargo.toml` | 15 | Crate manifest |
| `crates/agenticos-intelligence/src/lib.rs` | 120 | Crate root, re-exports, integration tests |
| `crates/agenticos-intelligence/src/provider.rs` | 25 | `LlmProvider` trait definition |
| `crates/agenticos-intelligence/src/types.rs` | 53 | `RecommendationContext` struct + tests |
| `crates/agenticos-intelligence/src/recommendation.rs` | 48 | Re-exports `Recommendation` from domain, additional tests |
| `crates/agenticos-intelligence/src/mock.rs` | 103 | `MockProvider` implementation + tests |
| `docs/adr/0012-intelligence-layer-boundary.md` | 143 | Architecture Decision Record |
| `docs/research/phase6-foundation-report.md` | — | This report |

### Modified Files

| File | Change |
|------|--------|
| `Cargo.toml` (workspace root) | Added `crates/agenticos-intelligence` to `members` |
| `crates/agenticos-domain/src/ids.rs` | Added `id_type!(RecommendationId)` |
| `crates/agenticos-domain/src/recommendation.rs` | New module: `Recommendation` struct with `new()`, validation, serde tests |
| `crates/agenticos-domain/src/event.rs` | Added `Recommendation(Recommendation)` variant to `EventPayload` |
| `crates/agenticos-domain/src/lib.rs` | Added `pub mod recommendation; pub use recommendation::*;` |
| `crates/agenticos-domain/Cargo.toml` | Added `serde_json` dev-dependency |
| `crates/agenticos-bus/src/store.rs` | Added `sqlite_recommendation_round_trip` test |

---

## 3. Tests Added

| Test | File | Type | Status |
|------|------|------|--------|
| `recommendation_new_validates_confidence` | domain/recommendation.rs | Unit | ✅ |
| `recommendation_panics_on_bad_confidence` | domain/recommendation.rs | Unit (panic) | ✅ |
| `recommendation_round_trips_via_json` | domain/recommendation.rs | Unit (serde) | ✅ |
| `recommendation_constructs_and_validates` | intelligence/recommendation.rs | Unit | ✅ |
| `recommendation_serde_round_trip` | intelligence/recommendation.rs | Unit (serde) | ✅ |
| `context_round_trips_via_json` | intelligence/types.rs | Unit (serde) | ✅ |
| `mock_is_deterministic` | intelligence/mock.rs | Determinism | ✅ |
| `mock_generates_cpu_recommendation` | intelligence/mock.rs | Behaviour | ✅ |
| `mock_generates_memory_recommendation` | intelligence/mock.rs | Behaviour | ✅ |
| `mock_generates_generic_recommendation` | intelligence/mock.rs | Behaviour | ✅ |
| `mock_recommendation_has_valid_confidence` | intelligence/mock.rs | Validation | ✅ |
| `recommendation_trace_persistence` | intelligence/lib.rs | TraceStore | ✅ |
| `recommendation_replay_exact` | intelligence/lib.rs | Replay | ✅ |
| `recommendation_topic_routing` | intelligence/lib.rs | Topic routing | ✅ |
| `recommendation_has_no_execute_method` | intelligence/lib.rs | Boundary | ✅ |
| `recommendation_has_no_action_fields` | intelligence/lib.rs | Boundary | ✅ |
| `sqlite_recommendation_round_trip` | bus/store.rs | SQLite persistence | ✅ |

Total new tests: **17**  
Total workspace tests: **107** (all pass, no new warnings)

---

## 4. Security Boundaries

### What the Intelligence Layer CAN do

- Produce `Recommendation` values with summary, reasoning, and confidence
- Store recommendations in the trace store (SQLite)
- Route recommendations via the event bus (`recommendations.*` topics)
- Be deterministic (MockProvider) or non-deterministic (future LLM providers)

### What the Intelligence Layer CANNOT do

- Produce `Proposal` or `Incident` values
- Call `ApprovedActionExecutor::execute()`
- Call `DefaultSafetyGovernor::evaluate()`
- Mutate Linux cgroup files
- Access raw `Observation` data (only `RecommendationContext` summaries)
- Bypass the governance pipeline
- Create executable actions

### Enforcement Mechanism

The type system enforces the boundary:

1. `LlmProvider::generate_recommendation()` returns `Recommendation`, not `Proposal` or `ApprovedAction`
2. `Recommendation` has no `requested_action`, `safety_level`, or `decision_id` fields
3. No `From<Recommendation> for ApprovedAction` implementation exists
4. `RecommendationContext` contains only `String` fields — no references to OS state
5. `EventPayload::Recommendation` is a dead-end: no downstream consumer routes it to the executor

---

## 5. Traceability Guarantees

| Property | Guarantee | Mechanism |
|----------|-----------|-----------|
| **Serialization** | Any `Recommendation` can be serialized to JSON | `serde::Serialize` derive |
| **Persistence** | Stored in SQLite traces table | `SqliteTraceStore::append()` |
| **Replay** | Exact reconstruction from trace | `serde_json::from_str()` → `EventPayload::Recommendation` |
| **Ordering** | Chronological within trace_id | Auto-increment `id` in SQLite |
| **Routing** | Topic-based filtering | `"recommendations.{agent_name}"` convention |
| **Causality** | Link to triggering event | Optional `causation_id` in `EventEnvelope` |

---

## 6. Risks

### 6.1 LLM Non-Determinism (Phase 6.1)

Real LLM providers will produce different outputs for the same input (due to temperature, sampling, model updates). This breaks the current determinism guarantee. Mitigation: the `LlmProvider` trait itself is deterministic by contract; real implementations should log model parameters alongside recommendations.

### 6.2 Limited Context

`RecommendationContext` contains only string summaries. Providers that need numeric thresholds, raw metrics, or time-series data will require a context extension. This may introduce a new trait method or a `HashMap<String, String>` extensibility field.

### 6.3 No Feedback Loop

There is no mechanism for the system to evaluate recommendation quality. Future phases should consider a recommendation rating/feedback system that feeds back into provider selection or confidence calibration.

### 6.4 Single-Method Trait

`LlmProvider` has one method. Future requirements (streaming, batching, classification vs. generation) may require trait refactoring or additional traits.

---

## 7. Recommendation for Phase 6.1

### Priority: Intelligence-Driven Agent

Build an `IntelligenceAgent` that:

1. Receives observations (via existing `Agent::propose()` or `Agent::collect_incidents()`)
2. Summarizes them into `RecommendationContext`
3. Calls `LlmProvider::generate_recommendation()`
4. Publishes the `Recommendation` to the event bus
5. Optionally emits an `Incident` with `IncidentCategory::Security` and `IncidentSeverity::Info` to signal "recommendation available"

This agent would be the first consumer of the Phase 6.0 foundation and would validate the full pipeline.

### Second Priority: OpenRouter / OpenAI Provider

Implement a real `LlmProvider` that:

- Sends `RecommendationContext` to an external LLM API
- Parses the response into `Recommendation` fields
- Respects the same `Recommendation` return type (no new paths)
- Uses temperature=0 for determinism where possible

### Third Priority: Context Enrichment

Extend `RecommendationContext` to include:

- Optional structured fields (e.g., `cpu_percent: Option<f64>`)
- `HashMap<String, String>` for extensibility
- Observation count / system fingerprint for debugging

---

## 8. Acceptance Criteria Verification

| Criterion | Status |
|-----------|--------|
| `cargo build` — no errors | ✅ |
| `cargo test` — all 107 tests pass | ✅ |
| No existing governance behavior changes | ✅ (zero changes to policy, safety, executor, daemon) |
| No new warnings | ✅ |
| New crate: `agenticos-intelligence` | ✅ |
| ADR-0012 written | ✅ |
| Phase 6 report written | ✅ |
