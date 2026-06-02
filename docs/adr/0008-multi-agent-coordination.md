# ADR 0008: Multi-Agent Coordination

**Status:** Accepted  
**Date:** 2026-06-02  
**Deciders:** Research team

## Context

AgenticOS must support multiple autonomous agents observing the same system state and proposing resource-management actions simultaneously. The architecture must handle:

- **Concurrent proposals** — two or more agents may produce proposals from the same observation.
- **Deterministic ordering** — the order in which proposals are evaluated must be predictable and auditable.
- **Policy arbitration** — a single Policy Kernel evaluates every proposal, determining approval or denial based on a uniform policy configuration.
- **Trace integrity** — every event (observation, proposal, decision, action result) must be recorded in the trace store, forming a complete audit trail.

Without explicit coordination infrastructure, agents could produce conflicting proposals, the pipeline could drop or reorder events, and the trace could become inconsistent.

## Decision

### 1. Agent Registration Order Defines Evaluation Order

Agents are registered in an ordered list (`Vec<AgentId>` in `InMemoryAgentRuntime`). Proposal collection iterates agents in registration order, and proposals are collected in a flat `Vec<Proposal>` preserving that order.

This means:
- Agent A (registered first) always contributes its proposals before Agent B (registered second).
- The Policy Kernel evaluates proposals in the order they appear in the collected vec.
- The evaluation order is deterministic, reproducible, and auditable from the trace.

### 2. Single-Threaded Tick Loop

Each daemon tick processes the entire pipeline sequentially within a single async task:

```
Observe → Collect Proposals → For each proposal:
  Evaluate (Policy Kernel) → If approved: Execute (Executor)
```

There is no concurrent proposal processing, no speculative execution, and no out-of-order completion. This guarantees that decisions are made in a well-defined order and that the trace store receives events in causal order.

### 3. All Events Are Traced

Every event in the pipeline is published to the event bus and appended to the trace store:
- Observations → topic `observations.<source>`
- Proposals → topic `proposals.<agent_id>`
- Decisions → topic `decisions.<agent_id>`
- ActionResults → topic `results.<agent_id>`
- Errors → topic `system.error`
- Metrics → topic `metrics.daemon`

### 4. Default Agents

On startup, the daemon registers two default agents:
- `DummyAgentA` — proposes a conservative (10%) cgroup memory limit increase when memory usage > 0.
- `DummyAgentB` — proposes an aggressive (50%) cgroup memory limit increase when memory usage > 0.

These agents validate the multi-agent pipeline under low-stakes conditions. Production deployments replace these with policy-driven agents (MemoryAgent, ProcessAgent, etc.).

## Consequences

### Positive
- Deterministic, auditable proposal ordering — no race conditions or non-determinism.
- Complete audit trail — every event is recorded in the trace store with causal ordering.
- Policy arbitration is uniform — a single Policy Kernel evaluates all proposals, so no agent can bypass policy.
- Simple implementation — the single-threaded tick loop avoids locking, channels, or distributed coordination.

### Negative
- Sequential evaluation limits throughput — a single Policy Kernel must evaluate every proposal in order before the next tick begins.
- No load shedding — if 50 agents each produce 10 proposals, the tick loop processes all 500 sequentially. Mitigation: agents are expected to produce no more than 1–3 proposals per tick in normal operation.
- Agents cannot observe each other's proposals within the same tick — proposals from tick N are only visible as observations in tick N+1.

## Architecture Diagram (per tick)

```
                          ┌──────────────────┐
  Observer ──────────────►│   Event Bus      │──► Trace Store
                          └────────┬─────────┘
                                   │ observations
                                   ▼
                          ┌──────────────────┐
                          │  Agent Runtime   │
                          │  (ordered list)  │
                          │  ┌─ Agent A      │
                          │  ├─ Agent B      │
                          │  └─ ...          │
                          └────────┬─────────┘
                                   │ proposals
                                   ▼
                          ┌──────────────────┐
                          │  Policy Kernel   │──► Trace Store
                          └────────┬─────────┘
                                   │ decisions
                                   ▼
                          ┌──────────────────┐
                          │    Executor      │──► Trace Store
                          └────────┬─────────┘
                                   │ action results
                                   ▼
                          ┌──────────────────┐
                          │  Metric Collector│──► Trace Store
                          └──────────────────┘
```
