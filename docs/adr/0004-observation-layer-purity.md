# ADR 0004: Observation Layer Purity

**Status:** Accepted  
**Date:** 2026-06-02  
**Deciders:** Research team

## Context

The observation layer is the system's window into Linux state. If it performs classification, filtering, anomaly scoring, or policy decisions, it ceases to be a neutral data source and becomes an implicit policy layer — which violates the architecture's separation of concerns.

## Decision

The observation layer is strictly a data-collection boundary:

1. **Observations are uninterpreted facts.** No agent logic, classification, threat scoring, or scheduling hints exist in the observation layer.

2. **Every observation carries only:**
   - `observation_id` — unique, monotonic
   - `timestamp` — when collection started
   - `source` — which subsystem (process, memory, cgroup, ...)
   - `collection_duration_ms` — how long collection took
   - `payload` — the raw structured data

3. **No filtering.** All accessible processes, memory regions, or cgroups are collected. Filtering is an agent or policy concern.

4. **No aggregation.** Each process or cgroup is an individual observation. Aggregation (averages, histograms) belongs in the agent layer or benchmark harness.

5. **No state.** Collectors do not maintain cross-sample state. CPU percentages and deltas are computed by consumers from consecutive observations on the bus.

## Consequences

Positive:
- Observations are replayable and verifiable — same system state produces identical observations.
- Agents can choose which observations to act on without missing data due to premature filtering.
- The observation layer can be replaced (procfs → eBPF → netlink) without affecting agents.

Negative:
- Raw observation streams are larger (every PID every cycle).
- CPU percent requires two consecutive observation rounds to compute.
- Benchmark harness must handle unfiltered data volume.
