# Phase 6.2 — Recommendation-to-Proposal Bridge Report

**Date:** 2026-06-03  
**Status:** Complete  
**Crate:** `agenticos-agents` (recommendation_consumer module)

---

## 1. Architecture

```
Observation
    │
    ▼
WorkloadClassificationAgent (Phase 6.1)
    │
    ▼
Recommendation
    │
    ▼
RecommendationConsumerAgent (Phase 6.2)
    │
    ├── consume_recommendation(rec)     ← external caller pushes recommendations
    ├── propose(&observations)           ← Agent trait, drains internal buffer
    │
    ▼
Proposal
    │
    ▼
Policy Kernel
    │
    ▼
Safety Governor
    │
    ▼
Executor
```

### 1.1 Data Flow Detail

```
                    ┌──────────────────────────────────────┐
                    │  RecommendationConsumerAgent          │
                    │                                       │
                    │  pending: Mutex<Vec<Recommendation>>  │
                    │  consumed: Mutex<u64>                 │
                    │  ignored: Mutex<u64>                  │
                    │  converted: Mutex<u64>                │
                    │                                       │
                    │  Agent::propose() {                   │
                    │      let recs = drain_pending();      │
                    │      for rec in recs {                │
                    │          proposals += map(rec);       │
                    │      }                                │
                    │      return proposals;                │
                    │  }                                    │
                    └──────────────────────────────────────┘
                              │
                              ▼
                    ┌──────────────────────┐
                    │  static mapping fn   │
                    │                      │
                    │  Database    → wt 200│
                    │  Interactive → wt 150│
                    │  Batch       → wt  50│
                    │  Build       → wt 300│
                    │  SystemSvc   → wt 100│
                    │  Unknown     → ∅     │
                    └──────────────────────┘
```

---

## 2. Mapping Rules

| Recommendation Class | Action Kind | Target Group | CPU Weight | Safety Level | Confidence Used |
|---------------------|-------------|--------------|-----------|--------------|----------------|
| Database | `CgroupSetCpuWeight` | `agenticos/classified/database` | 200 | LowRisk | Copied from rec |
| Interactive | `CgroupSetCpuWeight` | `agenticos/classified/interactive` | 150 | LowRisk | Copied from rec |
| Batch | `CgroupSetCpuWeight` | `agenticos/classified/batch` | 50 | LowRisk | Copied from rec |
| Build | `CgroupSetCpuWeight` | `agenticos/classified/build` | 300 | LowRisk | Copied from rec |
| SystemService | `CgroupSetCpuWeight` | `agenticos/classified/system-service` | 100 | LowRisk | Copied from rec |
| Unknown | — | — | — | — | — |

### Rationale Format

All generated proposals include a rationale string linking back to the originating recommendation:

```
Recommendation RecommendationId-42: Database classified workload → set cpu.weight=200
```

This provides traceability from proposal → recommendation via the `proposal.rationale` field.

---

## 3. Agent Implementation Details

### Interior Mutability

The agent uses `Mutex` for interior mutability, required because the `Agent` trait's `propose()` takes `&self` (not `&mut self`):

```rust
pub struct RecommendationConsumerAgent {
    id: AgentId,
    pending: Mutex<Vec<Recommendation>>,
    consumed_count: Mutex<u64>,
    ignored_count: Mutex<u64>,
    converted_count: Mutex<u64>,
}
```

### Buffer Lifecycle

1. **Push**: External code calls `agent.consume_recommendation(rec)` which locks the pending buffer and pushes the recommendation.
2. **Drain**: `Agent::propose()` is called by the agent runtime. It locks the pending buffer, drains all recommendations via `std::mem::take`, and converts them.
3. **Empty**: After `propose()` returns, the pending buffer is empty. A subsequent `propose()` call returns an empty vec.

### Metrics

| Method | Returns | Description |
|--------|---------|-------------|
| `recommendations_consumed()` | `u64` | Total recommendations processed |
| `recommendations_ignored()` | `u64` | Recommendations that produced no proposals (Unknown class) |
| `recommendations_converted()` | `u64` | Total proposals generated |

---

## 4. Causation Chain

The spec requires: "Maintain: Recommendation ID, Proposal ID, Causation ID. Allow replay: Recommendation → Proposal."

### Current Implementation

- **Recommendation ID** is preserved in the proposal's `rationale` field: `"Recommendation {id}: ..."`
- **Proposal ID** is auto-generated via `ProposalId::new()`
- **Causation** is implicit via the rationale string — replay can trace Proposal → Recommendation by parsing `proposal.rationale`

### Full Event Causation (Future)

The `EventEnvelope` has a `causation_id: Option<MessageId>` field. When a recommendation event is published with `MessageId`, the resulting proposal's event envelope can set `causation_id` to link back. This requires orchestration-level wiring (the daemon/bench) and is not implemented inside the agent (which only produces Proposal values, not EventEnvelopes).

---

## 5. Tests Added

| Test | Type | What it verifies |
|------|------|-----------------|
| `database_generates_proposal` | Unit | Database → CgroupSetCpuWeight weight=200, LowRisk |
| `interactive_generates_proposal` | Unit | Interactive → weight=150 |
| `batch_generates_proposal` | Unit | Batch → weight=50 |
| `build_generates_proposal` | Unit | Build → weight=300 |
| `system_service_generates_proposal` | Unit | SystemService → weight=100 |
| `unknown_generates_no_proposal` | Unit | Unknown → empty vec |
| `proposal_confidence_matches_recommendation` | Unit | Confidence propagated correctly |
| `consume_and_propose_round_trip` | Integration | Full consume → propose → metrics flow |
| `unknown_recommendation_ignored` | Integration | Unknown increments ignored_count |
| `multiple_recommendations_batched` | Integration | 4 recs → 3 proposals, 1 ignored |
| `propose_drains_pending` | Integration | Second propose returns empty |
| `recommendation_and_proposal_in_trace_store` | Trace | Both rec and proposal in TraceStore, replayable |
| `agent_trait_id_and_kind` | Agent trait | Agent::id(), Agent::kind(), Agent::capabilities() |
| `proposal_passes_through_policy_kernel` | Policy | Proposal approved by DefaultPolicyKernel |
| `multiple_proposals_through_policy` | Policy | 4 proposals all approved |
| `proposal_passes_through_safety_governor` | Safety | Proposal passes DefaultSafetyGovernor |
| `proposal_within_resource_limits` | Safety | weight=300 within limit of 500 |
| `agent_satisfies_send_sync` | Compile-time | Agent trait's Send + Sync bound |

**All 18 tests pass** in the `agenticos-agents` crate. Total workspace: **151 tests**.

---

## 6. Files Changed

### New Files

| File | Purpose |
|------|---------|
| `crates/agenticos-agents/src/recommendation_consumer.rs` | Full agent implementation + 18 tests (~480 lines) |
| `docs/research/phase6-2-bridge-report.md` | This report |

### Modified Files

| File | Change |
|------|--------|
| `crates/agenticos-agents/src/lib.rs` | Added `pub mod recommendation_consumer; pub use recommendation_consumer::*;` |
| `crates/agenticos-agents/Cargo.toml` | Added `agenticos-safety` dev-dependency |
| `crates/agenticos-domain/src/metrics.rs` | Added `with_recommendations_consumed()`, `with_recommendations_ignored()`, `with_recommendations_converted()` |

---

## 7. Hard Constraints Verification

| Constraint | Status |
|------------|--------|
| No real LLMs added | ✅ Agent uses deterministic mapping, no LLM calls |
| No Safety Governor modification | ✅ Zero changes to `agenticos-safety` crate |
| No Policy bypass | ✅ Proposals go through `DefaultPolicyKernel::evaluate_tick()` in tests |
| No direct execution | ✅ Agent only produces Proposals, never executes |
| Governance retains final authority | ✅ Policy + Safety process all proposals |

---

## 8. Orchestration Example

To wire the full pipeline in a daemon or bench:

```rust
// Create agents
let classifier = WorkloadClassificationAgent::new(AgentId::from("classifier"));
let bridge = RecommendationConsumerAgent::new(AgentId::from("bridge"));

// Register bridge in agent runtime (it implements Agent trait)
let mut runtime = InMemoryAgentRuntime::new();
runtime.register(Box::new(bridge)).unwrap();

// Tick loop
loop {
    let observations = observer.observe().unwrap();

    // Phase 6.1: Classify workload
    let recommendation = classifier.classify_workload(&observations);

    // Phase 6.2: Bridge recommendation to proposal
    bridge.consume_recommendation(recommendation);

    // Standard governance pipeline
    let proposals = runtime.collect_proposals(&observations).unwrap();
    let incidents = runtime.collect_incidents(&observations).unwrap();
    let decisions = policy_kernel.evaluate_tick(PolicyInput { ... }).unwrap();
    let safety_output = safety_governor.evaluate(SafetyInput { ... }).unwrap();
    for approved in &safety_output.approved {
        executor.execute(ApprovedAction { ... }).unwrap();
    }
}
```

---

## 9. Limitations

1. **No EventEnvelope causation** — The agent produces `Proposal` values, not `EventEnvelope`s. Full causation tracking (`EventEnvelope::causation_id`) requires orchestration-level wiring.
2. **Static mapping** — Mapping rules are hardcoded. A future phase could make them configurable or data-driven.
3. **Single target group per class** — Each class maps to a fixed cgroup path. Multi-workload environments need per-cgroup agent instances.
4. **No back-pressure** — If proposals are vetoed, the agent has no mechanism to detect this and adjust future mapping.
