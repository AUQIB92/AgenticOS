use std::time::Instant;

use agenticos_agents::{MemoryAgent, ProcessAgent, SecurityAgent};
use agenticos_domain::{AgentId, Decision, DecisionOutcome};
use agenticos_executor::{ApprovedActionExecutor, DryRunExecutor};
use agenticos_policy::{DefaultPolicyKernel, DeterministicPolicyKernel, PolicyInput};
use agenticos_runtime::{AgentRuntime, InMemoryAgentRuntime};
use agenticos_safety::{DefaultSafetyGovernor, SafetyConfig, SafetyInput};
use serde::Serialize;

use crate::workload::{WorkloadConfig, WorkloadGenerator};

/// Per-tick benchmark measurement.
#[derive(Clone, Debug, Serialize)]
pub struct TickSample {
    pub workload: String,
    pub seed: u64,
    pub tick: u64,
    /// Total observations for this tick.
    pub obs_count: u64,
    /// Time to collect proposals (ms).
    pub collect_proposals_ms: f64,
    /// Time for policy evaluation (ms).
    pub policy_eval_ms: f64,
    /// Time for safety governor (ms).
    pub safety_eval_ms: f64,
    /// Time for executor (ms).
    pub executor_ms: f64,
    /// Total tick wall-clock (ms).
    pub total_ms: f64,
    /// Proposals emitted this tick.
    pub proposal_count: u64,
    /// Incidents emitted this tick.
    pub incident_count: u64,
    /// Veto decisions issued by safety governor.
    pub veto_count: u64,
    /// Decisions approved by policy + safety.
    pub approved_count: u64,
    /// Decisions denied by policy.
    pub denied_count: u64,
    /// Number of executed actions.
    pub executor_count: u64,
}

/// Aggregated benchmark result across multiple ticks.
#[derive(Clone, Debug, Serialize)]
pub struct AggregateResult {
    pub workload: String,
    pub seed: u64,
    pub ticks: u64,
    pub mean_total_ms: f64,
    pub p50_total_ms: f64,
    pub p95_total_ms: f64,
    pub p99_total_ms: f64,
    pub mean_collect_proposals_ms: f64,
    pub mean_policy_eval_ms: f64,
    pub mean_safety_eval_ms: f64,
    pub mean_executor_ms: f64,
    pub total_proposals: u64,
    pub total_incidents: u64,
    pub total_vetoes: u64,
    pub total_approved: u64,
    pub total_denied: u64,
    pub total_executions: u64,
}

/// Runs a single workload and collects per-tick measurements.
pub fn run_workload(config: WorkloadConfig) -> (Vec<TickSample>, AggregateResult) {
    let kind = config.kind.clone();
    let seed = config.seed;
    let tick_count = config.tick_count;
    let mut generator = WorkloadGenerator::new(config);

    // ── Setup pipeline components ────────────────────────────────────
    let runtime = InMemoryAgentRuntime::new();
    let policy_kernel = DefaultPolicyKernel::benchmark();
    let safety_governor = DefaultSafetyGovernor::new(SafetyConfig {
        max_cpu_weight: 10000,
        max_memory_bytes: Some(64u64 * 1024 * 1024 * 1024),
        veto_on_security_incidents: true,
    });
    let executor = DryRunExecutor::new();

    // Register agents based on workload
    match &kind {
        crate::workload::WorkloadKind::CpuContention => {
            runtime
                .register(Box::new(ProcessAgent::new(AgentId::from("process-agent"))))
                .unwrap();
            runtime.start(AgentId::from("process-agent")).unwrap();
        }
        crate::workload::WorkloadKind::MemoryPressure => {
            runtime
                .register(Box::new(MemoryAgent::new(AgentId::from("memory-agent"))))
                .unwrap();
            runtime.start(AgentId::from("memory-agent")).unwrap();
        }
        crate::workload::WorkloadKind::Mixed => {
            runtime
                .register(Box::new(ProcessAgent::new(AgentId::from("process-agent"))))
                .unwrap();
            runtime
                .register(Box::new(MemoryAgent::new(AgentId::from("memory-agent"))))
                .unwrap();
            runtime.start(AgentId::from("process-agent")).unwrap();
            runtime.start(AgentId::from("memory-agent")).unwrap();
        }
        crate::workload::WorkloadKind::ProcessExplosion
        | crate::workload::WorkloadKind::IncidentStorm => {
            // Security Agent only (advisory — emits incidents, not proposals)
            runtime
                .register(Box::new(SecurityAgent::new(AgentId::from("security-agent"))))
                .unwrap();
            runtime
                .start(AgentId::from("security-agent"))
                .unwrap();
        }
    }

    let mut samples: Vec<TickSample> = Vec::with_capacity(tick_count as usize);

    for _ in 0..tick_count {
        let tick_start = Instant::now();

        // 1. Generate observations
        let observations = generator.next_tick();

        // 2. Collect proposals
        let t0 = Instant::now();
        let proposals = runtime.collect_proposals(&observations).unwrap();
        let collect_proposals_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // 3. Collect incidents
        let incidents = runtime.collect_incidents(&observations).unwrap();

        // 4. Build PolicyInput
        let policy_input = PolicyInput {
            tick: generator.tick(),
            observations: observations.clone(),
            proposals: proposals.clone(),
            incidents: incidents.clone(),
            prior_decisions: vec![],
            metrics: agenticos_domain::MetricCollection {
                source: "bench".into(),
                samples: vec![],
            },
        };

        // 5. Policy evaluation
        let t1 = Instant::now();
        let decisions: Vec<Decision> = policy_kernel.evaluate_tick(&policy_input).unwrap();
        let policy_eval_ms = t1.elapsed().as_secs_f64() * 1000.0;

        // 6. Safety Governor
        let t2 = Instant::now();
        let safety_input = SafetyInput {
            policy_input: &policy_input,
            decisions: &decisions,
        };
        let safety_output = safety_governor.evaluate(safety_input).unwrap();
        let safety_eval_ms = t2.elapsed().as_secs_f64() * 1000.0;

        // 7. Execute approved actions
        let t3 = Instant::now();
        let safe_ids: std::collections::HashSet<_> = safety_output
            .approved
            .iter()
            .map(|d| d.proposal_id.clone())
            .collect();
        let mut executor_count = 0u64;
        for (prop, decision) in proposals.iter().zip(decisions.iter()) {
            if !safe_ids.contains(&decision.proposal_id) {
                continue;
            }
            if matches!(decision.outcome, DecisionOutcome::Approved) {
                let approved = agenticos_domain::ApprovedAction {
                    request: prop.requested_action.clone(),
                    decision_id: decision.id.clone(),
                };
                let _ = executor.execute(approved);
                executor_count += 1;
            }
        }
        let executor_ms = t3.elapsed().as_secs_f64() * 1000.0;

        let total_ms = tick_start.elapsed().as_secs_f64() * 1000.0;

        let approved_count = safety_output
            .approved
            .iter()
            .filter(|d| matches!(d.outcome, DecisionOutcome::Approved))
            .count() as u64;

        let denied_count = decisions
            .iter()
            .filter(|d| matches!(d.outcome, DecisionOutcome::Denied { .. }))
            .count() as u64;

        samples.push(TickSample {
            workload: kind.name().into(),
            seed,
            tick: generator.tick(),
            obs_count: observations.len() as u64,
            collect_proposals_ms,
            policy_eval_ms,
            safety_eval_ms,
            executor_ms,
            total_ms,
            proposal_count: proposals.len() as u64,
            incident_count: incidents.len() as u64,
            veto_count: safety_output.vetoes.len() as u64,
            approved_count,
            denied_count,
            executor_count,
        });
    }

    let aggregate = aggregate(&samples, kind.name(), seed);

    (samples, aggregate)
}

/// Compute aggregate statistics from tick samples.
pub fn aggregate(samples: &[TickSample], workload: &str, seed: u64) -> AggregateResult {
    let ticks = samples.len() as u64;
    if ticks == 0 {
        return AggregateResult {
            workload: workload.into(),
            seed,
            ticks: 0,
            mean_total_ms: 0.0,
            p50_total_ms: 0.0,
            p95_total_ms: 0.0,
            p99_total_ms: 0.0,
            mean_collect_proposals_ms: 0.0,
            mean_policy_eval_ms: 0.0,
            mean_safety_eval_ms: 0.0,
            mean_executor_ms: 0.0,
            total_proposals: 0,
            total_incidents: 0,
            total_vetoes: 0,
            total_approved: 0,
            total_denied: 0,
            total_executions: 0,
        };
    }

    let mut totals: Vec<f64> = samples.iter().map(|s| s.total_ms).collect();
    totals.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let p50 = percentile(&totals, 50.0);
    let p95 = percentile(&totals, 95.0);
    let p99 = percentile(&totals, 99.0);

    AggregateResult {
        workload: workload.into(),
        seed,
        ticks,
        mean_total_ms: totals.iter().sum::<f64>() / ticks as f64,
        p50_total_ms: p50,
        p95_total_ms: p95,
        p99_total_ms: p99,
        mean_collect_proposals_ms: mean(samples, |s| s.collect_proposals_ms),
        mean_policy_eval_ms: mean(samples, |s| s.policy_eval_ms),
        mean_safety_eval_ms: mean(samples, |s| s.safety_eval_ms),
        mean_executor_ms: mean(samples, |s| s.executor_ms),
        total_proposals: samples.iter().map(|s| s.proposal_count).sum(),
        total_incidents: samples.iter().map(|s| s.incident_count).sum(),
        total_vetoes: samples.iter().map(|s| s.veto_count).sum(),
        total_approved: samples.iter().map(|s| s.approved_count).sum(),
        total_denied: samples.iter().map(|s| s.denied_count).sum(),
        total_executions: samples.iter().map(|s| s.executor_count).sum(),
    }
}

fn mean<F>(samples: &[TickSample], extract: F) -> f64
where
    F: Fn(&TickSample) -> f64,
{
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().map(|s| extract(s)).sum::<f64>() / samples.len() as f64
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ── CSV / JSON export ──────────────────────────────────────────────

/// Write per-tick samples to a CSV file.
pub fn write_samples_csv(samples: &[TickSample], path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut wtr = csv::Writer::from_path(path)?;
    // Header
    wtr.write_record(&[
        "workload", "seed", "tick", "obs_count", "collect_proposals_ms", "policy_eval_ms",
        "safety_eval_ms", "executor_ms", "total_ms", "proposal_count", "incident_count",
        "veto_count", "approved_count", "denied_count", "executor_count",
    ])?;
    for s in samples {
        wtr.write_record(&[
            &s.workload,
            &s.seed.to_string(),
            &s.tick.to_string(),
            &s.obs_count.to_string(),
            &format!("{:.4}", s.collect_proposals_ms),
            &format!("{:.4}", s.policy_eval_ms),
            &format!("{:.4}", s.safety_eval_ms),
            &format!("{:.4}", s.executor_ms),
            &format!("{:.4}", s.total_ms),
            &s.proposal_count.to_string(),
            &s.incident_count.to_string(),
            &s.veto_count.to_string(),
            &s.approved_count.to_string(),
            &s.denied_count.to_string(),
            &s.executor_count.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

/// Write per-tick samples to a JSON file.
pub fn write_samples_json(samples: &[TickSample], path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(samples)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Write aggregate results to a JSON file.
pub fn write_aggregate_json(agg: &AggregateResult, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(agg)?;
    std::fs::write(path, json)?;
    Ok(())
}

// ── Convenience runner ─────────────────────────────────────────────

/// Run all 5 workloads with default configs and return samples + aggregates.
pub fn run_all_workloads(
    ticks: u64,
    seed: u64,
) -> Vec<(Vec<TickSample>, AggregateResult)> {
    use crate::workload::WorkloadKind;
    let kinds = vec![
        WorkloadKind::CpuContention,
        WorkloadKind::MemoryPressure,
        WorkloadKind::Mixed,
        WorkloadKind::ProcessExplosion,
        WorkloadKind::IncidentStorm,
    ];

    kinds
        .into_iter()
        .map(|kind| {
            let config = WorkloadConfig {
                kind,
                seed,
                tick_count: ticks,
                ..WorkloadConfig::default()
            };
            run_workload(config)
        })
        .collect()
}
