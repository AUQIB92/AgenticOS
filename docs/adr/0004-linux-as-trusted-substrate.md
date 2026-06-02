# ADR-0004: Use Linux As The Trusted Substrate

## Status

Accepted

## Context

The research objective is agent-governed OS policy, not a new kernel. Ubuntu Linux 24.04 provides mature scheduling, memory management, cgroups, namespaces, LSM, and observability mechanisms.

## Decision

AgenticOS runs above Ubuntu Linux. Linux executes mechanisms. AgenticOS governs policy through deterministic mediation.

## Consequences

The prototype is feasible and reproducible. It cannot claim to be a standalone bootable OS in early phases.
