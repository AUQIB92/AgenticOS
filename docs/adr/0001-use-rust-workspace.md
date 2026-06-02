# ADR-0001: Use A Rust Workspace

## Status

Accepted

## Context

AgenticOS requires strong typing, explicit ownership, predictable binaries, Linux systems integration, and a structure that can grow into multiple research components.

## Decision

Use a Cargo workspace with separate crates for domain, application ports, bus, policy, runtime, agents, observers, executors, daemon, CLI, and dashboard.

## Consequences

The workspace keeps boundaries explicit and allows independent testing. It adds some crate-management overhead, but that overhead is acceptable for a research OS prototype.
