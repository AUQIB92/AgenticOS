# Alpha-1 Research Findings

**Date:** 2026-06-02  
**Scope:** Phase 0 — core data model, observation, execution, policy, agent scaffold, multi-agent pipeline validation

---

## What Has Been Partially Validated

### Hypothesis 1: Deterministic Policy Mediation is Feasible

**Status:** Partially validated

The `DefaultPolicyKernel` evaluates proposals against a fixed set of rules (confidence, allowed actions, safety level) and produces deterministic decisions. The 5 policy tests confirm that the same proposal + same config always produces the same outcome.

**Remaining questions:**
- How does latency scale with 10+ proposals per tick? Currently untested.
- Does deterministic mediation hold when the policy config changes at runtime? Config reload is not implemented.
- Can the policy kernel express priority inversion (emergency override)? Currently no mechanism.

### Hypothesis 2: Clean Architecture Prevents Dependency Cycles

**Status:** Validated

The dependency graph is strictly layered:
```
domain → application → {bus, policy, runtime} → {observe, executor, agents, dashboard} → {daemon, cli}
```

No internal crate depends on a higher-layer crate. `agenticos-domain` depends only on `serde`. All 11 crates compile with zero cycles.

### Hypothesis 3: Trace Store Provides Complete Audit Trail

**Status:** Partially validated

The `SqliteTraceStore` persists every event envelope (observation, proposal, decision, action result) with causal ordering via `trace_id` and auto-incrementing `id`. The `full_pipeline_traces_all_events` test confirms all 6 events from 2 agents are recorded.

**Remaining questions:**
- Can the trace store survive concurrent daemon restarts? IDs are `AtomicU64` and reset on restart.
- How does SQLite perform with continuous 1 Hz writes + no compaction? Not tested.
- Is replay-from-trace functionally equivalent to live execution? Not tested (no replay mechanism exists beyond `replay()`).

### Hypothesis 4: Multi-Agent Coordination is Deterministic

**Status:** Partially validated

Agent registration order defines proposal evaluation order. The `proposals_are_returned_in_registration_order` test confirms that `DummyAgentA` (registered first) always contributes proposals before `DummyAgentB` (registered second).

**Remaining questions:**
- What happens when an agent is deregistered mid-tick? Not supported (no deregister API).
- What happens when an agent panics during `propose()`? The panic propagates to the daemon tick loop.
- How does the system behave with 0 agents? Trivially: empty proposals vec. No test covers this.

### Hypothesis 5: Executor Authority Boundary is Enforceable

**Status:** Validated at the trait level, unvalidated at the OS level

The `ApprovedActionExecutor` trait and ADR-0006 mandate that only the Executor mutates OS state. On Linux, `LinuxCgroupExecutor` is cfg-gated. On all platforms, `DryRunExecutor` prevents any OS mutation.

**Remaining questions:**
- Does `LinuxCgroupExecutor` correctly capture and restore cgroup state on a real kernel? Not tested (Linux-only, no CI).
- Does the rollback mechanism handle partial failures (e.g., memory.max restored but cpu.max not)? The rollback applies snapshots in order; a failure during restore is not handled.
- Can a malicious agent bypass the executor via procfs writes? The privilege model (drop capabilities after boot) is not implemented.

### Hypothesis 6: Observation Layer Purity is Maintainable

**Status:** Validated

The `procfs` parsing functions are stateless. `SystemSampler` wraps collectors that return raw observations with no filtering, aggregation, or state. The 9 parsing tests confirm correct extraction of numeric values from `/proc/stat`, `/proc/meminfo`, `/proc/<pid>/status`, and `cpu.stat`.

---

## What Remains Unvalidated

### Hypothesis 7: Policy Kernel Can Arbitrate Conflicting Proposals

**Risk:** Medium

Two agents can propose different `CgroupSetMemoryMax` values for the same group. The benchmark policy approves both (both are `MediumRisk`). There is no mechanism for:
- Conflict detection (same action kind + same group)
- Conflict resolution (pick the higher/lower value, reject both, ask for human judgement)
- Budget enforcement (prevent `CgroupCreate` when group limit is reached)

The `AgentBudget`, `CapabilityGrant`, and `PolicyInvariant` structs exist but are not wired into the policy kernel.

### Hypothesis 8: System Degrades Gracefully Under Load

**Risk:** Medium

No load tests exist. Unknowns:
- Tick duration with 500 process observations, 10 cgroup observations, 5 agents × 3 proposals
- SQLite write queue depth under sustained load
- Memory growth of `InMemoryEventBus` over hours of continuous operation

### Hypothesis 9: Rollback is Sufficient for Production

**Risk:** Medium

`CgroupRollbackManager` captures a JSON snapshot of `memory.current`, `memory.max`, `cpu.max`, and PIDs before mutation. Rollback restores these values. Unaddressed:
- What if the cgroup was deleted between snapshot and rollback?
- What if PIDs have exited or been reparented?
- What if the rollback itself fails (e.g., permission denied)?
- No transactional rollback (all-or-nothing across multiple cgroup writes).

### Hypothesis 10: The Three-Plane Architecture Maps to Real Hardware

**Risk:** Low

The architecture assumes a Linux kernel with cgroup v2, procfs, and cgroupfs. These are verified on Ubuntu 24.04. However:
- No integration test runs on actual hardware
- No systemd unit or deployment script exists
- No capability-dropping sequence is implemented (the daemon runs as root)

---

## Experiments Now Possible

### Experiment 1: Tick Latency Profile

**What:** Instrument the daemon service loop with `Instant::now()` at each stage (observe, collect, evaluate, execute). Run on a Linux host with real procfs observations. Measure p50/p95/p99 tick duration.

**Hypothesis:** Tick duration < 200ms for 1 agent × 1 proposal with real observations.

**Success criterion:** 100 consecutive ticks complete in < 200ms each.

### Experiment 2: Conflict Scenario — Two Agents, Same Group

**What:** Register two agents that both propose `CgroupSetMemoryMax` for group `agenticos` with different values (1 GB vs 2 GB). Verify policy arbitrates both, executor applies both (second overwrites first).

**Hypothesis:** The executor applies both actions in order, and the final state reflects the last-executed action.

### Experiment 3: Replay Fidelity

**What:** Record a sequence of 10 ticks (observations → proposals → decisions → results) to the trace store. Write a replayer that reads the trace and replays decisions through the executor. Verify that `replay()` returns events in causal order with identical payloads.

**Hypothesis:** Trace replay produces identical `ActionResult` sequences.

### Experiment 4: Edge Case — Empty Observation Tick

**What:** Run the daemon with no registered agents. Verify the tick loop completes without error and produces `proposal_queue_depth=0`.

**Hypothesis:** The daemon tolerates empty ticks.

### Experiment 5: Edge Case — Policy Denial Burst

**What:** Configure `safe-local` policy (denies all mutations). Register `DummyAgentA` and `DummyAgentB`. Feed memory observations at 95% usage. Verify both proposals are denied, no actions are executed, and the trace contains 2 × denied decisions.

**Hypothesis:** Policy denies can burst without executor-side effects.

### Experiment 6: Memory Agent Parametric Sweep

**What:** Vary the MemoryAgent threshold from 0.1 to 0.9 in 0.1 increments. For each threshold, feed 100 observations at varying usage levels. Measure:
- Proposal count vs threshold
- Proposal latency vs observation count
- False positive rate (proposal when usage < threshold)

**Hypothesis:** Proposal count is inversely proportional to threshold, with no false positives.

### Experiment 7: Concurrent Agent Registration Stress Test

**What:** Register 100 agents sequentially, then call `collect_proposals()` with a single observation. Measure:
- Registration time
- Proposal collection time
- Memory overhead per agent

**Hypothesis:** Agent registration is O(n) and proposal collection is O(n) where n = agent count.

### Experiment 8: SQLite Write Benchmark

**What:** Measure SQLite `append()` latency for event envelopes of varying sizes (empty observation vs full 50-process observation). Test with synchronous vs asynchronous pragma settings.

**Hypothesis:** SQLite append latency < 5ms for typical event sizes.

---

## Key Risk Assessment

| Risk | Impact | Likelihood | Detection | Mitigation |
|------|--------|-----------|-----------|------------|
| Tick > 1s under load | High | Medium | Profile experiment | Limit agents per tick, parallelize policy evaluation |
| Rollback inconsistency | Medium | Low | Manual testing | Add rollback integration tests on Linux |
| Policy kernel SPOF | High | Low | N/A | Deploy policy kernel as separate process (future) |
| Trace store corruption | High | Low | SQLite integrity check | Add periodic `PRAGMA integrity_check` |
| Agent panic crashes daemon | High | Medium | Code review | Wrap `collect_proposals()` in catch_unwind |
| Linux kernel cgroup API drift | Medium | Low | Kernel CI | Pin Ubuntu 24.04 LTS kernel version |

### Security Considerations (Alpha-1)

| Concern | Status | Action Required |
|---------|--------|----------------|
| Daemon runs as root | Not addressed | Implement ADR-0003 privilege model |
| cgroupfs permissions | Not addressed | Implement cgroup delegation |
| Trace store contains all event data | Not addressed | Add encryption at rest (future) |
| No authentication between components | Not addressed | Out of scope for Alpha-1 |
| Proposals not signed | Not addressed | Out of scope for Alpha-1 |

---

## Recommendations for Alpha-2

1. **Instrument the daemon** — add `Instant::now()` timing at each pipeline stage and export as `Histogram` metrics.
2. **Implement privilege dropping** — start as root, create `/sys/fs/cgroup/agenticos/`, delegate to unprivileged user, drop capabilities.
3. **Wire budget/capability/invariant enforcement** — integrate the existing `AgentBudget`, `CapabilityGrant`, and `PolicyInvariant` structs into `DefaultPolicyKernel`.
4. **Add agent deregistration** — `runtime.deregister(agent_id)` to support dynamic agent lifecycles.
5. **Wrap agent proposal collection in `catch_unwind`** — prevent agent panics from crashing the daemon.
6. **Add at least one Linux integration test** — build and run on a Linux host to validate cgroup operations.
7. **Replace `AtomicU64` IDs with UUIDs** — use `uuid` crate for crash-safe, distributed IDs.
