# Alpha-4 Governance Validation Report

**Date:** 2026-06-02  
**Status:** Complete  
**Campaign:** Graduated incident response (Warning→Execute, Error→SelectiveVeto, Critical→GlobalFreeze)  
**Binary:** `alpha4` (crates/agenticos-bench/src/bin/alpha4.rs)  
**Results:** `experiments/alpha4/results/alpha4-results.json`

---

## 1. Objective

Validate the graduated incident-triggered veto system in a controlled synthetic environment. Three scenarios exercise all three severity levels:

| Scenario | Incident Severity | Expected Behaviour |
|----------|-------------------|--------------------|
| A | Warning | All proposals pass (0 vetoes, 5 executions) |
| B | Error | Resource-modifying proposals vetoed (SelectiveVeto), advisory pass (5 vetoes, 5 executions) |
| C | Critical | All proposals vetoed via GlobalFreeze (IncidentTriggered, 0 executions) |

Each scenario runs for 5 ticks with 5 replay iterations (25 samples total per scenario).

---

## 2. Metrics

### 2.1 Summary Table

| Mode | Vetoes | SelectiveVeto | IncidentTriggered | Executions | RM-ratio | Liveness |
|------|--------|---------------|-------------------|------------|----------|----------|
| Warning | 0 | 0 | 0 | 5 | 0% | ✅ |
| Error | 5 | 5 | 0 | 5 | 50% | ✅ |
| Critical | 15 | 0 | 15 | 0 | 67% | ✅ |

### 2.2 Scenario A — Warning → Execute

| Metric | Value |
|--------|-------|
| Iterations | 5 |
| Ticks per iteration | 5 |
| Total proposals | 5 |
| Resource-modifying | 0 |
| Advisory | 5 |
| Total incidents | 5 |
| Total vetoes | 0 |
| Approved | 5 |
| Denied | 0 |
| Executions | 5 |
| Replay consistent | true |

**Proposal type breakdown:** `WorkloadClassifyRecommend: 5`  
**Execution breakdown:** `WorkloadClassifyRecommend: 5`  
**Veto reason breakdown:** (none)

### 2.3 Scenario B — Error → SelectiveVeto

| Metric | Value |
|--------|-------|
| Iterations | 5 |
| Ticks per iteration | 5 |
| Total proposals | 10 |
| Resource-modifying | 5 (`CgroupSetCpuMax`) |
| Advisory | 5 (`WorkloadClassifyRecommend`) |
| Total incidents | 10 |
| Total vetoes | 5 |
| SelectiveVeto | 5 |
| IncidentTriggered | 0 |
| Approved | 5 |
| Denied | 0 |
| Executions | 5 |
| Replay consistent | true |

**Proposal type breakdown:** `WorkloadClassifyRecommend: 5`, `CgroupSetCpuMax: 5`  
**Execution breakdown:** `WorkloadClassifyRecommend: 5`  
**Veto reason breakdown:** `SelectiveVeto: 5`

### 2.4 Scenario C — Critical → GlobalFreeze

| Metric | Value |
|--------|-------|
| Iterations | 5 |
| Ticks per iteration | 5 |
| Total proposals | 15 |
| Resource-modifying | 10 (`CgroupSetCpuMax: 5`, `CgroupSetCpuWeight: 5`) |
| Advisory | 5 (`WorkloadClassifyRecommend`) |
| Total incidents | 26 |
| Total vetoes | 15 |
| SelectiveVeto | 0 |
| IncidentTriggered | 15 |
| Approved | 0 |
| Denied | 0 |
| Executions | 0 |
| Replay consistent | true |

**Proposal type breakdown:** `CgroupSetCpuMax: 5`, `CgroupSetCpuWeight: 5`, `WorkloadClassifyRecommend: 5`  
**Execution breakdown:** (none)  
**Veto reason breakdown:** `IncidentTriggered: 15`

---

## 3. Verification Results

| Check | Scenario A | Scenario B | Scenario C |
|-------|-----------|-----------|-----------|
| Replay consistency | ✅ | ✅ | ✅ |
| `executions > 0` | ✅ (5) | ✅ (5) | ❌ (0, as expected) |
| `vetoes = 0` | ✅ | ❌ (5, as expected) | ❌ (15, as expected) |
| `selective_vetoes > 0` | — | ✅ | — |
| `veto ≈ proposals` | — | — | ✅ (100%) |
| `executions = 0` | — | — | ✅ |
| `reason = IncidentTriggered` | — | — | ✅ |

**All three scenarios pass.**

---

## 4. Selective-Veto Saturation Analysis

### 4.1 Phenomenon

When `check_incident_trigger` encounters an `Error`-severity incident, it vetoes all resource-modifying proposals (`CgroupSetCpuMax`, `CgroupSetCpuWeight`, `CgroupSetMemoryMax`). Non-resource-modifying proposals (advisory, e.g., `WorkloadClassifyRecommend`) pass through.

If 100% of proposals at a given severity level are resource-modifying, **all** proposals are vetoed, yielding `executor_count = 0`.

### 4.2 Reproduction Condition

This occurs when the environment produces only observation types that trigger resource-modifying proposals and no advisory proposals. For example:

- WSL lacks `/proc/pressure/cpu` → CPU pressure = 0.0 → `WorkloadClassifyRecommend` never fires
- Only `CgroupSetCpuMax`/`CgroupSetCpuWeight` proposals are generated
- SelectiveVeto blocks everything → zero execution

### 4.3 Mitigation Checklist

If a real deployment shows 100% resource-modifying proposals:

1. Does `/proc/pressure/cpu` exist and return valid data?
2. Are non-resource-modifying agents (e.g., `ObserveOnly`) registered?
3. Is the workload generating the right observation types for advisory proposals?

### 4.4 Design Decision

This is **not a bug** — it is correct behaviour of the graduated response. The system prioritises safety over liveness during Error incidents, and if all proposals are deemed risky, blocking all of them is the conservative choice. In practice, real deployments should always have some advisory-only agents (observe/recommend) to maintain liveness.

---

## 5. Test Coverage

| Test | File | Status |
|------|------|--------|
| `critical_incident_triggers_global_veto` | `governor.rs` | ✅ |
| `warning_incident_no_auto_veto` | `governor.rs` | ✅ |
| `error_incident_selective_veto` | `governor.rs` | ✅ |
| Scenario A (alpha4) | `alpha4.rs` | ✅ |
| Scenario B (alpha4) | `alpha4.rs` | ✅ |
| Scenario C (alpha4) | `alpha4.rs` | ✅ |

Total: 98 tests pass (90 unit + 8 integration/experiment).

---

## 6. Limitations

1. **Synthetic workload only.** Scenarios create proposals/incidents programmatically rather than via `SystemSampler`. Real-world observation → proposal mapping may produce different veto patterns.
2. **No adversarial mixing.** Scenarios test each severity in isolation. A real tick may have multiple incidents at different severities; the current logic picks the max severity per tick.
3. **SelectiveVeto saturation is unmasked but unhandled.** The diagnostic explains the root cause but no automatic mitigation (e.g., fallback to allow advisory proposals) is implemented. This is a deliberate safety-first choice.
4. **Threshold-0 for Critical in SecurityAgent.** The `high_process_critical_threshold = Some(0)` in Scenario C triggers Critical on every tick, which is unrealistic. Real deployments should set a higher threshold.
