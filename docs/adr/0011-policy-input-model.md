# ADR 0011: Policy Input Model

**Status:** Accepted  
**Date:** 2026-06-02  
**Deciders:** Research team

## Context

The Policy Kernel currently evaluates proposals one at a time via `DeterministicPolicyKernel::evaluate(&self, proposal: &Proposal) -> Result<Decision, AppError>`. Each call sees only the proposal and the kernel's own configuration. It has no visibility into:

- What other proposals exist in the same tick.
- What observations triggered those proposals.
- What incidents are active.
- What decisions were made earlier in the same tick.
- What the system's overall resource state is.

This per-proposal isolation creates two architectural risks:

1. **Event-order dependence.** If the kernel's decision depends on which proposal it sees first, but the kernel cannot see other proposals, then ordering logic must live in the daemon service loop. Today the loop evaluates proposals in registration order (ADR-0008), but there is no architectural guarantee that a later proposal cannot overturn an earlier decision — because the kernel never knows about earlier decisions within the same tick.

2. **Non-deterministic replay.** If replaying a trace requires replaying not just the kernel's decisions but also the exact interleaving of events from the bus, then the replay is fragile. A snapshot-based model captures all relevant state at a single point, making replay deterministic by construction.

The governance model (ADR-0010) also introduces incidents as first-class events that should influence policy evaluation — but the current `evaluate(proposal)` signature has no channel for incidents to reach the kernel.

## Decision

The Policy Kernel does not consume raw events.

The Policy Kernel consumes a `PolicyInput` snapshot.

### PolicyInput structure

```rust
pub struct PolicyInput {
    /// Tick number (monotonically increasing, 1-indexed per daemon lifetime).
    pub tick: u64,

    /// All observations collected during this tick,
    /// keyed by source for efficient lookup.
    pub observations: Vec<Observation>,

    /// All proposals submitted by agents during this tick,
    /// in registration order.
    pub proposals: Vec<Proposal>,

    /// All incidents emitted since the last tick.
    /// Includes incidents from the previous tick's observation window
    /// that have not yet been consumed.
    pub incidents: Vec<Incident>,

    /// Decisions made in prior ticks (bounded window, configurable size).
    /// Used by the kernel to detect repeated patterns, escalation conditions,
    /// or to implement confidence decay.
    pub prior_decisions: Vec<Decision>,

    /// System metrics collected during this tick
    /// (active agents, proposal queue depth, tick duration, etc.).
    pub metrics: MetricCollection,
}
```

### Evaluation becomes once per tick

```rust
pub trait DeterministicPolicyKernel: Send + Sync {
    /// Evaluate all proposals in the context of the full tick snapshot.
    /// Returns one Decision per proposal, in the same order as input.
    fn evaluate_tick(&self, input: &PolicyInput) -> Result<Vec<Decision>, AppError>;
}
```

This replaces the current per-proposal `evaluate(&self, proposal: &Proposal)`.

**Key properties:**

- **All inputs are collected during tick N.** Observations, proposals, incidents, prior decisions, and metrics are gathered at the start of the tick and frozen into a `PolicyInput` snapshot. No new events from tick N+1 can enter the snapshot.

- **Policy evaluation occurs once per tick.** A single call to `evaluate_tick()` processes all proposals. The kernel has full visibility into the entire proposal set, all observations, all active incidents, and the decision history window.

- **The Policy Kernel evaluates a stable snapshot.** The `PolicyInput` is an immutable data structure. The kernel cannot modify it. Decisions are returned as a separate `Vec<Decision>`. This guarantees that the same snapshot always produces the same decisions.

### Incident consumption model

Incidents from tick *N* appear in the `PolicyInput.incidents` field on tick *N+1*. This is the only path by which incidents influence policy:

```
Tick N:
  Agent emits Incident ──► TraceStore
                                │
Tick N+1:                       │
  PolicyInput builds ───────────┘ includes incidents from tick N
    │
    ▼
  evaluate_tick() considers incidents when evaluating proposals
```

This one-tick delay is deliberate (ADR-0010): it separates problem detection from remediation and ensures the incident is durably stored before any policy decision references it.

### Deterministic replay

Because `PolicyInput` is a self-contained snapshot, replay works as follows:

1. Replay the trace store for a given window.
2. Reconstruct `PolicyInput` for each tick from the replayed events (observations, proposals, incidents, prior decisions, metrics).
3. Call `evaluate_tick()` with the reconstructed input.
4. Assert that decisions match the original trace.

There is no dependence on event ordering within the bus, wall-clock timing, or other non-deterministic factors. Every replay of the same tick window produces the same `PolicyInput` and therefore the same decisions.

## Consequences

### Positive

- **Deterministic by construction.** A `PolicyInput` snapshot is a value type: same inputs always yield same decisions. Replay is trivial.
- **Full tick visibility.** The kernel sees all proposals, observations, and incidents before making any decision. It can implement cross-proposal reasoning (e.g., "if two agents propose conflicting limits for the same cgroup, prefer the lower limit").
- **Incidents become first-class policy input.** The kernel can factor active incidents into its decisions without violating the governance model.
- **Cleaner service loop.** The daemon loop becomes `collect_input() → kernel.evaluate_tick(input) → execute(decisions)` instead of `collect_input() → for each proposal: evaluate → if approved: execute`.
- **Easier to test.** Policy tests construct a `PolicyInput` directly without mocking the bus, observer, or agent runtime.

### Negative

- **Breaking change.** The `DeterministicPolicyKernel` trait must change from `evaluate(proposal)` to `evaluate_tick(input)`. All implementations (currently `DefaultPolicyKernel`, plus test/mock kernels) must be updated.
- **Larger API surface.** `PolicyInput` is a new public struct with five fields. Construction and serialization logic must be added.
- **Memory overhead.** The kernel receives all observations and proposals in a single snapshot, which may be larger than per-proposal evaluation. Mitigation: agents are expected to produce 1–3 proposals per tick; observations are bounded by the number of cgroups and processes (typically <1000 on a single node).
- **Prior decision window introduces statefulness.** The kernel must either receive prior decisions in the input (current design) or maintain internal state. The input-based approach preserves replayability but increases `PolicyInput` size.

## Migration Path

1. Define `PolicyInput` struct in `agenticos-domain` (or `agenticos-policy`).
2. Update `DeterministicPolicyKernel` trait: add `evaluate_tick()` with default fallback to `evaluate()` for backward compatibility.
3. Update `DefaultPolicyKernel` to implement `evaluate_tick()`.
4. Update daemon service loop to build `PolicyInput` and call `evaluate_tick()` instead of per-proposal evaluation.
5. Update all call sites (tests, benchmarks) to use the new interface.
6. Remove the per-proposal `evaluate()` method once all call sites are migrated.

## References

- ADR-0008: Multi-Agent Coordination (registration order, tick loop)
- ADR-0010: Incident Handling Model (incidents as immutable facts, one-tick delay)
- `crates/agenticos-policy/src/policy_kernel.rs` — `DeterministicPolicyKernel` trait, `DefaultPolicyKernel`
- `crates/agenticos-daemon/src/service.rs` — Current per-proposal evaluation loop
- `crates/agenticos-domain/src/event.rs` — `Incident`, `IncidentCategory`
- `docs/research/governance-model.md` — Governance specification (§3.5 Observation → Incident, §5 Conflict Resolution)
