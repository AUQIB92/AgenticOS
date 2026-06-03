pub mod workload;
pub mod harness;
pub mod replay;
pub mod experiment;
pub mod compare;

// Re-exports
pub use harness::{run_workload, run_all_workloads, AggregateResult, TickSample};
pub use workload::{WorkloadConfig, WorkloadGenerator, WorkloadKind};

#[cfg(test)]
mod tests {
    use super::*;
    use harness::run_workload;
    use workload::WorkloadKind;

    const TEST_TICKS: u64 = 3;
    const TEST_SEED: u64 = 42;

    // ---------------------------------------------------------------
    // B1: Benchmark Harness — all 5 workloads run without error
    // ---------------------------------------------------------------
    #[test]
    fn benchmark_cpu_contention() {
        let config = WorkloadConfig {
            kind: WorkloadKind::CpuContention,
            seed: TEST_SEED,
            tick_count: TEST_TICKS,
            ..WorkloadConfig::default()
        };
        let (samples, agg) = run_workload(config);
        assert_eq!(samples.len(), 3);
        assert!(agg.total_proposals > 0);
    }

    #[test]
    fn benchmark_memory_pressure() {
        let config = WorkloadConfig {
            kind: WorkloadKind::MemoryPressure,
            seed: TEST_SEED,
            tick_count: TEST_TICKS,
            ..WorkloadConfig::default()
        };
        let (samples, agg) = run_workload(config);
        assert_eq!(samples.len(), 3);
        assert!(agg.total_proposals > 0);
    }

    #[test]
    fn benchmark_mixed() {
        let config = WorkloadConfig {
            kind: WorkloadKind::Mixed,
            seed: TEST_SEED,
            tick_count: TEST_TICKS,
            ..WorkloadConfig::default()
        };
        let (samples, agg) = run_workload(config);
        assert_eq!(samples.len(), 3);
        assert!(agg.total_proposals > 0);
    }

    #[test]
    fn benchmark_process_explosion() {
        let config = WorkloadConfig {
            kind: WorkloadKind::ProcessExplosion,
            seed: TEST_SEED,
            tick_count: TEST_TICKS,
            process_count: 60,
            ..WorkloadConfig::default()
        };
        let (samples, agg) = run_workload(config);
        assert_eq!(samples.len(), 3);
        // Security Agent should have produced incidents
        assert!(agg.total_incidents > 0);
    }

    #[test]
    fn benchmark_incident_storm() {
        let config = WorkloadConfig {
            kind: WorkloadKind::IncidentStorm,
            seed: TEST_SEED,
            tick_count: TEST_TICKS,
            process_count: 80,
            ..WorkloadConfig::default()
        };
        let (samples, agg) = run_workload(config);
        assert_eq!(samples.len(), 3);
        assert!(agg.total_incidents > 0);
    }

    // ---------------------------------------------------------------
    // B2: Replay — 10 deterministic replays
    // ---------------------------------------------------------------
    #[test]
    fn deterministic_replay_cpu() {
        let result = replay::verify_deterministic_replay(
            WorkloadKind::CpuContention,
            TEST_SEED,
            TEST_TICKS,
            10, // 10 replays
        );
        assert!(result.passed, "replay mismatch: {:?}", result.mismatches);
    }

    #[test]
    fn deterministic_replay_memory() {
        let result = replay::verify_deterministic_replay(
            WorkloadKind::MemoryPressure,
            TEST_SEED,
            TEST_TICKS,
            10,
        );
        assert!(result.passed, "replay mismatch: {:?}", result.mismatches);
    }

    #[test]
    fn deterministic_replay_process_explosion() {
        let result = replay::verify_deterministic_replay(
            WorkloadKind::ProcessExplosion,
            TEST_SEED,
            TEST_TICKS,
            10,
        );
        assert!(result.passed, "replay mismatch: {:?}", result.mismatches);
    }

    // ---------------------------------------------------------------
    // B3: Experiment — parameter sweep
    // ---------------------------------------------------------------
    #[test]
    fn experiment_sweep_pressure() {
        use std::collections::HashMap;
        let mut sweeps = HashMap::new();
        sweeps.insert("target_pressure".into(), vec![0.5, 0.8, 0.95]);

        let manifest = experiment::ExperimentManifest {
            name: "pressure-sweep-test".into(),
            description: "test sweep".into(),
            seeds: vec![42],
            tick_counts: vec![3],
            workloads: vec![WorkloadKind::CpuContention],
            sweeps,
        };
        let result = experiment::run_experiment(manifest);
        // 1 seed × 1 tick_count × 3 pressures = 3 runs
        assert_eq!(result.runs.len(), 3);
        for run in &result.runs {
            assert!(
                run.params.contains_key("target_pressure"),
                "missing target_pressure param"
            );
        }
    }

    // ---------------------------------------------------------------
    // B4: Comparative — compare pipeline modes
    // ---------------------------------------------------------------
    #[test]
    fn comparative_all_modes() {
        let results = compare::run_comparative(
            WorkloadKind::CpuContention,
            TEST_SEED,
            TEST_TICKS,
        );
        // 3 modes × 3 ticks = 9 results
        assert_eq!(results.len(), 9);
        let modes: std::collections::HashSet<String> = results.iter().map(|r| r.mode.clone()).collect();
        assert!(modes.contains("policy-only"));
        assert!(modes.contains("agents+policy"));
        assert!(modes.contains("full"));
    }

    #[test]
    fn comparative_incident_awareness() {
        // ProcessExplosion with MemoryAgent to generate proposals + incidents
        let results = compare::run_comparative(
            WorkloadKind::Mixed,
            TEST_SEED,
            TEST_TICKS,
        );
        // 3 modes × 3 ticks = 9 results
        assert_eq!(results.len(), 9);
        // Full mode always has >= vetoes compared to agents+policy mode
        let full_vetoes: u64 = results.iter().filter(|r| r.mode == "full").map(|r| r.veto_count).sum();
        let agents_vetoes: u64 = results.iter().filter(|r| r.mode == "agents+policy").map(|r| r.veto_count).sum();
        assert!(full_vetoes >= agents_vetoes, "full mode should have at least as many vetoes as agents+policy mode");
    }
}
