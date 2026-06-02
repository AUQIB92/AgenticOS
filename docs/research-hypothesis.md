# AgenticOS Research Hypothesis

## Thesis Statement

Operating-system services can be partially governed by autonomous, explainable policy agents if all authority is mediated by a deterministic trust kernel, all actions are bounded and replayable, and the system degrades safely to conventional Linux behavior.

## Core Claim

The publishable contribution is not "an AI OS." It is:

> A deterministic mediation architecture that allows autonomous agents to participate in OS service policy without granting them direct authority over kernel mechanisms.

## Sub-Hypotheses

1. **Policy-plane separation:** Separating OS policy (agents) from mechanism (Linux) is feasible without kernel modifications and with acceptable overhead.

2. **Deterministic mediation:** A lightweight Rust policy kernel can validate, bound, audit, and replay agent recommendations at sub-millisecond latency.

3. **Rule-based competence:** Simple rule-based agents can produce non-trivial resource-management recommendations (memory limits under pressure, CPU quota adjustments, process group freezing) that are comparable to or better than Linux default behavior on specific workloads.

4. **Safety through architecture:** An explicit veto layer (Safety Governor) and immutable audit log can prevent unsafe actions even when multiple agents produce conflicting or malicious proposals.

5. **LLM optionality:** Local LLM agents can improve explanation quality and handle edge cases, but the system remains functional and safe with rule agents alone.

## Falsifiable Predictions

- AgenticOS policy decisions add less than 5ms overhead per decision on modern hardware.
- The Safety Governor denies at least 90% of deliberately injected unsafe proposals during fault-injection testing.
- Replay of any experiment trace produces identical decisions (deterministic modulo timeouts).
- Rule-based agents measurably reduce OOM events under memory pressure compared to Linux defaults in at least one benchmark workload.

## Non-Goals

- Replacing the Linux scheduler, memory manager, or any kernel mechanism.
- Achieving lower latency than bare-metal Linux for hot-path operations.
- Running as a bootable OS distribution.
- Distributed multi-machine AgenticOS.
- Cloud LLM dependency.
