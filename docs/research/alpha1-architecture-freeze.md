# AgenticOS Alpha-1 Architecture Freeze

**Date:** 2026-06-02  
**Status:** Active  
**Scope:** All crates under `crates/agenticos-*`  
**Duration:** Until Alpha-2 designation

---

## Frozen Components

The following model types are frozen. No structural changes (field additions, removals, renames, type changes) are permitted during A7/A8 without a new ADR.

### Observation Model

`crates/agenticos-domain/src/observation.rs`

| Type | Status |
|------|--------|
| `Observation` | Frozen |
| `ObservationId` | Frozen |
| `ObservationSource` (all variants) | Frozen |
| `ObservationPayload` (all variants) | Frozen |
| `MemoryObservation` | Frozen |
| `CpuObservation` | Frozen |
| `ProcessObservation` | Frozen |
| `CgroupObservation` | Frozen |

### Proposal Model

`crates/agenticos-domain/src/proposal.rs`

| Type | Status |
|------|--------|
| `Proposal` | Frozen |
| `ProposalId` | Frozen |

### Incident Model

`crates/agenticos-domain/src/event.rs`

| Type | Status |
|------|--------|
| `Incident` | Frozen |
| `IncidentId` | Frozen |
| `IncidentCategory` (all variants) | Frozen |
| `IncidentSeverity` (all variants) | Frozen |

### PolicyInput Model

`crates/agenticos-policy/src/policy_input.rs`

| Type | Status |
|------|--------|
| `PolicyInput` | Frozen |

### Decision Model

`crates/agenticos-domain/src/decision.rs`

| Type | Status |
|------|--------|
| `Decision` | Frozen |
| `DecisionId` | Frozen |
| `DecisionOutcome` (all variants) | Frozen |
| `DenialReason` (all variants) | Frozen |

### Action Model

`crates/agenticos-domain/src/action.rs`

| Type | Status |
|------|--------|
| `ActionRequest` | Frozen |
| `ActionId` | Frozen |
| `ActionKind` (all variants) | Frozen |
| `ActionSafetyLevel` (all variants) | Frozen |
| `ApprovedAction` | Frozen |

### ActionResult Model

`crates/agenticos-domain/src/action.rs`

| Type | Status |
|------|--------|
| `ActionResult` | Frozen |
| `ActionStatus` (all variants) | Frozen |
| `RollbackToken` | Frozen |

---

## Frozen ADRs

All ADRs from 0001 through 0011 are accepted and frozen. No amendments are permitted during A7/A8.

| ADR | Title |
|-----|-------|
| 0001 | Agent-Governed OS Policy Plane |
| 0002 | Clean Architecture |
| 0003 | Privilege Model |
| 0004 | Observation Layer Purity |
| 0005 | Agents Never Hold Privileged Authority |
| 0006 | Executor Authority Boundary |
| 0007 | (unused) |
| 0008 | Multi-Agent Coordination |
| 0009 | Security Agent Authority |
| 0010 | Incident Handling Model |
| 0011 | Policy Input Model |

---

## Not Frozen

The following components are not part of the freeze. They may be modified, extended, or replaced during A7/A8 without requiring a new ADR:

| Component | Rationale |
|-----------|-----------|
| Agent implementations (MemoryAgent, ProcessAgent, SecurityAgent) | New agents, new rules, new proposals — all within the frozen proposal model |
| Policy Kernel implementation (DefaultPolicyKernel) | Logic changes are permitted as long as the `PolicyInput` and `Decision` types remain frozen |
| Executor implementation (DryRunExecutor, LinuxCgroupExecutor) | Bug fixes, new action handlers for existing `ActionKind` variants, rollback improvements |
| Daemon service loop | Tick timing, metrics collection, incident routing — as long as the frozen model types are used |
| Event bus and trace store | Performance improvements, new subscriptions, replay optimization |
| Tests | New tests for agents, policy, executor — verifying frozen model behaviour |

---

## Rules

1. **No architectural changes during A7/A8 except bug fixes.** A bug fix is a change that corrects a deviation from the intended behaviour specified by the frozen ADRs and models. A bug fix does not alter field types, add or remove variants, or change the semantics of a frozen type.

2. **Any architectural modification requires a new ADR and Alpha-2 designation.** If a frozen type must change, the change requires:
   - A new ADR (0012 or higher) describing the modification and its rationale.
   - An Alpha-2 designation in the ADR header (`Designation: Alpha-2`).
   - Review and acceptance by the research team.

3. **ADR numbers 0007 remains unused.** No ADR may be inserted between existing numbers. New ADRs begin at 0012.

---

## Purpose

This freeze preserves a stable baseline for experiments and benchmarking during A7 (Security Agent) and A8 (Safety Governor). By locking the architectural surface, we ensure that:

- Experiments run against a fixed data model and produce comparable results.
- Benchmarks measure performance of the implementation, not churn in the type system.
- The transition from Alpha-1 to Alpha-2 is a deliberate, documented process rather than incremental drift.
- New researchers and contributors can reason about the system from a known, stable reference point.
