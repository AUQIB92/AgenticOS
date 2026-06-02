# ADR 0009: Security Agent Authority

**Status:** Accepted  
**Date:** 2026-06-02  
**Deciders:** Research team

## Context

Security agents introduce a tension between safety and autonomy. The same capability that makes a security agent useful — the ability to freeze, terminate, or quarantine workloads — also makes it dangerous if the agent is incorrect, compromised, or acting on bad observations.

The governance model (ADR-0005, ADR-0006) forbids agents from mutating OS state directly. But even proposing security-motivated mutations (freeze, terminate, quarantine) creates risk: if the Policy Kernel approves such proposals by default, the system has effectively granted a security agent enforcement authority without the governance infrastructure to review, throttle, or roll back those decisions.

During Alpha, the priority is validating the governance architecture itself: the observation pipeline, the proposal lifecycle, deterministic policy arbitration, and the executor authority boundary. Granting enforcement-level authority to security agents before these foundations are proven introduces uncontained risk.

## Decision

Security Agents are advisory during Alpha.

**Security Agents may:**
- **Observe** — collect process, file, network, and security-relevant observations via the standard observation layer.
- **Propose** — emit proposals for security-relevant actions (e.g., `ProcessFreezeGroup`, `ProcessTerminateGroup`), subject to Policy Kernel evaluation.
- **Raise incidents** — emit `Incident` events on the event bus with severity `Info`, `Warning`, `Error`, or `Critical` (see `agenticos_domain::event::Incident`).
- **Generate recommendations** — emit proposals with `ActionKind::WorkloadClassifyRecommend` to flag workloads for human review.

**Security Agents may not:**
- **Terminate processes** — proposals for `ProcessTerminateGroup` are evaluated by the Policy Kernel and denied by default under Alpha safety config.
- **Freeze cgroups** — proposals for `ProcessFreezeGroup` follow the same deny-by-default path.
- **Quarantine workloads** — no quarantine action kind exists in the domain model. Workload classification is advisory only.
- **Modify permissions** — security agents have no capability to modify cgroup permissions, seccomp profiles, or other access control mechanisms.

These restrictions are enforced by:
1. **Default policy config:** Alpha presets (`safe-local`) deny all mutation actions. The `benchmark` preset allows resource-agent actions but not security-agent actions.
2. **ActionKind gating:** `DefaultPolicyKernel` is configured with an allowlist that excludes freeze, terminate, and quarantine actions for non-supervisor agents.
3. **CapabilityScope:** Security agents are registered with `CapabilityScope::ProposalOnly`. No agent has `ApprovedAction` capability during Alpha.

## Consequences

### Positive
- The governance architecture can be validated under low-stakes conditions before security enforcement is activated.
- Incidents and recommendations from security agents build operator trust without risking workload availability.
- The `WorkloadClassifyRecommend` action kind provides a safe channel for security agents to flag concerns without mutating OS state.
- Future Security Enforcement Agents inherit a proven governance pipeline.

### Negative
- Security threats cannot be automatically contained during Alpha. The system relies on human operators to act on security incidents and recommendations.
- The security agent's proposal logic cannot be validated end-to-end for freeze/terminate actions until enforcement is enabled.
- Policy Kernel must explicitly deny security-motivated mutations rather than simply not allowing them — this adds a small config burden.

## Future: Security Enforcement Agents

A future phase may introduce Security Enforcement Agents with authority to freeze, terminate, and quarantine workloads. This phase will require:

1. **Explicit Policy Kernel approval** — security enforcement actions must pass through a dedicated `SecurityPolicyKernel` that enforces invariants (e.g., "do not freeze the daemon's own cgroup", "do not terminate init", "quarantine requires two-agent consensus").
2. **Rollback guarantees** — every enforcement action must have a corresponding rollback (e.g., thaw after freeze, restore from quarantine).
3. **Audit escalation** — enforcement actions must emit `Incident` events at severity `Critical` and may require human acknowledgment before subsequent actions on the same target.
4. **Design review and ADR** — a separate ADR will define the Security Enforcement Agent's authority, invariants, and escalation protocol before any enforcement capability is implemented.

## References

- ADR-0005: Agents Never Hold Privileged Authority
- ADR-0006: Executor Authority Boundary
- ADR-0008: Multi-Agent Coordination
- `docs/research/governance-model.md` — Governance specification (§5.2 Security Agent conflict resolution)
- `crates/agenticos-domain/src/event.rs` — `Incident`, `IncidentSeverity`
- `crates/agenticos-domain/src/action.rs` — `ActionKind::{ProcessFreezeGroup, ProcessTerminateGroup, WorkloadClassifyRecommend}`
- `crates/agenticos-policy/src/policy_kernel.rs` — `DefaultPolicyKernel` allowlist configuration
- `crates/agenticos-domain/src/agent.rs` — `CapabilityScope::ProposalOnly`
