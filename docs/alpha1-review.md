# Alpha-1 Milestone Review

**Date:** 2026-06-02  
**Status:** Complete  
**Tests:** 31 passing, 0 failing, 0 warnings (build)

---

## Completed Milestones

| Milestone | Description | Status |
|-----------|------------|--------|
| A1 | SQLite TraceStore — persistent, replayable event log | Done |
| A2 | Linux observation layer — procfs-based process, memory, and cgroup collectors with noop stubs for non-Linux | Done |
| A3 | Linux cgroup v2 executor — create, set CPU/memory limits, move PID, freeze/thaw/terminate, with JSON-snapshot rollback | Done |
| A4 | Async daemon service loop — tokio-based 1 Hz ticker, config loading, component bootstrap, event bus + trace store integration | Done |
| A5 | Memory agent with rule logic (80% threshold → CgroupSetMemoryMax proposal) + full pipeline test | Done |
| A5.5 | Multi-agent validation — DummyAgentA (conservative), DummyAgentB (aggressive), ordered proposal collection, policy arbitration | Done |
| ADR-0001 | Agent-Governed OS Policy Plane Over Linux | Written |
| ADR-0001 | Use a Rust Workspace | Written |
| ADR-0002 | Clean Architecture Dependency Direction | Written |
| ADR-0003 | Event-Driven Agent Runtime | Written |
| ADR-0003 | Privilege Model — Drop All Capabilities After cgroup Setup | Written |
| ADR-0004 | Use Linux as the Trusted Substrate | Written |
| ADR-0004 | Observation Layer Purity | Written |
| ADR-0005 | Agents Never Hold Privileged Authority | Written |
| ADR-0005 | Observation Sampling | Written |
| ADR-0006 | Executor Authority Boundary | Written |
| ADR-0008 | Multi-Agent Coordination | Written |

### Crate Inventory (11 crates)

| Crate | Lines | Purpose |
|-------|-------|---------|
| `agenticos-domain` | ~350 | Domain types, Agent trait, event envelope, metrics |
| `agenticos-application` | ~100 | Port traits (EventBus, ObserverPort, PolicyKernelPort, etc.), use cases, supervision |
| `agenticos-bus` | ~250 | InMemoryEventBus, InMemoryTraceStore, SqliteTraceStore |
| `agenticos-policy` | ~320 | DeterministicPolicyKernel, DefaultPolicyKernel, action classification |
| `agenticos-runtime` | ~200 | InMemoryAgentRuntime, lifecycle states, ordered agent registry |
| `agenticos-agents` | ~200 | MemoryAgent, DummyAgentA/B, stubs for Process/Security/File/Device/Supervisor |
| `agenticos-observe` | ~350 | SystemSampler, Procfs collectors, noop stubs, procfs parsing |
| `agenticos-executor` | ~350 | DryRunExecutor, LinuxCgroupExecutor (cfg-gated), CgroupRollbackManager, NoopExecutor |
| `agenticos-daemon` | ~250 | DaemonConfig, DaemonContext bootstrap, DaemonService tokio loop, integration tests |
| `agenticos-cli` | ~30 | CLI scaffold (pre-functional) |
| `agenticos-dashboard` | ~30 | Dashboard scaffold (pre-functional) |

---

## Architecture Diagrams

### Component Architecture (Clean Architecture Layers)

```
┌──────────────────────────────────────────────────────────────────────────────┐
│  Layer 4: Entry Points                                                        │
│  ┌──────────────┐  ┌──────────────┐                                            │
│  │ agenticos-cli│  │agenticos-    │                                            │
│  │ (scaffold)   │  │daemon        │                                            │
│  └──────────────┘  └──────┬───────┘                                            │
├────────────────────────────┼──────────────────────────────────────────────────┤
│  Layer 3: Infrastructure   │                                                   │
│  ┌──────────┐ ┌──────────┐ │ ┌──────────┐ ┌──────────┐ ┌──────────┐          │
│  │ agenticos│ │ agenticos│ │ │ agenticos│ │ agenticos│ │ agenticos│          │
│  │ -observe │ │ -executor│ │ │ -bus     │ │ -agents  │ │-dashboard│          │
│  └──────────┘ └──────────┘ │ └──────────┘ └──────────┘ └──────────┘          │
├────────────────────────────┼──────────────────────────────────────────────────┤
│  Layer 2: Application Ports│                                                  │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐                           │
│  │ agenticos    │ │ agenticos    │ │ agenticos    │                           │
│  │ -policy      │ │ -runtime     │ │ -application │                           │
│  └──────────────┘ └──────────────┘ └──────────────┘                           │
├──────────────────────────────────────────────────────────────────────────────┤
│  Layer 0: Domain                                                              │
│  ┌──────────────────────────────────────────────┐                             │
│  │ agenticos-domain                             │                             │
│  │ (Observation, Proposal, Decision, Action,    │                             │
│  │  Agent trait, EventEnvelope, Metrics, IDs)   │                             │
│  └──────────────────────────────────────────────┘                             │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Dependency Direction

```
domain ◄── application ◄── bus, policy, runtime ◄── observe, executor, agents ◄── daemon
         (no cycles, strict layered)
```

---

## Current Event Flow (per daemon tick)

```
┌──────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ Observer  │────►│  Event Bus   │────►│ Trace Store  │     │  Metrics     │
│ (1 Hz)    │     │ (InMemory)   │     │ (SQLite/     │     │  Collector   │
└─────┬─────┘     └──────┬───────┘     │  InMemory)   │     └──────────────┘
      │                  │             └──────────────┘            ▲
      │ observations     │                                         │
      ▼                  │                                         │
┌──────────┐             │                                         │
│  Agent   │             │                                         │
│ Runtime  │             │                                         │
│ (ordered)│             │                                         │
└─────┬────┘             │                                         │
      │ proposals        │                                         │
      ▼                  │                                         │
┌──────────────┐         │                                         │
│   Policy     │         │                                         │
│   Kernel     │─────────┼─────────────────────────────────────────┘
└──────┬───────┘         │            (MetricCollection emitted as
       │ decisions       │             Trace event after each tick)
       ▼                 │
┌──────────────┐         │
│  Executor    │         │
│ (DryRun/     │─────────┘
│  LinuxCgroup)│
└──────┬───────┘
       │ action results
       ▼
  ┌─────────┐
  │ OS State│ (Linux only)
  └─────────┘
```

### Topics Published per Tick

| Topic | Payload | Source |
|-------|---------|--------|
| `observations.<source>` | `EventPayload::Observation` | Observer |
| `proposals.<agent_id>` | `EventPayload::Proposal` | Agent Runtime |
| `decisions.<agent_id>` | `EventPayload::Decision` | Policy Kernel |
| `results.<agent_id>` | `EventPayload::ActionResult` | Executor |
| `system.error` | `EventPayload::Incident` | Any component on failure |
| `metrics.daemon` | `EventPayload::Trace` | Daemon service |

---

## Agent Lifecycle

```
                  ┌──────────┐
                  │ Created   │
                  └─────┬────┘
                        │ runtime.register()
                        ▼
                  ┌──────────┐
                  │Registered│
                  └─────┬────┘
                        │ runtime.start()
                        ▼
                  ┌──────────┐
                  │   Idle    │◄──────────────┐
                  └─────┬────┘                │
                        │ tick begins         │
                        ▼                     │
                  ┌──────────┐                │
                  │ Observing│                │
                  └─────┬────┘                │
                        │ observer.observe()  │
                        ▼                     │
                  ┌──────────┐                │
                  │ Reasoning│                │
                  └─────┬────┘                │
                        │ agent.propose()     │
                        ▼                     │
                  ┌──────────┐                │
                  │ Proposing│                │
                  └─────┬────┘                │
                        │ proposal emitted     │
                        ▼                     │
                  ┌──────────────┐            │
                  │AwaitingPolicy│            │
                  │  Decision    │            │
                  └──────┬───────┘            │
                         │ policy.evaluate()  │
                         ▼                    │
                  ┌──────────────┐            │
                  │ Evaluating   │            │
                  │   Result     │            │
                  └──────┬───────┘            │
                         │ (always returns     │
                         │  to Idle)          │
                         ▼                    │
                  ┌──────────┐                │
                  │   Idle    ├───────────────┘
                  └──────────┘    next tick

Terminal states: Terminated (runtime.stop()), Degraded (error threshold exceeded)
```

Implementation note: The current `InMemoryAgentRuntime` tracks only `Registered`, `Idle`, and `Terminated` states. The state machine above (Observing → Reasoning → Proposing → AwaitingPolicyDecision → EvaluatingResult) is defined in the `LifecycleState` enum but not yet wired into the daemon service loop. All agents are treated as always-ready.

---

## Policy Lifecycle

```
┌──────────┐     ┌──────────────┐     ┌──────────────┐
│ Proposal  │────►│   Evaluate    │────►│  Decision    │
│ (from     │     │  safety level │     │              │
│  agent)   │     │  action kind  │     │ Approved     │
└──────────┘     │  confidence   │     │ Denied       │
                 │  capabilities │     │ Requires     │
                 └──────┬───────┘     │ Approval     │
                        │             └──────┬───────┘
                        │                     │
                        │  Denied             │ Approved
                        ▼                     ▼
                 ┌──────────────┐     ┌──────────────┐
                 │ DenialReason │     │  Validate +  │
                 │ (logged/     │     │  Create      │
                 │  traced)     │     │ ApprovedAction│
                 └──────────────┘     └──────┬───────┘
                                              │
                                              ▼
                                      ┌──────────────┐
                                      │   Executor    │
                                      └──────────────┘
```

### Policy Evaluation Rules (`DefaultPolicyKernel`)

| Check | Condition | Outcome |
|-------|-----------|---------|
| Confidence | `confidence.0 < config.minimum_confidence` | `Denied { MalformedProposal }` |
| Confidence | `confidence.0 ∉ [0.0, 1.0]` | `Denied { MalformedProposal }` |
| Capability | `action_kind ∉ config.allowed_actions` | `Denied { MissingCapability }` |
| Safety (ReadOnly/LowRisk) | always allowed | `Approved` |
| Safety (MediumRisk) | `config.allow_medium_risk` | `Approved` or `Denied { UnsafeAction }` |
| Safety (HighRisk) | `config.allow_high_risk` | `Approved` or `Denied { UnsafeAction }` |

### Policy Presets

| Preset | Allowed Actions | MediumRisk | HighRisk |
|--------|----------------|------------|----------|
| `safe-local` | `[ObserveOnly]` | Denied | Denied |
| `benchmark` | All except `ProcessTerminateGroup` | Approved | Denied |
| `development` | Same as benchmark | Approved | Denied |

---

## Executor Lifecycle

```
┌──────────────┐
│ ApprovedAction│
└──────┬───────┘
       │
       ▼
┌──────────────────┐
│   Snapshot pre-   │
│   mutation state  │ (LinuxCgroupExecutor only)
└──────┬───────────┘
       │
       ▼
┌──────────────────┐
│   Execute action  │
│   (cgroupfs       │
│    write / kill)  │
└──────┬───────────┘
       │
       ├── Success ──► ┌──────────────┐
       │               │  Rollback    │
       │               │  Token saved │
       │               └──────────────┘
       │
       ├── Failure ──► ┌──────────────┐
       │               │ Error logged  │
       │               │ No OS change  │
       │               └──────────────┘
       │
       ▼
┌──────────────────┐
│   ActionResult    │
│ { status, message,│
│   executed_at,    │
│   duration_ms,    │
│   rollback }      │
└──────────────────┘
```

### Executor Implementations

| Executor | Platform | Behavior |
|----------|----------|----------|
| `DryRunExecutor` | All (default) | Returns `ActionStatus::DryRun`, no OS mutation |
| `LinuxCgroupExecutor` | Linux only | Writes cgroupfs files, captures snapshots, supports rollback |
| `NoopExecutor` | Non-Linux | Returns `ActionStatus::DryRun` (fallback when Linux cfg not active) |

### Action Kinds Supported

| Action Kind | LinuxCgroupExecutor | DryRunExecutor |
|-------------|-------------------|----------------|
| `ObserveOnly` | Returns success, no-op | DryRun |
| `CgroupCreate { name }` | Creates cgroup directory | DryRun |
| `CgroupSetCpuMax { group, quota }` | Writes `cpu.max` | DryRun |
| `CgroupSetMemoryMax { group, bytes }` | Writes `memory.max` | DryRun |
| `CgroupMovePid { group, pid }` | Writes `cgroup.procs` | DryRun |
| `ProcessFreezeGroup { group }` | Writes `cgroup.freeze` | DryRun |
| `ProcessThawGroup { group }` | Writes `cgroup.freeze` | DryRun |
| `ProcessTerminateGroup { group }` | Calls `kill -9` on PIDs | DryRun |

### Rollback

```
CgroupRollbackManager:
  rollback(token) ──► Reads CgroupSnapshot from JSON ──►
    Restore memory.max, cpu.max, move PIDs back ──► ActionResult
```

Unsupported rollback actions return `Failed` with message "no rollback available".

---

## Metrics Collected

Metrics are emitted as `MetricCollection` events on topic `metrics.daemon` once per tick.

| Metric | Type | Description |
|--------|------|-------------|
| `active_agents` | Gauge | Number of registered agents (currently hardcoded to 2.0) |
| `proposal_queue_depth` | Gauge | Number of proposals collected this tick |
| `decision_latency_ms` | Histogram | Cumulative time spent in policy evaluation (ms) |
| `executor_latency_ms` | Histogram | Cumulative time spent in executor (ms) |

Metrics are serialized to JSON and embedded in a `TraceEvent` payload. No dedicated metric export pipeline exists yet (the `MetricExporterPort` trait is defined but unimplemented).

---

## Known Limitations

### Architecture
1. **Single-threaded tick loop** — the full pipeline (observe → propose → evaluate → execute) runs sequentially in one async task. With 50 agents producing 10 proposals each, latency per tick could exceed 1 second.
2. **No agent scheduling** — all agents run every tick regardless of whether they have work to do.
3. **No proposal prioritization** — proposals are processed in registration order. A high-urgency proposal from a later-registered agent must wait.
4. **No budget enforcement** — `AgentBudget` struct exists but is not wired into the policy kernel.
5. **No capability enforcement** — `CapabilityGrant` struct exists but is not wired into the policy kernel.
6. **No invariant enforcement** — `PolicyInvariant` struct exists but is not wired into the policy kernel.
7. **Metrics stored as JSON strings** — `MetricCollection` is serialized and embedded in `TraceEvent` rather than stored in its own table.

### Implementation
8. **Linux-only cgroup operations** — `LinuxCgroupExecutor` is behind `#[cfg(target_os = "linux")]`. All non-Linux platforms fall back to `DryRunExecutor`.
9. **No cgroup hierarchy bootstrap** — the daemon does not create `/sys/fs/cgroup/agenticos/` on startup. This must be done manually or via a systemd unit.
10. **No privilege dropping** — the daemon runs with full privileges. The ADR-0003 privilege model (start as root, create cgroup hierarchy, drop all capabilities) is not implemented.
11. **Observation collectors are hardcoded** — `SystemSampler` always constructs procfs-based collectors. There is no configuration-driven collector selection.
12. **Agent stubs** — `ProcessAgent`, `SecurityAgent`, `FileAgent`, `DeviceAgent`, `SupervisorAgent` exist as identity-only stubs with no `propose()` implementation.

### Testing
13. **No Linux integration tests** — cgroup operations, procfs parsing, and the full pipeline cannot be tested on non-Linux CI runners.
14. **No performance benchmarks** — no tests measure tick latency, throughput, or memory usage.
15. **No fuzzing** — no fuzz tests for procfs parsing or policy evaluation.

---

## Technical Debt

### Immediate
1. **`serde_json` used without feature flags** — `serde_json` is a dependency of `agenticos-daemon` for metrics serialization but is imported without explicit feature configuration.
2. **Dead code warnings** — 5 warnings on `cargo build` for fields in `DaemonContext` and `DaemonConfig` that exist for future milestones.
3. **No `#![deny(warnings)]`** — the workspace does not enforce warning-free builds at the CI level.

### Short-term
4. **`MemoryAgent` tests use trait-dispatch syntax** — tests call `Agent::propose(&agent, ...)` instead of `agent.propose(...)`. This works but is non-idiomatic.
5. **`InMemoryAgentRuntime.state` uses `Mutex`** — the runtime is accessed from a single-threaded async context but uses `Mutex` internally. An `Rc<RefCell<...>>` or `tokio::sync::Mutex` would be more appropriate for async context.
6. **`ProposalQueueDepth` always reads as `Gauge`** — this metric is semantically a gauge, but the name suggests a queue depth counter. Rename or use `Counter`.
7. **No metric prefix/namespace convention** — metric names are bare strings (`active_agents`, `decision_latency_ms`). Future integrations (Prometheus, OpenTelemetry) may need namespacing.

### Long-term
8. **No proper error type** — `AppError` is a single-variant enum `Message(String)`. Structured error types with causes, backtraces, and recovery hints would improve debuggability.
9. **No logging framework** — the daemon uses `println!` and `eprintln!`. A structured logging crate (tracing, log, slog) would be beneficial.
10. **All IDs use `AtomicU64`** — IDs are generated via `static NEXT_ID: AtomicU64`. This is not crash-safe (IDs reset on restart). UUIDs or ULIDs would be more appropriate for distributed tracing.

---

## Research Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Tick latency exceeds 1s with many agents | Medium | High | Profile with realistic agent counts; add concurrent proposal collection |
| cgroup v2 interface changes between kernel versions | Low | High | Pin kernel version (Ubuntu 24.04) + integration tests |
| Rollback is insufficient for complex state | Medium | Medium | Rollback is designed for cgroup limits only; process termination is irreversible |
| Policy kernel is a single point of failure | Low | High | Add policy kernel redundancy in future phases |
| MemoryAgent produces conflicting proposals with ProcessAgent | Medium | Low | Policy arbitration handles conflicting proposals deterministically |
| SQLite write throughput limits tick rate | Low | Medium | SQLite is write-optimized for this workload (<100 writes/sec) |
| No e2e encryption for trace data | Low | Low | Out of scope for Alpha-1; addressed in production hardening |
