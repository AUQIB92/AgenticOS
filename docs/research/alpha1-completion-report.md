# Alpha-1 Completion Report

**Date:** 2026-06-02  
**Status:** Complete  
**ADRs:** 0001–0011 (11 accepted, 0007 unused)  
**Milestone:** Observe → Propose → Decide → Execute pipeline with Safety Governor

---

## 1. Architecture Summary

```
                     ┌─────────────────────────────────────────┐
                     │             Tick Daemon                  │
                     │  (tokio, 1 Hz, single-threaded tick)    │
                     └──────┬──────────────────────┬───────────┘
                            │                      │
              ┌─────────────▼──────────┐   ┌───────▼────────┐
              │     Observation Layer  │   │  Agent Runtime  │
              │  (procfs, cgroupfs)    │   │  (InMemory)     │
              │  SystemSampler         │   │  ┌─────────┐   │
              │  ┌──────────────────┐  │   │  │ ProcA   │   │
              │  │ CpuCollector     │  │   │  │ MemA    │   │
              │  │ MemoryCollector  │  │   │  │ SecA    │   │
              │  │ ProcessCollector │  │   │  │ DummyA/B│   │
              │  │ CgroupCollector  │  │   │  └─────────┘   │
              │  └──────────────────┘  │   └────────────────┘
              └───────────┬────────────┘         │
                          │                      │
                          ▼                      ▼
              ┌──────────────────────────────────────┐
              │         PolicyInput Snapshot          │
              │  (tick, observations, proposals,      │
              │   incidents, prior_decisions, metrics) │
              └──────────────────┬───────────────────┘
                                 │
                                 ▼
              ┌──────────────────────────────────────┐
              │         Policy Kernel                 │
              │  DeterministicPolicyKernel trait      │
              │  DefaultPolicyKernel                  │
              │  evaluate_tick() → Vec<Decision>      │
              │  Config: safe-local / benchmark       │
              └──────────────────┬───────────────────┘
                                 │
                                 ▼
              ┌──────────────────────────────────────┐
              │         Safety Governor                │
              │  DefaultSafetyGovernor                │
              │  evaluate() → filtered decisions       │
              │  Invariants:                          │
              │    • Proposal validation              │
              │    • Incident-triggered veto          │
              │    • Resource limit enforcement       │
              │    • Conflict arbitration             │
              │    • Security Agent enforcement       │
              └──────────────────┬───────────────────┘
                                 │
                                 ▼
              ┌──────────────────────────────────────┐
              │         Executor                      │
              │  ApprovedActionExecutor trait         │
              │  DryRunExecutor (default)             │
              │  LinuxCgroupExecutor (Linux,cfg-gated)│
              │  execute(ApprovedAction) → Result     │
              └──────────────────┬───────────────────┘
                                 │
                                 ▼
              ┌──────────────────────────────────────┐
              │         Linux cgroup v2 APIs          │
              │  /sys/fs/cgroup/ agenticos/           │
              └──────────────────────────────────────┘
```

### Pipeline per tick

1. **Observe** — `SystemSampler` reads procfs (`/proc/stat`, `/proc/meminfo`, `/proc/<pid>/status`, cgroup `cpu.stat`) and returns raw observations
2. **Collect proposals** — each registered agent's `propose()` examines observations, returns `Vec<Proposal>`
3. **Collect incidents** — each agent's `collect_incidents()` examines observations, returns `Vec<Incident>`
4. **Build PolicyInput** — snapshots observations, proposals, incidents into a frozen `PolicyInput`
5. **Policy evaluation** — `evaluate_tick()` maps each proposal to a `Decision` (Approved/Denied)
6. **Safety Governor** — filters approved decisions through governance invariants, emits vetoes and escalations
7. **Execute** — only decisions passing both policy + safety are executed; results traced
8. **Metrics** — tick duration, proposal depth, veto count, safety escalations emitted per tick

---

## 2. Metrics

| Metric | Value |
|--------|-------|
| **Total tests** | 61 |
| **Crates** | 12 |
| **ADRs** | 11 (0001–0011) |
| **Governance invariants** | 5 enforced |

### Test distribution

| Crate | Tests | Focus |
|-------|-------|-------|
| `agenticos-domain` | 0 | Types + derive macros (validated by compilation) |
| `agenticos-application` | 0 | Trait definitions |
| `agenticos-bus` | 4 | Event bus pub/sub, trace store round-trip |
| `agenticos-policy` | 5 | Policy evaluation, risk levels, safe-local vs benchmark |
| `agenticos-runtime` | 3 | Agent lifecycle, registration, proposal collection |
| `agenticos-agents` | 21 | DummyA/B, MemoryAgent, ProcessAgent, **SecurityAgent** (7) |
| `agenticos-observe` | 9 | Procfs parsing (CPU, memory, pressure, cgroup stat) |
| `agenticos-executor` | 1 | DryRun executor returns DryRun status |
| `agenticos-daemon` | 9 | Multi-agent pipeline, evaluate_tick, trace integrity |
| `agenticos-safety` | 9 | Veto logic, conflict arbitration, incident triggers |
| `agenticos-cli` | 0 | CLI scaffold (bin only) |
| `agenticos-dashboard` | 0 | Dashboard scaffold (pre-alpha) |

### Governance invariants enforced

| # | Invariant | Enforced by | ADR |
|---|-----------|-------------|-----|
| 1 | No direct Observation → Action path | Trait design + daemon loop | 0006 |
| 2 | No Agent → Executor path | Trait design + daemon loop | 0006 |
| 3 | Security Agents cannot execute actions | Trait default + Safety Governor | 0009 |
| 4 | PolicyInput evaluation is deterministic per tick | eveluate_tick() semantics | 0011 |
| 5 | Incidents are immutable historical facts | Incident struct (no update/delete methods) | 0010 |

---

## 3. Governance Guarantees

### 3.1 No Observation → Action Path

An agent must never convert an observation directly into an OS mutation.

**Enforcement:**
- Agents return `Vec<Proposal>` or `Vec<Incident>` from `propose()` / `collect_incidents()`. No agent method accepts an `ApprovedAction` or `ActionResult`.
- The daemon service loop never forwards an `Observation` to the Executor. Only `ApprovedAction` values (produced by `PolicyKernel::validate_action()` + Safety Governor filtering) reach the Executor.
- Agents hold no file descriptors to cgroupfs, procfs, or any privileged device.

### 3.2 No Agent → Executor Path

No agent — regardless of `AgentKind`, `CapabilityScope`, or registration order — may call Linux system calls or mutate OS state.

**Enforcement:**
- Agents are `Box<dyn Agent>` trait objects. They receive `&[Observation]` and return `Vec<Proposal>`. They have no access to the executor, the policy kernel, or any `pub` function that performs mutations.
- The daemon drops all Linux capabilities after initialising the cgroup hierarchy (planned — ADR-0003). Without capabilities, even a compromised agent cannot write to cgroupfs.
- Trait-level: `ApprovedActionExecutor` is not part of the `Agent` trait and is not accessible from within `propose()`.

### 3.3 Security Agents Cannot Execute Actions

Security Agent is advisory-only per ADR-0009. This is enforced at two levels:

1. **Trait level:** `Agent` trait provides `collect_incidents()` with a default empty `propose()` — Security Agent implements only `collect_incidents()` and returns an empty proposal vec.
2. **Safety Governor level:** If any proposal from `agent_id == "security-agent"` reaches the Safety Governor, it is vetoed with `ActionNotPermitted`.

### 3.4 Deterministic PolicyInput Evaluation

Per ADR-0011, the Policy Kernel consumes a snapshot-based `PolicyInput` per tick:

```
evaluate_tick(&self, input: &PolicyInput) -> Result<Vec<Decision>, AppError>
```

- The same `PolicyInput` always produces the same `Vec<Decision>` for the same kernel config.
- Per-proposal `evaluate()` is removed in favour of batch `evaluate_tick()`.
- Decisions are returned in the same order as `input.proposals`, preserving registration-order determinism.
- The trace store records every event, enabling offline replay and audit.

### 3.5 Incident Immutability

Per ADR-0010, Incidents are immutable historical facts:

- `Incident` has no `update()` or `delete()` methods.
- Incidents may be **correlated** (`correlation_id`), **escalated** (new incident referencing original), or **acknowledged** (trace annotation on incident topic).
- Incidents never trigger actions directly. They flow into the next tick's `PolicyInput.incidents` field, where the Safety Governor may use them to trigger governance vetoes.

---

## 4. Known Limitations

### 4.1 No Real OS Mutation Tested

`LinuxCgroupExecutor` exists but is never exercised in CI (Windows host). Tests use `DryRunExecutor`, which returns `DryRun` status for every action. The rollback mechanism (`CgroupRollbackManager`, `RollbackToken`) is untested against a real kernel.

### 4.2 Identity System: AtomicU64 (Not Crash-Safe)

All identity types (`ObservationId`, `ProposalId`, `DecisionId`, `IncidentId`, etc.) use `AtomicU64` counters that reset on daemon restart. IDs are unique within a single process lifetime but may collide across restarts or in distributed deployments.

### 4.3 No Agent Deregistration

`InMemoryAgentRuntime` supports `register()` and lifecycle transitions (`start`, `stop`) but not `deregister()`. Agents cannot be removed once registered. This constrains dynamic agent lifecycles.

### 4.4 No Agent Panic Isolation

If an agent's `propose()` or `collect_incidents()` panics, the panic propagates to the daemon tick loop and crashes the process. There is no `catch_unwind` wrapping.

### 4.5 noop Observation Layer on Non-Linux

On Windows, the observation crate returns empty observation vecs. The tick loop completes with zero observations, zero proposals, zero incidents. This is correct for development but provides no validation surface outside Linux.

### 4.6 Single Policy Kernel (SPOF)

`DefaultPolicyKernel` runs in-process within the daemon. There is no mechanism for:
- Hot-reloading policy config
- Running multiple policy kernels for redundancy
- Delegating policy decisions to an external service

### 4.7 No Budget or Capability Enforcement

`AgentBudget`, `CapabilityGrant`, and `PolicyInvariant` structs exist in `agenticos-policy/src/` but are not wired into the policy kernel. Every agent has unlimited proposal budget and no resource quota.

### 4.8 Safety Governor Is Default Implementation

`DefaultSafetyGovernor` is the only Safety Governor implementation. There is no trait abstraction (unlike `DeterministicPolicyKernel` / `ApprovedActionExecutor`). Pluggable safety policies require extracting a `SafetyGovernor` trait.

---

## 5. Alpha-2 Goals

### 5.1 Benchmark Framework

A dedicated benchmark runner that:
- Registers N agents with configurable proposal patterns
- Feeds synthetic observations at configurable rates
- Measures tick latency, proposal throughput, SQLite write latency
- Exports results as structured JSON for comparative analysis
- Runs on Linux with real observations for valid results

### 5.2 Replay Validation

A replayer that reads a trace from `SqliteTraceStore` and replays decisions through the executor, verifying that:
- Replayed events match original events in causal order
- Executor actions produce identical `ActionResult` sequences
- Trace integrity is maintained across daemon restarts

### 5.3 Experiment Runner

An experiment harness that:
- Defines experiment scenarios as TOML configs (agent count, observation patterns, policy mode)
- Runs each scenario for N ticks
- Collects per-tick metrics (latency, veto count, proposal depth)
- Produces comparative reports across scenarios
- Supports parameter sweeps (e.g., MemoryAgent threshold from 0.1 to 0.9)

### 5.4 Comparative Evaluation

Evaluate and document:
- Tick latency with 1 vs 5 vs 20 agents
- Safety Governor overhead (policy-only latency vs policy + safety latency)
- SQLite write throughput under sustained 1 Hz ticking
- DryRun vs simulated LinuxCgroupExecutor execution times
- Baseline: all metrics on a bare Ubuntu 24.04 VM with real procfs/cgroupfs
