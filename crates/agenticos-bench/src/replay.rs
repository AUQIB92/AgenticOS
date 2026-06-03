use serde::Serialize;

use crate::harness;
use crate::workload::{WorkloadConfig, WorkloadKind};

/// Run N deterministic replays of the same workload, verify every tick
/// produces identical outcomes.
///
/// Returns the number of passes and any mismatches found.
pub fn verify_deterministic_replay(
    kind: WorkloadKind,
    seed: u64,
    ticks: u64,
    iterations: usize,
) -> ReplayResult {
    let mut mismatches: Vec<ReplayMismatch> = Vec::new();

    let config = WorkloadConfig {
        kind: kind.clone(),
        seed,
        tick_count: ticks,
        ..WorkloadConfig::default()
    };

    // Run once to establish baseline
    let (baseline_samples, baseline_agg) = harness::run_workload(config.clone());

    // Re-run with same seed, compare every tick
    for iteration in 1..iterations {
        let (samples, _agg) = harness::run_workload(config.clone());

        if samples.len() != baseline_samples.len() {
            mismatches.push(ReplayMismatch {
                iteration,
                tick: 0,
                detail: format!(
                    "sample count mismatch: got {} expected {}",
                    samples.len(),
                    baseline_samples.len()
                ),
            });
            continue;
        }

        for (_tick_idx, (s, baseline)) in samples.iter().zip(baseline_samples.iter()).enumerate() {
            if s.obs_count != baseline.obs_count {
                mismatches.push(ReplayMismatch {
                    iteration,
                    tick: s.tick,
                    detail: format!(
                        "obs_count mismatch: got {} expected {}",
                        s.obs_count, baseline.obs_count
                    ),
                });
            }
            if s.proposal_count != baseline.proposal_count {
                mismatches.push(ReplayMismatch {
                    iteration,
                    tick: s.tick,
                    detail: format!(
                        "proposal_count mismatch: got {} expected {}",
                        s.proposal_count, baseline.proposal_count
                    ),
                });
            }
            if s.incident_count != baseline.incident_count {
                mismatches.push(ReplayMismatch {
                    iteration,
                    tick: s.tick,
                    detail: format!(
                        "incident_count mismatch: got {} expected {}",
                        s.incident_count, baseline.incident_count
                    ),
                });
            }
            if s.approved_count != baseline.approved_count {
                mismatches.push(ReplayMismatch {
                    iteration,
                    tick: s.tick,
                    detail: format!(
                        "approved_count mismatch: got {} expected {}",
                        s.approved_count, baseline.approved_count
                    ),
                });
            }
            if s.denied_count != baseline.denied_count {
                mismatches.push(ReplayMismatch {
                    iteration,
                    tick: s.tick,
                    detail: format!(
                        "denied_count mismatch: got {} expected {}",
                        s.denied_count, baseline.denied_count
                    ),
                });
            }
            if s.veto_count != baseline.veto_count {
                mismatches.push(ReplayMismatch {
                    iteration,
                    tick: s.tick,
                    detail: format!(
                        "veto_count mismatch: got {} expected {}",
                        s.veto_count, baseline.veto_count
                    ),
                });
            }
        }
    }

    let passed = mismatches.is_empty();
    ReplayResult {
        workload: kind.name().into(),
        seed,
        ticks,
        iterations,
        passed,
        mismatches,
        total_approved: baseline_agg.total_approved,
        total_denied: baseline_agg.total_denied,
        total_vetoes: baseline_agg.total_vetoes,
        total_incidents: baseline_agg.total_incidents,
        mean_latency_ms: baseline_agg.mean_total_ms,
    }
}

#[derive(Debug, Serialize)]
pub struct ReplayResult {
    pub workload: String,
    pub seed: u64,
    pub ticks: u64,
    pub iterations: usize,
    pub passed: bool,
    pub mismatches: Vec<ReplayMismatch>,
    pub total_approved: u64,
    pub total_denied: u64,
    pub total_vetoes: u64,
    pub total_incidents: u64,
    pub mean_latency_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct ReplayMismatch {
    pub iteration: usize,
    pub tick: u64,
    pub detail: String,
}
