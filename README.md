# AgenticOS

AgenticOS is a research prototype for an agent-oriented operating-system policy plane.

It is not an AI assistant, not an agent application framework, and not AIOS. AgenticOS explores whether operating-system service policy can be governed by autonomous agents while all privileged execution remains mediated by deterministic system code and a trusted Linux substrate.

Target platform: Ubuntu Linux 24.04.

Language: Rust.

## Authority Model

Agents may:

- observe system state through approved observation interfaces
- reason over observations
- recommend bounded proposals
- receive policy decisions
- request execution only through approved system APIs

Agents may not:

- directly manipulate hardware
- directly mutate kernel state
- bypass the Policy Kernel
- execute privileged actions without an approved action
- treat LLM output as authority

## Workspace Layout

```text
crates/
├── agenticos-domain       # core entities and event types
├── agenticos-application  # clean architecture ports and use-case interfaces
├── agenticos-bus          # event bus interfaces and envelopes
├── agenticos-policy       # deterministic policy-kernel interfaces
├── agenticos-runtime      # agent runtime and lifecycle interfaces
├── agenticos-agents       # supervisor and OS-service agent shells
├── agenticos-observe      # observation adapter shells
├── agenticos-executor     # approved-action executor shells
├── agenticos-daemon       # daemon entrypoint shell
├── agenticos-cli          # CLI entrypoint shell
└── agenticos-dashboard    # dashboard API shell
```

## Clean Architecture

Dependency direction:

```text
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

- `agenticos-domain` depends on no internal crates.
- `agenticos-application` defines ports and use-case interfaces.
- Infrastructure crates implement ports in later phases.
- Agents produce proposals only.
- Executors accept approved actions only.
- Dashboard reads status, traces, and decisions; it must not bypass policy.

## Current Status

This repository is a scaffold only. It contains Cargo workspace configuration, interface definitions, placeholder modules, configs, policies, and architectural records.

No business logic, privileged executor behavior, kernel integration, or LLM integration is implemented yet.

## Planned Milestones

1. Core event model and replayable message bus
2. Deterministic Policy Kernel
3. Linux observation and approved-action executor
4. Agent runtime and rule-based service agents
5. Daemon, CLI, and dashboard control surface
6. Benchmark harness and Linux baselines
7. Local LLM integration with deterministic mediation
8. Kernel-adjacent observability and publication artifact
