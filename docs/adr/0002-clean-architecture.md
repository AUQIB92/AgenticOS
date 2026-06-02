# ADR 0002: Clean Architecture Dependency Direction

**Status:** Accepted  
**Date:** 2026-06-02  
**Deciders:** Research team

## Context

The Rust workspace must remain modular, testable, and intellectually tractable as a research artifact. Dependencies between crates must prevent infrastructure concerns from leaking into domain logic.

## Decision

We define a strict dependency graph:

```
domain
  ↑
application
  ↑
runtime / policy / bus
  ↑
observe / executor / agents / dashboard
  ↑
daemon / cli
```

Rules:
- `agenticos-domain` depends on no internal crate. It defines OS-policy concepts.
- `agenticos-application` defines ports (traits) and use-case interfaces.
- Infrastructure crates implement ports.
- Agents produce proposals only; they never call executors.
- Executors accept approved actions only; they never call agents.
- The dashboard reads status, traces, and decisions; it must not bypass policy.

## Consequences

Positive:
- The domain model can be reasoned about in isolation.
- Ports can be mocked for unit tests without Linux dependencies.
- Substituting infrastructure (e.g., InMemoryEventBus → Kafka) does not affect domain logic.

Negative:
- Some indirection is required (trait objects, dependency injection).
- Small changes sometimes touch multiple crates.
