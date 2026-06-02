# ADR 0010: Incident Handling Model

**Status:** Accepted  
**Date:** 2026-06-02  
**Deciders:** Research team

## Context

Incidents are events that represent security concerns, resource contention, governance violations, policy violations, or component failures. They are produced by agents (including the future Security Agent) and by the daemon itself (e.g., `emit_error`).

If incidents could be modified, deleted, or directly trigger actions, three problems arise:

1. **Audit integrity is lost.** An incident that is modified or deleted after creation cannot serve as a reliable record of what happened.
2. **The Policy Kernel is bypassed.** If an incident directly triggers a cgroup freeze, process termination, or any other OS mutation, the executor authority boundary (ADR-0006) is violated and the governance architecture is undermined.
3. **Observation and remediation become conflated.** The agent that detects a problem also decides how to fix it, violating the separation of concerns between sensing and acting.

The governance model (ADR-0009, `docs/research/governance-model.md`) already establishes that Security Agents emit Incidents, not Actions. However, the lifecycle of an incident — what may and may not be done to it — is not yet specified.

## Decision

Incidents are immutable historical facts.

### Incidents may:

**Be correlated.** Multiple incidents can share a `correlation_id: Option<String>`. This allows grouping related incidents (e.g., three agents reporting the same resource contention, or a cascade of agent failures after an executor error) without modifying any of the individual incident records. Correlation is a query-time concern, not a mutation.

**Be escalated.** An agent or the daemon may emit a new incident with higher severity referencing an earlier incident's `incident_id` in its `description` or via `correlation_id`. Escalation produces a new immutable record; it does not change the original incident. For example:
- An agent emits `Incident(severity: Warning, category: ResourceContention)`.
- After 3 consecutive ticks with no resolution, the daemon emits `Incident(severity: Error, category: ResourceContention)` with the same `correlation_id`, referencing the original incident.

**Be acknowledged.** External operators or future tooling may record acknowledgment events as separate trace entries (e.g., `EventPayload::Trace(TraceEvent { message: "ack: incident <id>" })`). Acknowledgments do not modify the original incident and are not first-class domain events — they are annotations in the trace store.

### Incidents may not:

**Be modified.** Once an `Incident` is published to the event bus and appended to the trace store, its fields (`incident_id`, `category`, `severity`, `source_agent`, `source_observation`, `correlation_id`, `timestamp`, `description`) are immutable. There is no `update_incident()` method on any type, no `PATCH` endpoint, and no mechanism to alter a stored trace record.

**Be deleted.** There is no `delete_incident()` method. The trace store is append-only; records are never removed. (Future compaction or archival is out of scope for Alpha.)

**Directly trigger actions.** An incident never produces an `ApprovedAction`. The daemon service loop does not route `EventPayload::Incident` to the executor. There is no code path in which an incident causes a cgroup write, a process signal, or any other OS mutation.

### How incidents influence governance

Only Policy Kernel evaluation may convert an incident into future governance decisions. The mechanism is indirect and tick-delayed:

1. An agent emits an `Incident` on tick *N*.
2. On tick *N+1*, the Policy Kernel (or a future `SecurityPolicyKernel`) receives the incidents from tick *N* as part of its input — either via the observation layer or via a dedicated incident feed.
3. The Policy Kernel may factor the incident into its evaluation of proposals during tick *N+1*. For example:
   - If a `Security` incident exists for a given cgroup, the kernel may deny resource-increase proposals targeting that cgroup.
   - If an `ExecutorFailure` incident exists, the kernel may switch to `DryRunExecutor` until the incident is acknowledged.

This indirect path preserves the separation between problem detection (agents) and remediation authority (Policy Kernel → Executor).

## Consequences

### Positive
- The trace store is an append-only, immutable record of all incidents — no tampering, no silent deletion.
- The executor authority boundary is preserved: incidents never become actions.
- Correlation, escalation, and acknowledgment are all additive operations that produce new records, never mutations.
- The Policy Kernel can use incidents as input without violating the governance model, because incidents influence decisions indirectly (via observations) rather than directly (as action requests).

### Negative
- Operators cannot correct an incident with wrong severity or category after publication. The correct response is to emit a follow-up incident with the correct data and a correlation reference to the original.
- Escalation requires explicit agent or daemon logic; there is no automatic severity escalation based on time or frequency.
- The indirect incident-to-decision path (tick *N* incident → tick *N+1* policy evaluation) adds one tick of latency before incidents can influence governance.

## Implementation Map

| Concept | Implementation Status |
|---------|----------------------|
| `Incident` struct with `incident_id`, `category`, `severity`, `source_agent`, `source_observation`, `correlation_id`, `timestamp`, `description` | Implemented (`event.rs`) |
| `IncidentCategory` enum (`Security`, `ResourceContention`, `GovernanceViolation`, `PolicyViolation`, `ExecutorFailure`, `AgentFailure`) | Implemented (`event.rs`) |
| `IncidentSeverity` enum (`Info`, `Warning`, `Error`, `Critical`) | Implemented (`event.rs`) |
| `Incident::with_correlation()` builder | Implemented (`event.rs`) |
| Immutability (no update/delete methods) | Enforced by type system; no `update_incident()` or `delete_incident()` exists |
| No direct action trigger | Enforced by daemon service loop: `EventPayload::Incident` is never routed to executor |
| Incident → Policy Kernel feed | Not yet implemented; requires adding incident history to the Policy Kernel's evaluation input |
| Escalation logic (time-based severity bumps) | Not yet implemented; deferred |
| Acknowledgment tracing | Ad-hoc via `TraceEvent`; no first-class acknowledgment type |
| Correlation query support | Deferred — `correlation_id` is stored but no index or query API exists |

## References

- ADR-0006: Executor Authority Boundary
- ADR-0009: Security Agent Authority
- `docs/research/governance-model.md` — Governance specification (§3.5 Observation → Incident)
- `crates/agenticos-domain/src/event.rs` — `Incident`, `IncidentCategory`, `IncidentSeverity`
- `crates/agenticos-daemon/src/service.rs` — `emit_error` (existing incident producer)
