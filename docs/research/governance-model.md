# Governance Model

**Date:** 2026-06-02  
**Status:** Draft  
**Supersedes:** ADR-0005, ADR-0006, ADR-0008 (formalises their principles into a single governance specification)

---

## 1. Agent Types

Every agent has an `AgentKind` and a `CapabilityScope` that together define its role and reach.

### Observation Agents

Produce observations only. They never emit proposals.

| Agent | `AgentKind` | Observations | Proposal Authority |
|-------|-------------|--------------|--------------------|
| System Sampler | (infrastructure) | Process, Memory, Cpu, Cgroup | None |
| Future: Network Observer | `Network` | Connection tables, interface stats | None |
| Future: Device Observer | `Device` | Device events, `udev` changes | None |

**Capability scope:** `ReadOnly`

### Resource Agents

Observe resource pressure and propose cgroup adjustments.

| Agent | `AgentKind` | Rules | Proposals |
|-------|-------------|-------|-----------|
| MemoryAgent | `Memory` | Memory usage >80% → propose `CgroupSetMemoryMax` | `SetMemoryMax` |
| ProcessAgent | `Process` | CPU usage >80% → propose `CgroupSetCpuMax`; throttling → `CgroupSetCpuWeight`; pressure >50% → `WorkloadClassifyRecommend` | `SetCpuMax`, `SetCpuWeight`, `WorkloadClassifyRecommend` |
| Future: I/O Agent | `Custom("io")` | I/O pressure > threshold → propose I/O throttling | (action kinds TBD) |

**Capability scope:** `ProposalOnly`

### Security Agents

Enforce security invariants and respond to threat signals.

| Agent | `AgentKind` | Rules | Proposals |
|-------|-------------|-------|-----------|
| Future: SecurityAgent | `Security` | Anomalous process behaviour, cgroup escape attempts, unexpected `ptrace` | Incidents (`Security`, `GovernanceViolation`) via event bus; `WorkloadClassifyRecommend` proposals |
| Future: PolicyAgent | `Supervisor` | Runtime policy violations, capability misuse | Incidents (`PolicyViolation`, `GovernanceViolation`); `ObserveOnly` proposals |

**Capability scope:** `ProposalOnly`

### Supervisory Agents

Oversee the agent ecosystem itself. They observe agent health, detect coordination failures, and escalate.

| Agent | `AgentKind` | Rules | Proposals |
|-------|-------------|-------|-----------|
| Future: SupervisorAgent | `Supervisor` | Agent crash detection, proposal flood suppression, deadlock recovery | `ObserveOnly` (escalation via incident) |
| Future: BudgetAgent | `Supervisor` | Per-agent proposal budgets, tick-time budgets, memory budgets | `ObserveOnly` (enforcement is in the Policy Kernel) |

**Capability scope:** `ProposalOnly` (but supervisory proposals are typically recommendations to the human operator, not OS mutations)

---

## 2. Authority Levels

Four discrete authority levels. Each is a strict superset of the previous.

| Level | Label | What the component may do | Examples |
|-------|-------|---------------------------|----------|
| 0 | **Observe** | Read OS state via the observation layer | `ProcfsMemoryCollector`, `ProcfsCpuCollector`, `CgroupFsCollector` |
| 1 | **Propose** | Emit a `Proposal` containing a requested action and rationale | `MemoryAgent::propose()`, `ProcessAgent::propose()` |
| 2 | **Decide** | Evaluate a `Proposal` and produce a binding `Decision` | `DefaultPolicyKernel::evaluate()` |
| 3 | **Execute** | Mutate OS state via approved actions | `LinuxCgroupExecutor::execute()`, `DryRunExecutor::execute()` |

### Authority assignment

| Component | Level(s) | Rationale |
|-----------|----------|-----------|
| Observation layer | 0 | Stateless collectors; no opinion, no mutation |
| Agents | 0, 1 | May observe and propose; may never decide or execute |
| Policy Kernel | 2 | Deterministic decision-maker; no OS access |
| Executor | 3 | Sole mutation authority; no opinion on proposals |
| Daemon service loop | Orchestrates 0→1→2→3 | Glue; no authority of its own |

---

## 3. Allowed Transitions

The governance model enforces a linear pipeline for resource-management decisions. Incidents follow a separate, shorter path. Every state transition is explicit and traced.

```
── Resource-management path ──     ── Incident path ──

Observation                         Observation
    │                                    │
    ▼                                    ▼
Proposal                            Incident
    │
    ▼
Decision
    │
    ▼
Action
    │
    ▼
Result
```

### 3.1 Observation → Proposal

**Condition:** An agent has received observations and its `propose()` method returns one or more `Proposal` values.

**Invariants:**
- Every `Proposal` references at least one `ObservationId` in its `based_on` field.
- The agent must be registered in the `InMemoryAgentRuntime` to participate in the tick.
- The agent's `CapabilityScope` must be `ProposalOnly` or higher.

**Traced:** Yes. Proposals are published to `proposals.<agent_id>` and appended to the trace store.

### 3.2 Proposal → Decision

**Condition:** The `PolicyKernel::evaluate()` method is called with a single `Proposal` and returns a `Decision`.

**Invariants:**
- Every `Decision` references exactly one `ProposalId`.
- `DecisionOutcome` is either `Approved` or `Denied`; no indeterminate state.
- Deciding is synchronous and deterministic given the same proposal and policy config.

**Traced:** Yes. Decisions are published to `decisions.<agent_id>` and appended to the trace store.

### 3.3 Decision → Action

**Condition:** A `Decision` with `outcome: Approved` is converted into an `ApprovedAction` via `PolicyKernel::validate_action()`.

**Invariants:**
- An `ApprovedAction` wraps the original `ActionRequest` plus the `DecisionId`.
- Denied decisions do not produce actions. They are final and recorded.

**Traced:** Yes. The `ApprovedAction` is passed to the Executor.

### 3.4 Action → Result

**Condition:** The `Executor::execute()` method is called with an `ApprovedAction` and returns an `ActionResult`.

**Invariants:**
- Every `ActionResult` references exactly one `ActionId`.
- `ActionStatus` is one of `Succeeded`, `Failed`, `DryRun`, `Denied`.
- If the action mutated OS state and the Executor supports rollback, a `RollbackToken` is included.
- The Executor is the **only** component that calls Linux APIs.

**Traced:** Yes. Results are published to `results.<agent_id>` and appended to the trace store.

### 3.5 Observation → Incident

**Condition:** An agent determines that observations indicate a security concern, resource contention, governance violation, policy violation, or agent/executor failure. The agent constructs an `Incident` and publishes it via the event bus.

**Invariants:**
- Every `Incident` has a unique `incident_id: IncidentId` and a `category: IncidentCategory`.
- Every `Incident` references exactly one `source_agent: AgentId` and may reference an optional `source_observation: ObservationId`.
- Incidents are **not** proposals. They do not enter the policy-evaluation pipeline and are never converted into `ApprovedAction`s.
- Incidents carry a `severity: IncidentSeverity` (Info, Warning, Error, Critical) for operator triage.
- Optional `correlation_id: Option<String>` groups related incidents (e.g., multiple agents reporting the same resource contention).
- Incidents may be emitted by any agent at any time. They do not require Policy Kernel approval.

**Traced:** Yes. Incidents are published to `incidents.<category>` and appended to the trace store. The full `Incident` struct (including category, severity, source, correlation_id, and description) is persisted via the existing `EventPayload::Incident` → `TraceStore` pipeline.

**Contrast with proposals:**
| Dimension | Proposal | Incident |
|-----------|----------|----------|
| Requires policy approval | Yes | No |
| Leads to OS mutation | Possibly | Never |
| Has `ActionKind` | Yes | No |
| Has `IncidentCategory` | No | Yes |
| Published to | `proposals.<agent_id>` | `incidents.<category>` |

---

## 4. Forbidden Transitions

The following transitions are architecturally impossible. If any is observed, it is a security incident.

### 4.1 Observation → Action

An agent must never convert an observation directly into an OS mutation.

**Enforcement:**
- Agents do not hold file descriptors to cgroupfs, procfs, or any privileged device.
- The `Agent` trait has no method that accepts an `ApprovedAction` or produces an `ActionResult`.
- The daemon service loop never calls `Executor::execute()` with a `Proposal`; only with an `ApprovedAction` that has passed through the Policy Kernel.

**Violation response:** The agent is deregistered and the incident is traced to `system.error`.

### 4.2 Proposal → Action

A proposal must never be forwarded to the Executor without a Decision.

**Enforcement:**
- The daemon service loop explicitly calls `kernel.evaluate()` → `kernel.validate_action()` → `executor.execute()` in that order, on every tick.
- The type system enforces this: `evaluate()` returns `Decision`, `validate_action()` returns `Option<ApprovedAction>`, `execute()` accepts `ApprovedAction`. There is no `execute(Proposal)` overload.

### 4.3 Agent → Linux Mutation

No agent — regardless of its `AgentKind`, `CapabilityScope`, or registration order — may call Linux system calls, write to `/sys/fs/cgroup/`, or otherwise mutate OS state.

**Enforcement:**
- See ADR-0006: Executor Authority Boundary.
- Agents are Rust trait objects (`Box<dyn Agent>`). They receive `&[Observation]` and return `Vec<Proposal>`. They have no access to the executor, the policy kernel, or any `pub` function that performs mutations.
- The daemon process drops all Linux capabilities after initialising the cgroup hierarchy. Even if an agent finds a way to execute code, it cannot write to cgroupfs unless the Executor has the necessary capabilities.

---

## 5. Conflict Resolution

Multiple agents may produce conflicting proposals from the same observations. The governance model defines explicit arbitration rules.

### 5.1 Memory Agent vs Process Agent

**Conflict scenario:** Both agents observe high resource usage. MemoryAgent proposes raising `memory.max` for a cgroup. ProcessAgent proposes setting `cpu.max` for the same cgroup.

| Dimension | MemoryAgent | ProcessAgent |
|-----------|-------------|--------------|
| Trigger | Memory usage >80% | CPU usage >80% or throttling |
| Action target | `memory.max` | `cpu.max`, `cpu.weight` |
| Resource | Memory | CPU |
| Conflict type | **No direct conflict** — different cgroup files | |

**Resolution:** Both proposals are approved and executed in registration order. They affect independent cgroup files and do not interfere.

**Exception:** If both agents target the same cgroup **and** the combined memory+cpu limits would overcommit the parent cgroup's resources, the Policy Kernel denies the later proposal. This requires future work: a resource budget tracker in the Policy Kernel.

### 5.2 Process Agent vs Security Agent

**Conflict scenario:** ProcessAgent proposes a CPU weight increase for a cgroup to handle a workload spike. SecurityAgent proposes freezing the same cgroup because it detected anomalous behaviour.

| Dimension | ProcessAgent | SecurityAgent |
|-----------|-------------|---------------|
| Trigger | Throttling detected | Anomalous process behaviour |
| Action target | `cpu.weight` | `cgroup.freeze` |
| Resource | CPU | Process state |
| Conflict type | **Mutually exclusive** — cannot both boost and freeze | |

**Resolution:** SecurityAgent wins. When a security agent proposes `ProcessFreezeGroup` for a group that another resource agent has proposed CPU/memory adjustments for, the Policy Kernel:
1. Approves the freeze (higher safety priority).
2. Denies the CPU/memory proposal with rationale `"overridden by security — cgroup <name> frozen"`.

**Rationale:** Security invariants take precedence over performance optimisation. A frozen process cannot benefit from CPU weight, and allowing CPU adjustments on a frozen group could mask the security incident.

**Implementation mechanism:** The Policy Kernel tracks tentative state changes within each tick. If a later proposal freezes a cgroup, any earlier approved action targeting the same cgroup's resource limits is marked `Denied` and the Denied decision replaces the earlier Approved decision in the trace.

### 5.3 Future Arbitration Rules

These rules are not yet implemented. They are specified here as requirements for the next iteration.

#### 5.3.1 Same-Resource Contention

Two agents propose adjustments to the same cgroup file in the same tick.

| Example | Agent A | Agent B |
|---------|---------|---------|
| `memory.max` | Proposes 4 GB | Proposes 2 GB |

**Rule:** The Policy Kernel evaluates proposals in registration order. The first proposal to set a given (cgroup, file) pair is approved; subsequent proposals targeting the same (cgroup, file) are denied with the explanation `"superseded by earlier proposal from <agent_id>"`.

**Exception:** If the second proposal is from a `Supervisor`-kind agent or has higher confidence, the Policy Kernel may override. This override logic is gated by a config flag `allow_supervisor_override: bool`.

#### 5.3.2 Cascade Prevention

An approved action that triggers a secondary condition (e.g., a memory limit decrease that causes OOM kills, leading to a process agent proposing CPU adjustments for the surviving process) is handled in the **next tick**. This prevents cascade loops within a single tick.

#### 5.3.3 Stall Recovery

If an agent fails to produce proposals within the tick budget (default 100 ms), its proposals are skipped for that tick. The skip is traced as `"agent <id> timed out"`. After 3 consecutive timeouts, the agent is deregistered.

#### 5.3.4 Priority Inversion

A low-priority agent must not block a high-priority agent's proposal. The Policy Kernel assigns `proposal.priority` during evaluation. Higher-priority proposals are evaluated first regardless of registration order. Priority levels:

| Priority | Used by |
|----------|---------|
| `Critical` | SecurityAgent freeze/terminate proposals |
| `High` | Supervisor agent proposals |
| `Normal` | Resource agent proposals |
| `Low` | Benchmark/workload classification proposals |

---

## 6. Trust Model Summary

```
                     ┌─────────────┐
                     │   Daemon    │  — orchestrates pipeline
                     │   Service   │     trusts nobody
                     └──────┬──────┘
                            │
              ┌─────────────┼─────────────┐
              │             │              │
              ▼             ▼              ▼
     ┌────────────┐ ┌────────────┐ ┌────────────┐
     │ Observer   │ │   Agent    │ │  Policy    │
     │ (Level 0)  │ │ (Level 1)  │ │  Kernel    │
     │ untrusted  │ │ untrusted  │ │ (Level 2)  │
     └────────────┘ └────────────┘ └──────┬─────┘
                                          │
                                          ▼
                                  ┌────────────┐
                                  │  Executor  │
                                  │ (Level 3)  │
                                  │  trusted   │
                                  └──────┬─────┘
                                         │
                                         ▼
                                  ┌────────────┐
                                  │ Linux APIs │
                                  │ (cgroupfs, │
                                  │  procfs,   │
                                  │  signals)  │
                                  └────────────┘
```

- **Observers** are untrusted data collectors. If compromised, they can inject false observations but cannot mutate state.
- **Agents** are untrusted. If compromised, they can emit malicious proposals **or spurious incidents** but cannot bypass the Policy Kernel or the Executor. Incidents are advisory — they do not trigger OS mutations.
- **Policy Kernel** is semi-trusted. It has no OS access but determines which proposals become actions. It must be deterministic and auditable.
- **Executor** is fully trusted. It is the only component with OS mutation authority. It must be hardened, audited, and crash-safe.

---

## 7. Formal Specification (Future Work)

The governance model described in this document is intended to be formalised as:

1. **A configuration schema** — the allowed transitions, authority levels, and conflict rules expressed as a `GovernanceConfig` struct (TOML-deserialisable), loaded at daemon startup.
2. **A proof-checking module** — a `GovernanceProof` that verifies every event in the trace against the governance rules, runnable as an offline audit tool.
3. **Property-based tests** — the governance invariants (no forbidden transitions, deterministic ordering, complete tracing) expressed as `proptest` or `bolero` properties.

These are deferred until the agent ecosystem reaches sufficient complexity to justify the formalisation overhead.

---

**References:**
- ADR-0005: Agents Never Hold Privileged Authority
- ADR-0006: Executor Authority Boundary
- ADR-0008: Multi-Agent Coordination
- ADR-0009: Security Agent Authority
- `crates/agenticos-domain/src/agent.rs` — `Agent` trait, `AgentKind`, `CapabilityScope`
- `crates/agenticos-domain/src/action.rs` — `ActionKind`, `ActionSafetyLevel`
- `crates/agenticos-domain/src/event.rs` — `Incident`, `IncidentCategory`, `IncidentSeverity`
- `crates/agenticos-policy/src/policy_kernel.rs` — `DefaultPolicyKernel`
- `crates/agenticos-executor/src/linux.rs` — `LinuxCgroupExecutor`
- `docs/architecture/current-architecture.md` — Component architecture diagrams
