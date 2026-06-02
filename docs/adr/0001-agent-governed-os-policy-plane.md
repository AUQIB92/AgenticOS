# ADR 0001: Agent-Governed OS Policy Plane Over Linux

**Status:** Accepted  
**Date:** 2026-06-02  
**Deciders:** Research team

## Context

AgenticOS explores whether OS service policy can be governed by autonomous agents while all privileged execution remains mediated by deterministic system code and a trusted Linux substrate.

The core question is not "can an LLM be a kernel" but "can autonomous agents participate in OS service policy decisions safely, explainably, and measurably."

## Decision

We adopt a strict three-plane architecture:

1. **Mechanism plane:** Linux kernel, systemd, cgroup v2, namespaces, seccomp, AppArmor, eBPF (observability only). Linux is the trusted executor.

2. **Policy plane:** Agents (rule-based and optional LLM), policy rules, workload models. Agents recommend. They never execute.

3. **Trust plane:** Deterministic Rust Policy Kernel. Validates every proposal against policy invariants, budgets, and capability grants. Approves or denies. All decisions are logged and replayable.

## Consequences

Positive:
- Agents can be unprivileged, fallible, and replaceable without compromising system integrity.
- The Policy Kernel is the only component that can approve actions.
- Linux remains unmodified. No kernel patches needed.
- Benchmark comparisons against plain Linux are straightforward.

Negative:
- Agents cannot directly optimize kernel hot paths (scheduling, page replacement, interrupt handling).
- LLM agent latency makes them unsuitable for sub-millisecond policy decisions.
- The architecture adds a mediation layer that introduces overhead.

## Related

- ADR 0002: Clean Architecture Dependency Direction
