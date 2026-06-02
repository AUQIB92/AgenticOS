# ADR-0003: Use An Event-Driven Agent Runtime

## Status

Accepted

## Context

AgenticOS needs replayable, auditable coordination between observers, agents, policy, executors, and dashboards.

## Decision

All agent interaction flows through typed events: observations, proposals, decisions, action requests, action results, incidents, and traces.

## Consequences

Event flow improves auditability and replay. It also requires strict schemas and careful backpressure design in later implementation phases.
