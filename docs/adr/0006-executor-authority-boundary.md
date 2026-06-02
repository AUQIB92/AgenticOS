# ADR 0006: Executor Authority Boundary

**Status:** Accepted  
**Date:** 2026-06-02  
**Deciders:** Research team

## Context

If agents can write cgroup files, freeze processes, move tasks, or modify limits directly, then the Policy Kernel's mediation is bypassed, the audit trail is incomplete, and rollback becomes impossible. Every component that can mutate OS state is a potential vulnerability.

The architecture must draw a clear line: one and only one component is permitted to execute privileged actions.

## Decision

The Executor is the only component permitted to mutate operating-system state.

**Agents cannot:**
- write cgroup files (`memory.max`, `cpu.max`, `cgroup.procs`)
- freeze or thaw process groups
- move tasks between cgroups
- modify resource limits
- interact with kernel APIs directly
- execute any privileged system call

**Agents may only emit Proposals.** A Proposal is a typed message containing a requested action, rationale, and confidence. It carries no authority.

**The Policy Kernel** converts Proposals into Decisions by evaluating them against safety invariants, capability grants, budgets, and policy configuration.

**The Executor** converts approved Decisions into Actions by calling the appropriate Linux APIs (cgroupfs writes, process signals, etc.). It is the single component trusted with write access to OS state.

The data flow is:

```
Agent → Proposal → Policy Kernel → Decision → Executor → ActionResult
                                                   ↓
                                              Linux APIs
                                           (cgroupfs, procfs,
                                            signals, …)
```

No side channel exists. Agents cannot write files, open privileged devices, or send signals outside this flow.

## Consequences

Positive:
- Audit trail is complete: every mutation has a corresponding Decision and Approval.
- Rollback is possible because the Executor can track and revert its own mutations.
- Agents can be implemented, crashed, replaced, or compromised without risking OS integrity.

Negative:
- Latency: every mutation requires a round trip through the bus (Proposal → Decision → Action).
- The Executor must be trusted and hardened, as it is the single point of failure for OS mutations.
- Some reversible mutations (e.g., cgroup limits) require explicit rollback token support in the Executor.
