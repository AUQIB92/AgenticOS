# ADR-0005: Agents Never Hold Privileged Authority

## Status

Accepted

## Context

Agents may be nondeterministic, slow, incorrect, or compromised. LLM-backed agents are especially unsuitable as trusted execution authorities.

## Decision

Agents can observe, reason, and propose. Only the deterministic Policy Kernel can approve actions, and only the executor can perform approved actions.

## Consequences

The architecture preserves a clear trust boundary. Some agent proposals may be denied even when useful, but safety and reproducibility take priority.
