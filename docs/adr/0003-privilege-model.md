# ADR 0003: Privilege Model — Drop All Capabilities After cgroup Hierarchy Setup

**Status:** Accepted  
**Date:** 2026-06-02  
**Deciders:** Research team

## Context

The daemon must write to `/sys/fs/cgroup/` to manage cgroup v2 hierarchies. This requires root or `CAP_SYS_ADMIN` at startup to create the agenticos cgroup root and configure delegation.

The question is whether the daemon should retain `CAP_SYS_ADMIN` after initialization for ongoing cgroup operations.

## Decision

The daemon retains root only during a short initialization phase:

1. **Startup (root):** Create `/sys/fs/cgroup/agenticos/`. Set ownership and permissions so the daemon's uid/gid can write to child cgroups without privilege. Configure `cgroup.subtree_control` to enable `cpu` and `memory` controllers.

2. **After init:** Drop all capabilities including `CAP_SYS_ADMIN` and `CAP_DAC_OVERRIDE`. All subsequent cgroup operations go through normal file I/O on the pre-configured hierarchy.

3. **`CGROUP_DELEGATION`:** Use kernel cgroup delegation — child cgroups inherit writable permissions from the parent when ownership is set correctly. The daemon operates as an unprivileged user within its delegated subtree.

## Consequences

Positive:
- The daemon runs without any retained capability after initialization.
- cgroup operations are ordinary file writes, not privileged operations.
- A compromise of the daemon process does not yield `CAP_SYS_ADMIN` to an attacker.
- The model aligns with container runtime delegation patterns (Docker, podman).

Negative:
- The daemon cannot create new top-level cgroup hierarchies at runtime.
- All experiment cgroups must live under the pre-created `/sys/fs/cgroup/agenticos/` tree.
- Initial setup requires root (or a privileged init script / sudo helper).
