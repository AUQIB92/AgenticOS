# AgenticOS Current State

**Date:** 2026-06-02  
**Branch:** main  
**Phase:** 0 — Research scaffold and core data model

## What Exists

### Crate Inventory

| Crate | Status | Contents |
|-------|--------|----------|
| `agenticos-domain` | Stable | Core types: IDs, Agent, Observation, Proposal, Decision, Action, Event, Error. Serialization via serde. |
| `agenticos-application` | Stable | Port traits: EventBus, PolicyKernelPort, ActionExecutorPort, ObserverPort, MetricExporterPort. Use-case commands. |
| `agenticos-bus` | Stable | In memory EventBus and TraceStore. Topic matching with wildcards. SqliteTraceStore pending. |
| `agenticos-policy` | Stable | DefaultPolicyKernel with configurable action allowlists, safety levels, confidence thresholds. Tested. |
| `agenticos-runtime` | Stable | InMemoryAgentRuntime with lifecycle state machine (Registered → Idle → Terminated). Tested. |
| `agenticos-agents` | Skeleton | Agent trait implementations (Supervisor, Memory, Process, Security, File, Device) — no behavior yet. |
| `agenticos-observe` | Skeleton | Placeholder structs — no Linux observation logic yet. |
| `agenticos-executor` | Skeleton | ApprovedActionExecutor trait, RollbackManager trait, empty LinuxExecutor. |
| `agenticos-daemon` | Skeleton | Main scaffold, BootstrapPlan, DaemonConfig — not wired. |
| `agenticos-cli` | Skeleton | CLI scaffold with Command enum — no clap wiring. |
| `agenticos-dashboard` | Skeleton | DashboardApi struct, DashboardStatus model — not wired. |

### Tests

- **agenticos-domain:** None yet (value types only).
- **agenticos-bus:** Topic matching tests, trace store replay tests.
- **agenticos-policy:** Policy kernel evaluation tests (approve/deny by safety level, capability, confidence).
- **agenticos-runtime:** Agent lifecycle transition tests, duplicate registration rejection.

### Config & Policy

- `configs/dev.toml` — development mode, memory event store.
- `configs/safe-local.toml` — safe-local mode, privileged execution disabled.
- `policies/default.toml` — scaffold policy, no privileged actions enabled.

### Documentation

- `docs/architecture.md` — brief clean-architecture summary.
- `docs/threat-model.md` — security assumptions and out-of-scope items.
- `docs/adr/` — Architecture Decision Records (3 ADRs).
- `docs/research-hypothesis.md` — thesis, claims, predictions.
- `docs/current-state.md` — this file.

## What's Missing (Next Steps)

1. **SQLite TraceStore** — durable append-only event store for persistence and replay.
2. **Linux observation** — procfs, meminfo, cgroup stats readers.
3. **Linux cgroup executor** — create/set/move/freeze/thaw via cgroupfs.
4. **Daemon wiring** — tokio event loop connecting observer → bus → agents → policy → executor.
5. **Rule-based agents** — Memory, Process, Security agents with actual proposal logic.
6. **Safety Governor** — veto agent.
7. **CLI** — clap subcommands for inspection and control.
8. **Integration tests** — end-to-end pipeline verification.

## Build Status

- Toolchain: `rustc 1.96.0-nightly`
- Build: `cargo build` — compiles with warnings on skeleton code.
- Test: `cargo test` — all existing tests pass.
