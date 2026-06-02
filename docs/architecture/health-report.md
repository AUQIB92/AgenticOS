# Architecture Health Report — Alpha-1

**Date:** 2026-06-02  
**Evaluator:** Automated analysis

---

## Coupling

### Intra-crate Coupling

| Crate | Incoming Dependencies | Outgoing Dependencies | Assessment |
|-------|----------------------|----------------------|------------|
| `agenticos-domain` | 10 | 1 (serde) | **Excellent** — domain is the foundation with zero internal dependencies |
| `agenticos-application` | 8 | 1 (domain) | **Excellent** — thin port layer |
| `agenticos-bus` | 3 | 4 (app, domain, rusqlite, serde_json) | **Good** — depends on application ports |
| `agenticos-policy` | 2 | 2 (app, domain) | **Good** — clean layered dependency |
| `agenticos-runtime` | 2 | 2 (app, domain) | **Good** — clean layered dependency |
| `agenticos-agents` | 1 | 1 (domain) | **Excellent** — only depends on domain |
| `agenticos-observe` | 2 | 2 (app, domain) | **Good** — clean layered dependency |
| `agenticos-executor` | 1 | 4 (app, domain, serde, serde_json) | **Good** — would benefit from serialization abstraction |
| `agenticos-daemon` | 8 | 12 (7 internal + 5 external) | **Acceptable** — entry point naturally has many deps |
| `agenticos-cli` | 0 | 0 | **Excellent** — isolated scaffold |
| `agenticos-dashboard` | 1 | 1 (domain) | **Excellent** — isolated scaffold |

### Inter-crate Coupling Analysis

```
domain → application → {bus, policy, runtime} → {observe, executor, agents} → daemon
```

- **Cycle detection:** None. The dependency graph is a directed acyclic graph (DAG).
- **Hub score:** `agenticos-daemon` is the hub (8 incoming edges). This is expected for an entry-point crate.
- **Tightest coupling:** `agenticos-bus` ↔ `agenticos-application` (EventBus trait implemented by InMemoryEventBus). Coupling is through a trait, which is acceptable.

### Coupling Score: 9 / 10

**Strengths:** No cycles, strict layering, domain has zero internal deps.
**Weakness:** `agenticos-daemon` depends on all 7 internal crates — any interface change cascades to the daemon.

---

## Cohesion

### Within-Crate Cohesion

| Crate | Cohesion | Assessment |
|-------|----------|------------|
| `agenticos-domain` | High | All types relate to the OS policy plane domain; no unrelated concepts |
| `agenticos-application` | High | All types are port/interface definitions |
| `agenticos-bus` | High | Event bus, trace store, envelope — all messaging concerns |
| `agenticos-policy` | High | Policy evaluation, capability, budget, invariant — all policy concerns |
| `agenticos-runtime` | High | Agent lifecycle, registration — all agent management |
| `agenticos-agents` | Medium | Stubs for 7 agent types but only 3 are implemented (memory, dummy-a, dummy-b) |
| `agenticos-observe` | High | Observation collection, procfs parsing, sampling — all observation concerns |
| `agenticos-executor` | High | Action execution, rollback — all execution concerns |
| `agenticos-daemon` | Medium | Bootstrap, config, service loop — multiple responsibilities but acceptable for entry point |
| `agenticos-cli` | High | Single-purpose scaffold |
| `agenticos-dashboard` | High | Single-purpose scaffold |

### Cohesion Score: 8 / 10

**Strengths:** Every crate has a single, well-defined responsibility. No crate mixes concerns from different layers.
**Weakness:** `agenticos-agents` contains stubs for 7 agent types but only 3 have non-trivial `propose()` implementations. The stubs reduce module cohesion (present but non-functional).

---

## Test Coverage

### By Crate

| Crate | Tests | Coverage Estimate | Assessment |
|-------|-------|-------------------|------------|
| `agenticos-domain` | 0 | ~10% (types are data-only) | **Acceptable** — types are simple structs with serde derives; logic is minimal |
| `agenticos-application` | 0 | ~10% (trait definitions) | **Acceptable** — trait definitions have no logic to test |
| `agenticos-bus` | 4 | ~60% | **Good** — bus + store round-trips tested |
| `agenticos-policy` | 5 | ~85% | **Excellent** — all policy presets and edge cases covered |
| `agenticos-runtime` | 3 | ~70% | **Good** — lifecycle + registration + collect_proposals |
| `agenticos-agents` | 5 | ~70% | **Good** — memory rule logic + pipeline, dummies untested at unit level |
| `agenticos-observe` | 9 | ~80% | **Excellent** — all procfs parsing functions tested, sampler untested |
| `agenticos-executor` | 5 | ~60% | **Good** — dry run, cgroup path, snapshot, rollback tested; NoopExecutor untested |
| `agenticos-daemon` | 5 | ~50% | **Acceptable** — multi-agent integration tests, config loading untested |
| `agenticos-cli` | 0 | 0% | **Needs work** — scaffold |
| `agenticos-dashboard` | 0 | 0% | **Needs work** — scaffold |

### Coverage by Category

| Category | Tests | Assessment |
|----------|-------|------------|
| Unit tests | 30 | Good coverage of parsing, policy, runtime, agent logic |
| Integration tests | 2 (pipeline + multi-agent) | Minimal — only synchronous in-process tests, no tokio-based |
| Linux-specific tests | 4 (cfg-gated) | Cannot run on CI |
| Property-based tests | 0 | Not present |
| Fuzz tests | 0 | Not present |
| Performance benchmarks | 0 | Not present |

### Test Coverage Score: 7 / 10

**Strengths:** Core logic (policy, parsing, agent rules) is well-tested with 31 passing tests. No flaky tests.
**Weaknesses:** No tokio-based integration tests. No Linux CI. No benchmarks. Config parsing untested.

---

## Replayability

### Current Capability

| Feature | Status | Details |
|---------|--------|---------|
| Event persistence | ✅ | `SqliteTraceStore` persists all events to SQLite |
| Event ordering | ✅ | Auto-increment `id` column preserves insertion order |
| Trace-scoped replay | ✅ | `replay(trace_id)` returns events in causal order |
| Cross-trace ordering | ❌ | No global ordering across traces |
| Deterministic replay | ❌ | Replay returns events but cannot re-execute the pipeline |
| Replay verification | ❌ | No mechanism to compare replay output with original execution |

### Replayability Mechanisms

The trace store schema supports replay:

```sql
CREATE TABLE traces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    causation_id TEXT,
    topic TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    payload_json TEXT NOT NULL
);

CREATE INDEX idx_traces_trace_id ON traces(trace_id);
```

A full tick produces 6+ events for multi-agent scenarios:

```
trace_id = "daemon-main"

id=1: observations.memory     (1 per observation)
id=2: proposals.dummy-a       (1 per proposal)
id=3: proposals.dummy-b       (1 per proposal)
id=4: decisions.dummy-a       (1 per decision)
id=5: decisions.dummy-b       (1 per decision)
id=6: results.dummy-a         (1 per approved action)
id=7: results.dummy-b         (1 per approved action)
id=8: metrics.daemon          (1 per tick)
```

### Replayability Score: 6 / 10

**Strengths:** Events are persisted with causal ordering and trace-scoped queries. SQLite provides ACID guarantees.
**Weaknesses:** No re-execution capability. No replay-vs-live comparison. No cross-trace correlation. No snapshot-based replay.

---

## Safety

### Safety Mechanisms

| Mechanism | Status | Details |
|-----------|--------|---------|
| Executor authority boundary | ✅ | Only `ApprovedActionExecutor` can mutate OS state |
| Default DryRun executor | ✅ | All platforms return `ActionStatus::DryRun` by default |
| cfg-gated Linux executor | ✅ | `LinuxCgroupExecutor` only compiles on `target_os = "linux"` |
| Rollback for cgroup mutations | ✅ | `CgroupRollbackManager` with JSON snapshots |
| No-op fallback for non-Linux | ✅ | `NoopExecutor` and `NoopRollbackManager` |
| Deterministic policy evaluation | ✅ | Same proposal + config → same decision |
| Agent isolation | ✅ | Agents cannot access OS state directly |
| Event tracing | ✅ | Every event is persisted to trace store |

### Safety Gaps

| Gap | Severity | Details |
|-----|----------|---------|
| No privilege dropping | High | Daemon runs as root; ADR-0003 model is not implemented |
| No agent panic isolation | Medium | Agent panic crashes the entire daemon tick loop |
| No proposal validation | Medium | Proposals are not validated beyond confidence range |
| No budget enforcement | Medium | `AgentBudget` exists but is not wired |
| No rate limiting | Medium | Agents can propose arbitrarily many actions per tick |
| No cgroup hierarchy bootstrap | Low | `/sys/fs/cgroup/agenticos/` must be created manually |
| No signal handling | Low | No graceful shutdown on SIGTERM/SIGINT |
| No watchdog | Low | No mechanism to detect a hung daemon |

### Safety Score: 6 / 10

**Strengths:** Strong architectural safety through the Executor authority boundary and ADR-driven design decisions. The cfg-gating and DryRun defaults prevent accidental OS mutations on non-Linux platforms.
**Weaknesses:** Critical runtime safety mechanisms (privilege dropping, panic isolation, rate limiting) are not yet implemented.

---

## Extensibility

### Extension Points

| Extension Point | Mechanism | Assessment |
|-----------------|-----------|------------|
| New agent types | Implement `Agent` trait | ✅ **First-class** — add a struct, implement `propose()`, register |
| New action kinds | Add variant to `ActionKind` enum | ✅ **First-class** — add variant, handle in policy + executor |
| New policy kernels | Implement `DeterministicPolicyKernel` trait | ✅ **First-class** — trait is well-defined |
| New executors | Implement `ApprovedActionExecutor` trait | ✅ **First-class** — trait is well-defined |
| New observers | Implement `ObserverPort` trait | ✅ **First-class** — trait is well-defined |
| New trace stores | Implement `TraceStore` trait | ✅ **First-class** — trait is well-defined |
| New event buses | Implement `EventBus` trait | ✅ **First-class** — trait is well-defined |
| New observation collectors | Implement `ProcessCollector`, `MemoryCollector`, `CgroupCollector` | ✅ **First-class** — traits are well-defined |
| New metric exporters | Implement `MetricExporterPort` trait | ✅ **First-class** — trait exists but is unused |
| New config sources | Implement `DaemonConfig` serde | ⚠️ **Works** — TOML is hardcoded; other formats require new deserialization |
| Agent priority/scheduling | Modify `AgentRuntime` trait | ❌ **Not supported** — no priority concept |
| Policy config at runtime | N/A | ❌ **Not supported** — config is loaded once at startup |

### Extensibility Score: 8 / 10

**Strengths:** Trait-based architecture provides clean extension points for every major component. Adding a new agent, collector, executor, policy kernel, or trace store requires no changes to existing implementations — only implementing the trait and registering it.

**Weaknesses:** No runtime policy reload. No agent priority/scheduling. The config format is fixed to TOML.

---

## Overall Health Summary

| Dimension | Score | Trend |
|-----------|-------|-------|
| **Coupling** | 9 / 10 | ✅ Stable — clean DAG, no cycles |
| **Cohesion** | 8 / 10 | ✅ Stable — single-responsibility crates |
| **Test Coverage** | 7 / 10 | 🟡 Improving — 31 tests, but gaps in integration + Linux |
| **Replayability** | 6 / 10 | 🟡 Needs work — persistence is solid, re-execution is missing |
| **Safety** | 6 / 10 | 🟡 Needs work — architecture is sound, runtime hardening is missing |
| **Extensibility** | 8 / 10 | ✅ Strong — trait-based extension points |

### Overall: 7.3 / 10

**Alpha-1 achieves a solid foundation with clean architecture, strong extensibility, and well-defined extension points. The primary risks are in runtime safety (privilege dropping, panic isolation) and testing breadth (no Linux CI, no tokio integration tests).**

---

## Recommendations

### Priority 1 (Safety)
1. Implement privilege dropping (ADR-0003) — start as root, create cgroup hierarchy, delegate to unprivileged user.
2. Wrap `collect_proposals()` in `std::panic::catch_unwind` to isolate agent panics.
3. Add SIGTERM/SIGINT signal handling for graceful shutdown.

### Priority 2 (Testing)
4. Add tokio-based integration test that actually runs the service loop for 3 ticks.
5. Set up Linux CI runner for cfg-gated tests (cgroup executor, procfs collectors).
6. Add property-based tests for policy evaluation (all combos of confidence × action kind × safety level).

### Priority 3 (Replayability)
7. Add full-tick deterministic replay: record observations → replay through the same pipeline → compare action results.
8. Add cross-trace correlation via causation_id chains.

### Priority 4 (Governance)
9. Wire `AgentBudget`, `CapabilityGrant`, and `PolicyInvariant` into `DefaultPolicyKernel`.
10. Add runtime policy config reload.
