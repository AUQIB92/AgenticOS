use std::collections::HashMap;

use serde::Serialize;

use crate::harness::{self, AggregateResult, TickSample};
use crate::workload::{WorkloadConfig, WorkloadKind};

/// A named experiment that runs one or more workload configurations.
#[derive(Clone, Debug, Serialize)]
pub struct ExperimentManifest {
    pub name: String,
    pub description: String,
    pub seeds: Vec<u64>,
    pub tick_counts: Vec<u64>,
    pub workloads: Vec<WorkloadKind>,
    /// Parameter sweeps — maps parameter name to list of values.
    /// Supported: "target_pressure", "target_memory_usage", "process_count", "incident_probability"
    pub sweeps: HashMap<String, Vec<f64>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExperimentResult {
    pub manifest: ExperimentManifest,
    pub runs: Vec<WorkloadRunResult>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkloadRunResult {
    pub workload: String,
    pub seed: u64,
    pub ticks: u64,
    /// Parameter values for this run (from sweeps).
    pub params: HashMap<String, f64>,
    pub aggregate: AggregateResult,
    pub per_tick_samples: Vec<TickSample>,
}

/// Execute all runs defined by an experiment manifest.
pub fn run_experiment(manifest: ExperimentManifest) -> ExperimentResult {
    let mut runs: Vec<WorkloadRunResult> = Vec::new();

    for kind in &manifest.workloads {
        let seeds = if manifest.seeds.is_empty() {
            vec![42]
        } else {
            manifest.seeds.clone()
        };
        let ticks_list = if manifest.tick_counts.is_empty() {
            vec![10]
        } else {
            manifest.tick_counts.clone()
        };

        for &seed in &seeds {
            for &tick_count in &ticks_list {
                // Base config
                let base_config = WorkloadConfig {
                    kind: kind.clone(),
                    seed,
                    tick_count,
                    ..WorkloadConfig::default()
                };

                // If no sweeps, run base config once
                if manifest.sweeps.is_empty() {
                    let (samples, aggregate) = harness::run_workload(base_config);
                    runs.push(WorkloadRunResult {
                        workload: kind.name().into(),
                        seed,
                        ticks: tick_count,
                        params: HashMap::new(),
                        aggregate,
                        per_tick_samples: samples,
                    });
                } else {
                    // Parameter sweep: iterate over cartesian product of sweep params
                    let sweep_keys: Vec<String> = manifest.sweeps.keys().cloned().collect();
                    let sweep_values: Vec<Vec<f64>> = manifest.sweeps.values().cloned().collect();
                    let combinations = cartesian_product(&sweep_values);

                    for combo in combinations {
                        let mut config = base_config.clone();
                        let mut params: HashMap<String, f64> = HashMap::new();
                        for (i, key) in sweep_keys.iter().enumerate() {
                            let val = combo[i];
                            params.insert(key.clone(), val);
                            match key.as_str() {
                                "target_pressure" => config.target_pressure = val,
                                "target_memory_usage" => config.target_memory_usage = val,
                                "process_count" => config.process_count = val as usize,
                                "incident_probability" => config.incident_probability = val,
                                _ => {}
                            }
                        }

                        let (samples, aggregate) = harness::run_workload(config);
                        runs.push(WorkloadRunResult {
                            workload: kind.name().into(),
                            seed,
                            ticks: tick_count,
                            params,
                            aggregate,
                            per_tick_samples: samples,
                        });
                    }
                }
            }
        }
    }

    ExperimentResult {
        manifest,
        runs,
    }
}

/// Export experiment results as JSON.
pub fn write_experiment_json(
    result: &ExperimentResult,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(result)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Export aggregate results as CSV (one row per run).
pub fn write_experiment_csv(
    result: &ExperimentResult,
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record(&[
        "workload", "seed", "ticks",
        "mean_total_ms", "p50_total_ms", "p95_total_ms", "p99_total_ms",
        "mean_collect_proposals_ms", "mean_policy_eval_ms", "mean_safety_eval_ms", "mean_executor_ms",
        "total_proposals", "total_incidents", "total_vetoes", "total_approved", "total_denied", "total_executions",
    ])?;
    for run in &result.runs {
        let a = &run.aggregate;
        wtr.write_record(&[
            &run.workload,
            &run.seed.to_string(),
            &run.ticks.to_string(),
            &format!("{:.4}", a.mean_total_ms),
            &format!("{:.4}", a.p50_total_ms),
            &format!("{:.4}", a.p95_total_ms),
            &format!("{:.4}", a.p99_total_ms),
            &format!("{:.4}", a.mean_collect_proposals_ms),
            &format!("{:.4}", a.mean_policy_eval_ms),
            &format!("{:.4}", a.mean_safety_eval_ms),
            &format!("{:.4}", a.mean_executor_ms),
            &a.total_proposals.to_string(),
            &a.total_incidents.to_string(),
            &a.total_vetoes.to_string(),
            &a.total_approved.to_string(),
            &a.total_denied.to_string(),
            &a.total_executions.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn cartesian_product(lists: &[Vec<f64>]) -> Vec<Vec<f64>> {
    if lists.is_empty() {
        return vec![vec![]];
    }
    let mut result: Vec<Vec<f64>> = vec![vec![]];
    for list in lists {
        let mut new_result: Vec<Vec<f64>> = Vec::new();
        for prefix in &result {
            for value in list {
                let mut combined = prefix.clone();
                combined.push(*value);
                new_result.push(combined);
            }
        }
        result = new_result;
    }
    result
}
