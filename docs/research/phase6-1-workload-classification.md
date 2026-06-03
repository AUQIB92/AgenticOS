# Phase 6.1 — Workload Classification Agent Report

**Date:** 2026-06-03  
**Status:** Complete  
**Crate:** `agenticos-intelligence` (classifier module)

---

## 1. Architecture

```
Observations (Process, Cgroup, Cpu, Memory)
         │
         ▼
WorkloadClassificationAgent
         │
         ├── build_summary() → WorkloadObservationSummary
         │     (cpu%, mem%, process_count, process_names, pressure)
         │
         ├── summary_to_context() → RecommendationContext
         │
         ├── WorkloadClassifier (implements LlmProvider)
         │     └── classify(summary) → (WorkloadClass, confidence, reasoning)
         │
         └── generate_recommendation() → Recommendation
               │
               ├── TraceStore (persist + replay)
               ├── CLI (agenticos recommendations)
               └── Metrics (classification_count, per-class counters)
```

### 1.1 Boundary

```
Governance Pipeline:              Intelligence Layer:
Observation                        WorkloadObservationSummary
    │                                      │
    ▼                                      ▼
Proposal → Policy → Safety → Executor    WorkloadClassifier
                                              │
                                              ▼
                                         Recommendation
                                              │
                                              ├── TraceStore
                                              ├── CLI
                                              └── Metrics
                                              │
         No path from Recommendation to Proposal, Policy, Safety, or Executor
```

---

## 2. New Types

### `WorkloadClass` enum (agenticos-domain)

```rust
pub enum WorkloadClass {
    Database,
    Interactive,
    Build,
    Batch,
    SystemService,
    Unknown,
}
```

- `label()` → human-readable string (e.g. `"Database"`)
- Derives `Serialize`, `Deserialize`, `Clone`, `Debug`, `PartialEq`

### `WorkloadObservationSummary` struct (agenticos-domain)

```rust
pub struct WorkloadObservationSummary {
    pub cpu_utilization: f64,      // 0.0–100.0
    pub memory_utilization: f64,   // 0.0–100.0
    pub process_count: u32,
    pub process_names: Vec<String>,
    pub cpu_pressure: Option<f64>,  // PSI some avg10
}
```

---

## 3. Classification Rules

Rules are evaluated in priority order. The first matching rule determines the classification.

| Priority | Rule | Class | Confidence |
|----------|------|-------|------------|
| 1 | Has database process (postgres, mysql, mongod, redis, ...) AND max CPU > 30% | Database | 0.92 |
| 2 | ≥2 compiler processes (rustc, gcc, cargo, make, clang, ...) | Build | 0.88 |
| 3 | Has interactive process (Xorg, firefox, terminal, ...) AND max CPU < 40% | Interactive | 0.85 |
| 4 | Has system process (systemd, journald, dbus, ...) AND process count ≤ 15 | SystemService | 0.80 |
| 5 | CPU > 70% AND process count > 20, no specific signature | Batch | 0.75 |
| 6 | Default (no rules matched) | Unknown | 0.50 |

### Confidence Levels

Classification confidence is fixed per class:

| Class | Confidence | Rationale |
|-------|-----------|-----------|
| Database | 0.92 | Strong signal (specific process + high CPU) |
| Build | 0.88 | Strong signal (multiple compiler processes) |
| Interactive | 0.85 | Moderate signal (user processes + low CPU) |
| SystemService | 0.80 | Moderate signal (system processes, low count) |
| Batch | 0.75 | Weak signal (high CPU, many processes, no signature) |
| Unknown | 0.50 | No matching pattern |

---

## 4. Observation Extraction

The `WorkloadClassificationAgent::build_summary()` extracts system state from observations:

| Metric | Source | Strategy |
|--------|--------|----------|
| CPU utilization | `ProcessObservation::cpu_percent` | Max across all processes |
| CPU utilization (fallback) | `CgroupObservation::cpu_usage_usec` | Heuristic: `usage / 10000` (capped at 100) |
| Memory utilization | `MemoryObservation::used_bytes / total_bytes` | Scaled to 0–100 |
| Process count | `CgroupObservation::processes` | Direct read |
| Process names | `ProcessObservation::command` | Collected into Vec<String> |
| CPU pressure | `CpuObservation::pressure_some_avg10` | PSI metric (0.0–1.0) |

---

## 5. Metrics

The agent tracks per-class counters:

```
classification_count       → total classifications performed
classification_Database    → count of Database classifications
classification_Interactive → count of Interactive classifications
classification_Build       → count of Build classifications
classification_Batch       → count of Batch classifications
classification_SystemService → count of SystemService classifications
classification_Unknown    → count of Unknown classifications
```

Metrics are available via `WorkloadClassificationAgent`:
- `agent.classification_count()` → total
- `agent.class_count(&WorkloadClass::Database)` → per-class counter

And can be serialized to `MetricCollection` for CLI display.

---

## 6. CLI: `agenticos recommendations`

```
agenticos recommendations
```

Output format (table):

```
TIMESTAMP                   AGENT        CLASSIFICATION  CONFIDENCE  SUMMARY
2026-06-03T00:00:00Z        classifier   Database        0.92        Workload classified as Database
2026-06-03T00:00:01Z        classifier   Build           0.88        Workload classified as Build
```

With `--json` flag:

```json
[
  {
    "timestamp": "2026-06-03T00:00:00Z",
    "agent": "classifier",
    "classification": "Database",
    "confidence": 0.92,
    "summary": "Workload classified as Database"
  }
]
```

Queries the SQLite `traces` table for all events with topic prefix `recommendations.*` and extracts `Recommendation` payload fields.

---

## 7. Traceability

| Property | Mechanism |
|----------|-----------|
| Persistence | `EventPayload::Recommendation(Recommendation)` stored in SQLite `traces` table via `InMemoryTraceStore` or `SqliteTraceStore` |
| Replay | `TraceStore::replay(trace_id)` reconstructs exact `Recommendation` (id, summary, reasoning, confidence, source_agent) |
| Determinism | Same observations → same classification → same `Recommendation` (same id via `RecommendationId::new()` is sequential but classification result is deterministic) |
| Topic routing | `recommendations.{agent_name}` convention |

### Replay example

```rust
let store = SqliteTraceStore::new("agenticos.db").unwrap();
let events = store.replay(TraceId::from("my-trace")).unwrap();
for event in events {
    if let EventPayload::Recommendation(rec) = &event.payload {
        println!("{}: {} ({})", rec.summary, rec.confidence, rec.reasoning);
    }
}
```

---

## 8. Files Changed

### New Files

| File | Lines | Purpose |
|------|-------|---------|
| `crates/agenticos-domain/src/workload.rs` | 130 | `WorkloadClass` enum + `WorkloadObservationSummary` struct + tests |
| `crates/agenticos-intelligence/src/classifier.rs` | 685 | `WorkloadClassifier`, `WorkloadClassificationAgent`, all tests |
| `docs/research/phase6-1-workload-classification.md` | — | This report |

### Modified Files

| File | Change |
|------|--------|
| `crates/agenticos-domain/src/lib.rs` | Added `pub mod workload; pub use workload::*;` |
| `crates/agenticos-domain/src/metrics.rs` | Added `with_classification_count()`, `with_classification_class()` methods |
| `crates/agenticos-intelligence/src/lib.rs` | Added `pub mod classifier;` and re-exports |
| `crates/agenticos-cli/src/main.rs` | Added `Recommendations` command variant, `cmd_recommendations()` handler, test helpers and tests |

---

## 9. Tests Added

| Test | File | Type |
|------|------|------|
| `workload_class_label_database` | domain/workload.rs | Unit |
| `workload_class_label_interactive` | domain/workload.rs | Unit |
| `workload_class_label_build` | domain/workload.rs | Unit |
| `workload_class_label_batch` | domain/workload.rs | Unit |
| `workload_class_label_system_service` | domain/workload.rs | Unit |
| `workload_class_label_unknown` | domain/workload.rs | Unit |
| `workload_class_serde_round_trip` | domain/workload.rs | Serde |
| `workload_observation_summary_constructs` | domain/workload.rs | Unit |
| `workload_observation_summary_serde_round_trip` | domain/workload.rs | Serde |
| `workload_observation_summary_empty_process_names` | domain/workload.rs | Unit |
| `classify_database_workload` | intelligence/classifier.rs | Behaviour |
| `classify_build_workload` | intelligence/classifier.rs | Behaviour |
| `classify_interactive_workload` | intelligence/classifier.rs | Behaviour |
| `classify_unknown_workload` | intelligence/classifier.rs | Behaviour |
| `classify_system_service_workload` | intelligence/classifier.rs | Behaviour |
| `classify_batch_workload` | intelligence/classifier.rs | Behaviour |
| `deterministic_classification` | intelligence/classifier.rs | Determinism |
| `classification_counts_metrics` | intelligence/classifier.rs | Metrics |
| `recommendation_is_purely_advisory` | intelligence/classifier.rs | Boundary |
| `trace_persistence_and_replay` | intelligence/classifier.rs | TraceStore |
| `parse_summary_from_context_full` | intelligence/classifier.rs | Context parsing |
| `parse_summary_from_context_partial` | intelligence/classifier.rs | Context parsing |
| `build_summary_from_observations` | intelligence/classifier.rs | Extraction |
| `recommendation_has_no_action_fields` | intelligence/classifier.rs | Boundary |
| `test_recommendations_empty_db` | cli/main.rs | CLI |
| `test_recommendations_with_data` | cli/main.rs | CLI |

**Total new tests:** 26  
**Total workspace tests:** 133+ (all pass, zero new warnings)

---

## 10. Hard Constraints Verification

| Constraint | Status |
|------------|--------|
| No proposals created | ✅ `WorkloadClassificationAgent` produces `Recommendation` only |
| No executor invoked | ✅ No dependency on `agenticos-executor` |
| No Linux mutations | ✅ No cgroup, file, or process operations |
| No policy kernel changes | ✅ Zero changes to `agenticos-policy` crate |
| No safety governor changes | ✅ Zero changes to `agenticos-safety` crate |
| Recommendation-only output | ✅ Type system enforces `LlmProvider` returns `Recommendation` |

---

## 11. Limitations

1. **CPU estimation from cgroup** — The cgroup fallback (`cpu_usage_usec / 10000`) is a heuristic. A single cumulative counter without a time delta cannot express a true utilization percentage. The primary extraction path (process `cpu_percent`) is preferred.

2. **Process name matching** — Classification uses substring matching against known process names. New processes (e.g., `timescaledb` for database, `buck2` for build) would be misclassified until added to the keyword lists.

3. **Single-agent focus** — The `WorkloadClassificationAgent` classifies all observations as a single workload. In multi-workload cgroup hierarchies, each cgroup would need its own agent instance.

4. **No feedback loop** — The agent does not learn from classification outcomes. Confidence values are fixed per class.

5. **Sequential IDs in tests** — `RecommendationId::new()` is sequential (`RecommendationId-1`, `RecommendationId-2`, ...), so the `deterministic_classification` test compares summary/reasoning but not ID.

---

## 12. Future LLM Integration Path

The `WorkloadClassifier` implements `LlmProvider`, which means it can be replaced by an actual LLM provider in Phase 6.2:

```
WorkloadClassificationAgent
    │
    └── LlmProvider (trait)
            │
            ├── WorkloadClassifier (current — deterministic rules)
            │
            └── OpenAiProvider (future — real LLM)
```

The agent does not depend on the implementation — it only calls `LlmProvider::generate_recommendation()`. To swap:

```rust
// Current:
let agent = WorkloadClassificationAgent::new(agent_id);

// Future with LLM:
let provider = OpenAiProvider::new("gpt-4o");
let agent = WorkloadClassificationAgent::with_provider(agent_id, provider);
```

The `RecommendationContext` provides the same interface regardless of provider.
