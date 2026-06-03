//! Alpha-2 Experimental Campaign Runner.
//!
//! Executes E1–E5 experiment sets and writes results to:
//!   experiments/results/   — raw per-tick and aggregate data
//!   experiments/tables/    — publication-ready summary tables
//!   experiments/figures/   — CSV data files ready for plotting
//!
//! Usage: cargo run --bin experiment [--output-dir <dir>]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use agenticos_bench::compare;
use agenticos_bench::experiment::{self, ExperimentManifest};
use agenticos_bench::replay;
use agenticos_bench::workload::WorkloadKind;
use agenticos_bench::{
    harness, run_workload, AggregateResult, TickSample, WorkloadConfig,
};
use agenticos_policy::DeterministicPolicyKernel;
use serde::Serialize;

const TICKS: u64 = 10;
const SEED: u64 = 42;
const REPLAY_ITERATIONS: usize = 1000;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let base_dir = if args.len() > 2 && args[1] == "--output-dir" {
        args[2].clone()
    } else {
        "experiments".into()
    };

    let base = PathBuf::from(&base_dir);
    let results_dir = base.join("results");
    let tables_dir = base.join("tables");
    let figures_dir = base.join("figures");

    for d in [&results_dir, &tables_dir, &figures_dir] {
        fs::create_dir_all(d).expect("failed to create output directory");
    }
    for d in ["e1-determinism", "e2-safety-governor", "e3-multi-agent", "e4-resource-governance", "e5-governance-overhead"] {
        let _ = fs::create_dir_all(results_dir.join(d));
        let _ = fs::create_dir_all(figures_dir.join(d));
    }

    let campaign = ExperimentalCampaign {
        _base: base,
        results_dir,
        tables_dir,
        figures_dir,
    };

    println!("=== Alpha-2 Experimental Campaign ===\n");
    campaign.run_e1().unwrap();
    campaign.run_e2().unwrap();
    campaign.run_e3().unwrap();
    campaign.run_e4().unwrap();
    campaign.run_e5().unwrap();
    println!("\nAll experiments complete. Output in experiments/");
}

struct ExperimentalCampaign {
    _base: PathBuf,
    results_dir: PathBuf,
    tables_dir: PathBuf,
    figures_dir: PathBuf,
}

// ── Shared helpers ──────────────────────────────────────────────────

const ALL_WORKLOADS: [WorkloadKind; 5] = [
    WorkloadKind::CpuContention,
    WorkloadKind::MemoryPressure,
    WorkloadKind::Mixed,
    WorkloadKind::ProcessExplosion,
    WorkloadKind::IncidentStorm,
];

fn run_workload_default(kind: WorkloadKind) -> (Vec<TickSample>, AggregateResult) {
    let config = WorkloadConfig {
        kind,
        seed: SEED,
        tick_count: TICKS,
        ..WorkloadConfig::default()
    };
    run_workload(config)
}

fn write_json<T: Serialize>(path: &Path, data: &T) {
    let json = serde_json::to_string_pretty(data).unwrap();
    fs::write(path, json).unwrap();
}

fn write_csv_string(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
}

// ═══════════════════════════════════════════════════════════════════
//  E1: Determinism
// ═══════════════════════════════════════════════════════════════════

impl ExperimentalCampaign {
    fn run_e1(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("── E1: Determinism ──");
        let mut summary_rows: Vec<E1Row> = Vec::new();

        for kind in &ALL_WORKLOADS {
            let name = kind.name();
            println!("  {name}: {REPLAY_ITERATIONS} replays...");
            let t0 = Instant::now();

            let result = replay::verify_deterministic_replay(
                kind.clone(),
                SEED,
                TICKS,
                REPLAY_ITERATIONS,
            );

            let elapsed = t0.elapsed();
            println!(
                "    passed={}  mismatches={}  ({:.2}s)",
                result.passed,
                result.mismatches.len(),
                elapsed.as_secs_f64(),
            );

            let row = E1Row {
                workload: name.into(),
                seed: result.seed,
                ticks: result.ticks,
                iterations: result.iterations,
                passed: result.passed,
                mismatch_count: result.mismatches.len() as u64,
                total_approved: result.total_approved,
                total_denied: result.total_denied,
                total_vetoes: result.total_vetoes,
                total_incidents: result.total_incidents,
                mean_latency_ms: result.mean_latency_ms,
            };
            summary_rows.push(row);

            // Per-workload JSON
            let json_path = self.results_dir
                .join("e1-determinism")
                .join(format!("{name}-{REPLAY_ITERATIONS}replay.json"));
            write_json(&json_path, &result);
        }

        // Summary table
        let table = build_e1_table(&summary_rows);
        write_csv_string(&self.tables_dir.join("e1-determinism-table.csv"), &table);

        // Figure data — divergence rates
        let mut fig = String::from("workload,passed,mismatches,total_approved,total_denied,total_incidents\n");
        for r in &summary_rows {
            fig.push_str(&format!(
                "{},{},{},{},{},{}\n",
                r.workload, r.passed, r.mismatch_count, r.total_approved, r.total_denied, r.total_incidents,
            ));
        }
        write_csv_string(&self.figures_dir.join("e1-determinism").join("determinism-data.csv"), &fig);

        println!();
        Ok(())
    }
}

#[derive(Serialize)]
struct E1Row {
    workload: String,
    seed: u64,
    ticks: u64,
    iterations: usize,
    passed: bool,
    mismatch_count: u64,
    total_approved: u64,
    total_denied: u64,
    total_vetoes: u64,
    total_incidents: u64,
    mean_latency_ms: f64,
}

fn build_e1_table(rows: &[E1Row]) -> String {
    let mut s = String::new();
    s.push_str("### E1: Determinism — Replay Divergence Report\n\n");
    s.push_str("| Workload | Iterations | Passed | Mismatches | Approved | Denied | Vetoes | Incidents | Mean Latency (ms) |\n");
    s.push_str("|----------|-----------:|:------:|-----------:|--------:|------:|------:|----------:|------------------:|\n");
    for r in rows {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {:.4} |\n",
            r.workload,
            r.iterations,
            if r.passed { "✓" } else { "✗" },
            r.mismatch_count,
            r.total_approved,
            r.total_denied,
            r.total_vetoes,
            r.total_incidents,
            r.mean_latency_ms,
        ));
    }
    s
}

// ═══════════════════════════════════════════════════════════════════
//  E2: Safety Governor Impact
// ═══════════════════════════════════════════════════════════════════

impl ExperimentalCampaign {
    fn run_e2(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("── E2: Safety Governor Impact ──");

        let mut table_rows: Vec<E2Row> = Vec::new();

        for kind in &ALL_WORKLOADS {
            let name = kind.name();
            println!("  {name}...");

            let results = compare::run_comparative(kind.clone(), SEED, TICKS);
            let csv_path = self.results_dir
                .join("e2-safety-governor")
                .join(format!("{name}-comparative.csv"));
            compare::write_comparative_csv(&results, csv_path.to_str().unwrap())?;

            let json_path = self.results_dir
                .join("e2-safety-governor")
                .join(format!("{name}-comparative.json"));
            compare::write_comparative_json(&results, json_path.to_str().unwrap())?;

            // Aggregate per mode
            let mut modes: HashMap<String, Vec<&compare::ComparativeTickResult>> = HashMap::new();
            for r in &results {
                modes.entry(r.mode.clone()).or_default().push(r);
            }
            for (mode, ticks) in &modes {
                let total_proposals: u64 = ticks.iter().map(|t| t.proposal_count).sum();
                let total_incidents: u64 = ticks.iter().map(|t| t.incident_count).sum();
                let total_vetoes: u64 = ticks.iter().map(|t| t.veto_count).sum();
                let total_approved: u64 = ticks.iter().map(|t| t.approved_count).sum();
                let total_denied: u64 = ticks.iter().map(|t| t.denied_count).sum();
                let total_decisions: u64 = ticks.iter().map(|t| t.decision_count).sum();

                // Incident escalation rate: incidents / (proposals + incidents + decisions)
                let denominator = total_proposals + total_incidents + total_decisions;
                let escalation_rate = if denominator > 0 {
                    total_incidents as f64 / denominator as f64
                } else {
                    0.0
                };

                table_rows.push(E2Row {
                    workload: name.into(),
                    mode: mode.clone(),
                    proposals: total_proposals,
                    incidents: total_incidents,
                    vetoes: total_vetoes,
                    approved: total_approved,
                    denied: total_denied,
                    decisions: total_decisions,
                    escalation_rate,
                });
            }
        }

        // Summary table
        let table = build_e2_table(&table_rows);
        write_csv_string(&self.tables_dir.join("e2-safety-impact-table.csv"), &table);

        // Figure data — veto comparison agents+policy vs full
        let mut fig = String::from("workload,agents_policy_vetoes,full_vetoes,agents_policy_approved,full_approved\n");
        for row in &table_rows {
            if row.mode == "agents+policy" {
                let full = table_rows.iter().find(|r| r.workload == row.workload && r.mode == "full");
                if let Some(f) = full {
                    fig.push_str(&format!(
                        "{},{},{},{},{}\n",
                        row.workload, row.vetoes, f.vetoes, row.approved, f.approved,
                    ));
                }
            }
        }
        write_csv_string(&self.figures_dir.join("e2-safety-governor").join("safety-impact-data.csv"), &fig);

        println!();
        Ok(())
    }
}

#[derive(Serialize)]
struct E2Row {
    workload: String,
    mode: String,
    proposals: u64,
    incidents: u64,
    vetoes: u64,
    approved: u64,
    denied: u64,
    decisions: u64,
    escalation_rate: f64,
}

fn build_e2_table(rows: &[E2Row]) -> String {
    let mut s = String::new();
    s.push_str("### E2: Safety Governor Impact — Comparative Governance Report\n\n");
    s.push_str("| Workload | Mode | Proposals | Incidents | Vetoes | Approved | Denied | Decisions | Escalation Rate |\n");
    s.push_str("|----------|------|----------:|----------:|------:|--------:|------:|---------:|----------------:|\n");
    for r in rows {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {:.4} |\n",
            r.workload, r.mode, r.proposals, r.incidents, r.vetoes,
            r.approved, r.denied, r.decisions, r.escalation_rate,
        ));
    }
    s
}

// ═══════════════════════════════════════════════════════════════════
//  E3: Multi-Agent Governance
// ═══════════════════════════════════════════════════════════════════

impl ExperimentalCampaign {
    fn run_e3(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("── E3: Multi-Agent Governance ──");

        // Run Mixed workload and extract governance metrics
        let (samples, agg) = run_workload_default(WorkloadKind::Mixed);
        let name = "mixed";

        // Per-tick output
        let csv_path = self.results_dir
            .join("e3-multi-agent")
            .join(format!("{name}-governance.csv"));
        harness::write_samples_csv(&samples, csv_path.to_str().unwrap())?;

        let json_path = self.results_dir
            .join("e3-multi-agent")
            .join(format!("{name}-governance.json"));
        harness::write_samples_json(&samples, json_path.to_str().unwrap())?;

        let mut all_rows: Vec<E3Row> = Vec::new();

        let (samples2, agg2) = run_workload_default(WorkloadKind::ProcessExplosion);
        let (samples3, agg3) = run_workload_default(WorkloadKind::IncidentStorm);

        // Compute derived metrics
        for (kind_label, samples, agg) in [
            ("mixed", &samples, &agg),
            ("process-explosion", &samples2, &agg2),
            ("incident-storm", &samples3, &agg3),
        ] {
            let total_decisions = agg.total_approved + agg.total_denied;
            let proposal_conflicts = agg.total_proposals.saturating_sub(total_decisions);
            let arbitration_frequency = if TICKS > 0 {
                agg.total_vetoes as f64 / TICKS as f64
            } else {
                0.0
            };

            all_rows.push(E3Row {
                workload: kind_label.into(),
                sample_count: samples.len() as u64,
                total_proposals: agg.total_proposals,
                total_incidents: agg.total_incidents,
                total_vetoes: agg.total_vetoes,
                total_approved: agg.total_approved,
                total_denied: agg.total_denied,
                proposal_conflicts,
                arbitration_frequency,
                mean_total_ms: agg.mean_total_ms,
                mean_policy_eval_ms: agg.mean_policy_eval_ms,
                mean_safety_eval_ms: agg.mean_safety_eval_ms,
            });
        }

        let table = build_e3_table(&all_rows);
        write_csv_string(&self.tables_dir.join("e3-governance-table.csv"), &table);

        // Figure data — per-tick conflict/latency profile for Mixed workload
        let mut fig = String::from(
            "tick,proposals,incidents,vetoes,approved,denied,total_ms\n",
        );
        for s in &samples {
            fig.push_str(&format!(
                "{},{},{},{},{},{},{:.4}\n",
                s.tick, s.proposal_count, s.incident_count, s.veto_count,
                s.approved_count, s.denied_count, s.total_ms,
            ));
        }
        write_csv_string(&self.figures_dir.join("e3-multi-agent").join("governance-profile.csv"), &fig);

        println!();
        Ok(())
    }
}

#[derive(Serialize)]
struct E3Row {
    workload: String,
    sample_count: u64,
    total_proposals: u64,
    total_incidents: u64,
    total_vetoes: u64,
    total_approved: u64,
    total_denied: u64,
    proposal_conflicts: u64,
    arbitration_frequency: f64,
    mean_total_ms: f64,
    mean_policy_eval_ms: f64,
    mean_safety_eval_ms: f64,
}

fn build_e3_table(rows: &[E3Row]) -> String {
    let mut s = String::new();
    s.push_str("### E3: Multi-Agent Governance — Conflict & Arbitration Report\n\n");
    s.push_str("| Workload | Samples | Proposals | Incidents | Vetoes | Approved | Denied | Conflicts | Arb/Tick | Total Lat (ms) | Policy (ms) | Safety (ms) |\n");
    s.push_str("|----------|-------:|----------:|----------:|------:|--------:|------:|---------:|--------:|---------------:|------------:|------------:|\n");
    for r in rows {
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {:.2} | {:.4} | {:.4} | {:.4} |\n",
            r.workload, r.sample_count, r.total_proposals, r.total_incidents,
            r.total_vetoes, r.total_approved, r.total_denied,
            r.proposal_conflicts, r.arbitration_frequency,
            r.mean_total_ms, r.mean_policy_eval_ms, r.mean_safety_eval_ms,
        ));
    }
    s
}

// ═══════════════════════════════════════════════════════════════════
//  E4: Resource Governance
// ═══════════════════════════════════════════════════════════════════

impl ExperimentalCampaign {
    fn run_e4(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("── E4: Resource Governance ──");

        let mut all_rows: Vec<E4Row> = Vec::new();

        // Parameter sweep: target_pressure for CPU, target_memory_usage for Memory
        for (kind_label, param_name, values) in [
            ("cpu-contention", "target_pressure", vec![0.3f64, 0.5, 0.7, 0.85, 0.95]),
            ("memory-pressure", "target_memory_usage", vec![0.3f64, 0.5, 0.7, 0.85, 0.95]),
        ] {
            let kind = match kind_label {
                "cpu-contention" => WorkloadKind::CpuContention,
                "memory-pressure" => WorkloadKind::MemoryPressure,
                _ => unreachable!(),
            };

            let mut sweeps = HashMap::new();
            sweeps.insert(param_name.to_string(), values);

            let manifest = ExperimentManifest {
                name: format!("{kind_label}-sweep"),
                description: format!("{kind_label} governance sweep over {param_name}"),
                seeds: vec![SEED],
                tick_counts: vec![TICKS],
                workloads: vec![kind],
                sweeps,
            };

            let result = experiment::run_experiment(manifest);
            let csv_path = self.results_dir
                .join("e4-resource-governance")
                .join(format!("{kind_label}-sweep.csv"));
            experiment::write_experiment_csv(&result, csv_path.to_str().unwrap())?;

            let json_path = self.results_dir
                .join("e4-resource-governance")
                .join(format!("{kind_label}-sweep.json"));
            experiment::write_experiment_json(&result, json_path.to_str().unwrap())?;

            for run in &result.runs {
                let agg = &run.aggregate;
                let pressure = run.params.get(param_name).copied().unwrap_or(0.0);
                let proposal_rate = if TICKS > 0 {
                    agg.total_proposals as f64 / TICKS as f64
                } else {
                    0.0
                };
                let approval_rate = if agg.total_proposals > 0 {
                    agg.total_approved as f64 / agg.total_proposals as f64
                } else {
                    0.0
                };

                all_rows.push(E4Row {
                    workload: kind_label.into(),
                    param_name: param_name.into(),
                    param_value: pressure,
                    seed: run.seed,
                    mean_total_ms: agg.mean_total_ms,
                    total_proposals: agg.total_proposals,
                    total_approved: agg.total_approved,
                    total_denied: agg.total_denied,
                    proposal_rate,
                    approval_rate,
                    mean_latency_ms: agg.mean_total_ms,
                });
            }
        }

        let table = build_e4_table(&all_rows);
        write_csv_string(&self.tables_dir.join("e4-resource-table.csv"), &table);

        // Figure data — throughput vs pressure
        let mut fig = String::from(
            "workload,pressure,proposal_rate,approval_rate,mean_latency_ms\n",
        );
        for r in &all_rows {
            fig.push_str(&format!(
                "{},{},{:.4},{:.4},{:.4}\n",
                r.workload, r.param_value, r.proposal_rate, r.approval_rate, r.mean_latency_ms,
            ));
        }
        write_csv_string(&self.figures_dir.join("e4-resource-governance").join("resource-throughput.csv"), &fig);

        println!();
        Ok(())
    }
}

#[derive(Serialize)]
struct E4Row {
    workload: String,
    param_name: String,
    param_value: f64,
    seed: u64,
    mean_total_ms: f64,
    total_proposals: u64,
    total_approved: u64,
    total_denied: u64,
    proposal_rate: f64,
    approval_rate: f64,
    mean_latency_ms: f64,
}

fn build_e4_table(rows: &[E4Row]) -> String {
    let mut s = String::new();
    s.push_str("### E4: Resource Governance — Throughput & Latency Report\n\n");
    s.push_str("| Workload | Param | Value | Proposals | Approved | Denied | Proposal/Tick | Approval Rate | Mean Latency (ms) |\n");
    s.push_str("|----------|-------|-----:|----------:|--------:|------:|-------------:|--------------:|------------------:|\n");
    for r in rows {
        s.push_str(&format!(
            "| {} | {} | {:.2} | {} | {} | {} | {:.4} | {:.4} | {:.4} |\n",
            r.workload, r.param_name, r.param_value,
            r.total_proposals, r.total_approved, r.total_denied,
            r.proposal_rate, r.approval_rate, r.mean_latency_ms,
        ));
    }
    s
}

// ═══════════════════════════════════════════════════════════════════
//  E5: Governance Overhead
// ═══════════════════════════════════════════════════════════════════

impl ExperimentalCampaign {
    fn run_e5(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("── E5: Governance Overhead ──");

        let mut all_rows: Vec<E5Row> = Vec::new();

        // Compare latency profiles across all 3 modes for each workload
        for kind in &ALL_WORKLOADS {
            let name = kind.name();
            println!("  {name}...");

            // Agents+Policy (full pipeline via run_workload)
            let (_samples, agg_agents_policy) = run_workload_default(kind.clone());

            // Policy-only: run comparative for this mode's timing
            let mut generator = agenticos_bench::workload::WorkloadGenerator::new(WorkloadConfig {
                kind: kind.clone(),
                seed: SEED,
                tick_count: TICKS,
                ..WorkloadConfig::default()
            });

            // Policy-only timing
            let policy_kernel = agenticos_policy::DefaultPolicyKernel::benchmark();
            let mut policy_times: Vec<f64> = Vec::new();
            for _ in 0..TICKS {
                let obs = generator.next_tick();
                let input = agenticos_policy::PolicyInput {
                    tick: generator.tick(),
                    observations: obs.clone(),
                    proposals: vec![],
                    incidents: vec![],
                    prior_decisions: vec![],
                    metrics: agenticos_domain::MetricCollection {
                        source: "e5".into(),
                        samples: vec![],
                    },
                };
                let t0 = Instant::now();
                let _ = policy_kernel.evaluate_tick(&input).unwrap();
                policy_times.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
            let mean_policy_latency = if !policy_times.is_empty() {
                policy_times.iter().sum::<f64>() / policy_times.len() as f64
            } else {
                0.0
            };

            // Full pipeline timing (from run_workload aggregate)
            let mean_full_latency = agg_agents_policy.mean_total_ms;

            // Agents+Policy (no safety) — derive from full by subtracting safety
            let mean_agents_policy_latency = mean_full_latency - agg_agents_policy.mean_safety_eval_ms;

            // Full (from agents+policy aggregate, already includes safety)
            let (_samples_full, agg_full) = run_workload_default(kind.clone());

            all_rows.push(E5Row {
                workload: name.into(),
                mean_policy_only_ms: mean_policy_latency,
                mean_agents_policy_ms: mean_agents_policy_latency,
                mean_full_ms: mean_full_latency,
                policy_share: if mean_full_latency > 0.0 {
                    agg_full.mean_policy_eval_ms / mean_full_latency
                } else {
                    0.0
                },
                safety_share: if mean_full_latency > 0.0 {
                    agg_full.mean_safety_eval_ms / mean_full_latency
                } else {
                    0.0
                },
                executor_share: if mean_full_latency > 0.0 {
                    agg_full.mean_executor_ms / mean_full_latency
                } else {
                    0.0
                },
                proposal_throughput: if TICKS > 0 {
                    agg_full.total_proposals as f64 / TICKS as f64
                } else {
                    0.0
                },
                incident_throughput: if TICKS > 0 {
                    agg_full.total_incidents as f64 / TICKS as f64
                } else {
                    0.0
                },
                total_proposals: agg_full.total_proposals,
                total_incidents: agg_full.total_incidents,
            });
        }

        // Per-workload results CSV
        let csv_path = self.results_dir
            .join("e5-governance-overhead")
            .join("latency-comparison.csv");
        let mut csv = String::from(
            "workload,mean_policy_only_ms,mean_agents_policy_ms,mean_full_ms,policy_share,safety_share,executor_share,proposals_per_tick,incidents_per_tick\n",
        );
        for r in &all_rows {
            csv.push_str(&format!(
                "{},{:.6},{:.6},{:.6},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
                r.workload,
                r.mean_policy_only_ms,
                r.mean_agents_policy_ms,
                r.mean_full_ms,
                r.policy_share,
                r.safety_share,
                r.executor_share,
                r.proposal_throughput,
                r.incident_throughput,
            ));
        }
        write_csv_string(&csv_path, &csv);

        let json_path = self.results_dir
            .join("e5-governance-overhead")
            .join("latency-comparison.json");
        write_json(&json_path, &all_rows);

        // Summary table
        let table = build_e5_table(&all_rows);
        write_csv_string(&self.tables_dir.join("e5-overhead-table.csv"), &table);

        // Figure data — latency by mode
        let mut fig = String::from(
            "workload,policy_only_ms,agents_policy_ms,full_ms\n",
        );
        for r in &all_rows {
            fig.push_str(&format!(
                "{},{:.6},{:.6},{:.6}\n",
                r.workload, r.mean_policy_only_ms, r.mean_agents_policy_ms, r.mean_full_ms,
            ));
        }
        write_csv_string(&self.figures_dir.join("e5-governance-overhead").join("latency-comparison.csv"), &fig);

        println!();
        Ok(())
    }
}

#[derive(Serialize)]
struct E5Row {
    workload: String,
    mean_policy_only_ms: f64,
    mean_agents_policy_ms: f64,
    mean_full_ms: f64,
    policy_share: f64,
    safety_share: f64,
    executor_share: f64,
    proposal_throughput: f64,
    incident_throughput: f64,
    total_proposals: u64,
    total_incidents: u64,
}

fn build_e5_table(rows: &[E5Row]) -> String {
    let mut s = String::new();
    s.push_str("### E5: Governance Overhead — Latency Breakdown\n\n");
    s.push_str("| Workload | Policy-Only (ms) | Agents+Policy (ms) | Full (ms) | Policy Share | Safety Share | Executor Share | Proposals/Tick | Incidents/Tick |\n");
    s.push_str("|----------|----------------:|-------------------:|----------:|------------:|-------------:|---------------:|--------------:|---------------:|\n");
    for r in rows {
        s.push_str(&format!(
            "| {} | {:.6} | {:.6} | {:.6} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} |\n",
            r.workload,
            r.mean_policy_only_ms,
            r.mean_agents_policy_ms,
            r.mean_full_ms,
            r.policy_share,
            r.safety_share,
            r.executor_share,
            r.proposal_throughput,
            r.incident_throughput,
        ));
    }
    s
}
