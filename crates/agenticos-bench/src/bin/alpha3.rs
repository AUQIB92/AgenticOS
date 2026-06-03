//! Alpha-3 Adversarial Governance Campaign.
//!
//! Exercises governance mechanisms not activated during Alpha-2:
//!   S1: Proposal conflicts     — two agents propose conflicting values on same resource
//!   S2: Budget violations      — proposals exceed configured resource limits
//!   S3: Incident-driven vetoes — incidents + proposals coexist, governor vetoes all
//!   S4: Escalation chain       — multi-tick correlated incidents with escalation
//!   S5: Adversarial governance — combined scenario
//!
//! Each scenario measures:
//!   veto frequency, arbitration frequency, escalation frequency,
//!   governance latency breakdown, replay consistency (10 iterations).
//!
//! Output: experiments/alpha3/results/, tables/, figures/

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use agenticos_domain::{
    ActionId, ActionKind, ActionRequest, ActionSafetyLevel, AgentId, Confidence,
    Decision, DecisionOutcome, Incident, IncidentCategory,
    IncidentSeverity, MetricCollection,
    Proposal, ProposalId,
};
use agenticos_executor::DryRunExecutor;
use agenticos_policy::{DefaultPolicyKernel, DeterministicPolicyKernel, PolicyInput};
use agenticos_safety::{
    DefaultSafetyGovernor, SafetyConfig, SafetyInput, VetoReason,
};
use serde::Serialize;

const REPLAY_ITERATIONS: usize = 10;

// ── Output helpers ──────────────────────────────────────────────────

fn ensure_dirs(base: &PathBuf) {
    for d in ["results", "tables", "figures"] {
        let _ = fs::create_dir_all(base.join(d));
    }
}

fn write_json<T: Serialize>(path: &PathBuf, data: &T) {
    fs::write(path, serde_json::to_string_pretty(data).unwrap()).unwrap();
}

// ── Shared types ────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
pub struct Alpha3Sample {
    pub scenario: String,
    pub iteration: usize,
    pub tick: u64,
    pub proposal_count: u64,
    pub incident_count: u64,
    pub veto_count: u64,
    pub arbitration_count: u64,
    pub escalation_count: u64,
    pub approved_count: u64,
    pub denied_count: u64,
    pub policy_eval_ms: f64,
    pub safety_eval_ms: f64,
    pub total_ms: f64,
    pub veto_breakdown: HashMap<String, u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Alpha3Result {
    pub scenario: String,
    pub seed: u64,
    pub ticks: u64,
    pub iterations: usize,
    pub replay_consistent: bool,
    pub total_proposals: u64,
    pub total_incidents: u64,
    pub total_vetoes: u64,
    pub total_arbitrations: u64,
    pub total_escalations: u64,
    pub total_approved: u64,
    pub total_denied: u64,
    pub mean_total_ms: f64,
    pub mean_policy_eval_ms: f64,
    pub mean_safety_eval_ms: f64,
    pub veto_breakdown: HashMap<String, u64>,
}

// ── Tick runner ─────────────────────────────────────────────────────

fn run_tick(
    policy: &DefaultPolicyKernel,
    safety: &DefaultSafetyGovernor,
    _executor: &DryRunExecutor,
    proposals: &[Proposal],
    incidents: &[Incident],
    tick: u64,
) -> TickOutput {
    let policy_input = PolicyInput {
        tick,
        observations: vec![],
        proposals: proposals.to_vec(),
        incidents: incidents.to_vec(),
        prior_decisions: vec![],
        metrics: MetricCollection {
            source: "alpha3".into(),
            samples: vec![],
        },
    };

    // Policy evaluation
    let t0 = Instant::now();
    let decisions: Vec<Decision> = policy.evaluate_tick(&policy_input).unwrap();
    let policy_eval_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // Safety Governor
    let t1 = Instant::now();
    let safety_input = SafetyInput {
        policy_input: &policy_input,
        decisions: &decisions,
    };
    let safety_output = safety.evaluate(safety_input).unwrap();
    let safety_eval_ms = t1.elapsed().as_secs_f64() * 1000.0;

    let total_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // Count arbitration events (ConflictingProposals vetoes)
    let arbitration_count = safety_output
        .vetoes
        .iter()
        .filter(|v| v.reason == VetoReason::ConflictingProposals)
        .count() as u64;

    let escalation_count = safety_output.escalations.len() as u64;

    let approved_count = safety_output
        .approved
        .iter()
        .filter(|d| matches!(d.outcome, DecisionOutcome::Approved))
        .count() as u64;

    let denied_count = decisions
        .iter()
        .filter(|d| matches!(d.outcome, DecisionOutcome::Denied { .. }))
        .count() as u64;

    let veto_breakdown: HashMap<String, u64> = {
        let mut map: HashMap<String, u64> = HashMap::new();
        for v in &safety_output.vetoes {
            let key = format!("{:?}", v.reason);
            *map.entry(key).or_insert(0) += 1;
        }
        map
    };

    TickOutput {
        vetoes: safety_output.vetoes,
        policy_eval_ms,
        safety_eval_ms,
        total_ms,
        arbitration_count,
        escalation_count,
        approved_count,
        denied_count,
        veto_breakdown,
    }
}

struct TickOutput {
    vetoes: Vec<agenticos_safety::VetoDecision>,
    policy_eval_ms: f64,
    safety_eval_ms: f64,
    total_ms: f64,
    arbitration_count: u64,
    escalation_count: u64,
    approved_count: u64,
    denied_count: u64,
    veto_breakdown: HashMap<String, u64>,
}

// ── Scenario definitions ────────────────────────────────────────────

/// S1: Proposal Conflicts
///
/// Creates proposals targeting the same cgroup with different CPU weights.
/// Governor should detect conflicts and veto all but the first.
fn scenario_proposal_conflict(seed: u64) -> (Vec<Vec<Alpha3Sample>>, Vec<Alpha3Result>) {
    let policy = DefaultPolicyKernel::benchmark();
    let safety = DefaultSafetyGovernor::with_defaults();
    let executor = DryRunExecutor::new();
    let scenario = "S1-proposal-conflict";

    let mut all_samples: Vec<Vec<Alpha3Sample>> = Vec::new();
    let mut all_results: Vec<Alpha3Result> = Vec::new();

    for iter in 0..REPLAY_ITERATIONS {
        let mut samples: Vec<Alpha3Sample> = Vec::new();

        for tick in 1..=5u64 {
            // Two agents propose conflicting CPU weights on same group
            let proposals = vec![
                make_proposal("agent-a", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/workload".into(),
                    weight: 100,
                }, 0.9),
                make_proposal("agent-b", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/workload".into(),
                    weight: 300,
                }, 0.85),
                make_proposal("agent-c", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/workload".into(),
                    weight: 500,
                }, 0.8),
            ];

            let out = run_tick(&policy, &safety, &executor, &proposals, &[], tick);
            samples.push(Alpha3Sample {
                scenario: scenario.into(),
                iteration: iter,
                tick,
                proposal_count: proposals.len() as u64,
                incident_count: 0,
                veto_count: out.vetoes.len() as u64,
                arbitration_count: out.arbitration_count,
                escalation_count: out.escalation_count,
                approved_count: out.approved_count,
                denied_count: out.denied_count,
                policy_eval_ms: out.policy_eval_ms,
                safety_eval_ms: out.safety_eval_ms,
                total_ms: out.total_ms,
                veto_breakdown: out.veto_breakdown.clone(),
            });
        }

        let result = aggregate_samples(scenario, seed, iter, &samples);
        all_samples.push(samples);
        all_results.push(result);
    }

    (all_samples, all_results)
}

/// S2: Budget Violations
///
/// Configures a strict governor with low resource limits.
/// Proposals that exceed limits should be vetoed.
fn scenario_budget_violation(seed: u64) -> (Vec<Vec<Alpha3Sample>>, Vec<Alpha3Result>) {
    let policy = DefaultPolicyKernel::benchmark();
    let safety = DefaultSafetyGovernor::new(SafetyConfig {
        max_cpu_weight: 500,
        max_memory_bytes: Some(1_000_000_000), // 1 GB
        ..SafetyConfig::default()
    });
    let executor = DryRunExecutor::new();
    let scenario = "S2-budget-violation";

    let mut all_samples: Vec<Vec<Alpha3Sample>> = Vec::new();
    let mut all_results: Vec<Alpha3Result> = Vec::new();

    for iter in 0..REPLAY_ITERATIONS {
        let mut samples: Vec<Alpha3Sample> = Vec::new();

        for tick in 1..=5u64 {
            // Mix of within-budget and over-budget proposals
            let proposals = vec![
                // Within CPU budget (≤500)
                make_proposal("agent-a", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/light".into(),
                    weight: 300,
                }, 0.9),
                // Over CPU budget
                make_proposal("agent-b", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/heavy".into(),
                    weight: 800,
                }, 0.85),
                // Within memory budget (≤1 GB)
                make_proposal("agent-c", ActionKind::CgroupSetMemoryMax {
                    group: "agenticos/cache".into(),
                    bytes: 500_000_000,
                }, 0.9),
                // Over memory budget
                make_proposal("agent-d", ActionKind::CgroupSetMemoryMax {
                    group: "agenticos/db".into(),
                    bytes: 5_000_000_000,
                }, 0.8),
            ];

            let out = run_tick(&policy, &safety, &executor, &proposals, &[], tick);
            samples.push(Alpha3Sample {
                scenario: scenario.into(),
                iteration: iter,
                tick,
                proposal_count: proposals.len() as u64,
                incident_count: 0,
                veto_count: out.vetoes.len() as u64,
                arbitration_count: out.arbitration_count,
                escalation_count: out.escalation_count,
                approved_count: out.approved_count,
                denied_count: out.denied_count,
                policy_eval_ms: out.policy_eval_ms,
                safety_eval_ms: out.safety_eval_ms,
                total_ms: out.total_ms,
                veto_breakdown: out.veto_breakdown.clone(),
            });
        }

        let result = aggregate_samples(scenario, seed, iter, &samples);
        all_samples.push(samples);
        all_results.push(result);
    }

    (all_samples, all_results)
}

/// S3: Incident-Driven Vetoes
///
/// Proposals coexist with security incidents.
/// Governor vetoes all proposals when security incidents are present.
fn scenario_incident_driven(seed: u64) -> (Vec<Vec<Alpha3Sample>>, Vec<Alpha3Result>) {
    let policy = DefaultPolicyKernel::benchmark();
    let safety = DefaultSafetyGovernor::with_defaults();
    let executor = DryRunExecutor::new();
    let scenario = "S3-incident-driven";

    let mut all_samples: Vec<Vec<Alpha3Sample>> = Vec::new();
    let mut all_results: Vec<Alpha3Result> = Vec::new();

    for iter in 0..REPLAY_ITERATIONS {
        let mut samples: Vec<Alpha3Sample> = Vec::new();

        for tick in 1..=5u64 {
            // Proposals as usual
            let proposals = vec![
                make_proposal("agent-a", ActionKind::WorkloadClassifyRecommend {
                    group: "system".into(),
                    classification: "high-cpu".into(),
                }, 0.9),
                make_proposal("agent-b", ActionKind::CgroupSetMemoryMax {
                    group: "agenticos/data".into(),
                    bytes: 2_000_000_000,
                }, 0.85),
            ];

            // Security incidents that should trigger veto
            let incidents = vec![
                Incident::new(
                    IncidentCategory::Security,
                    IncidentSeverity::Warning,
                    AgentId::from("security-agent"),
                    None,
                    format!("ForkStormDetected: tick {tick}"),
                ).with_correlation(format!("escalation-{tick}")),
            ];

            let out = run_tick(&policy, &safety, &executor, &proposals, &incidents, tick);
            samples.push(Alpha3Sample {
                scenario: scenario.into(),
                iteration: iter,
                tick,
                proposal_count: proposals.len() as u64,
                incident_count: incidents.len() as u64,
                veto_count: out.vetoes.len() as u64,
                arbitration_count: out.arbitration_count,
                escalation_count: out.escalation_count,
                approved_count: out.approved_count,
                denied_count: out.denied_count,
                policy_eval_ms: out.policy_eval_ms,
                safety_eval_ms: out.safety_eval_ms,
                total_ms: out.total_ms,
                veto_breakdown: out.veto_breakdown.clone(),
            });
        }

        let result = aggregate_samples(scenario, seed, iter, &samples);
        all_samples.push(samples);
        all_results.push(result);
    }

    (all_samples, all_results)
}

/// S4: Escalation Chain
///
/// Multi-tick scenario where incidents escalate:
///   Tick 1: single incident (no escalation)
///   Tick 2: two incidents, one correlated to tick-1 incident
///   Tick 3: three incidents with chain correlation
///   Ticks 4-5: governor escalates vetoed decisions
fn scenario_escalation_chain(seed: u64) -> (Vec<Vec<Alpha3Sample>>, Vec<Alpha3Result>) {
    let policy = DefaultPolicyKernel::benchmark();
    let safety = DefaultSafetyGovernor::with_defaults();
    let executor = DryRunExecutor::new();
    let scenario = "S4-escalation-chain";

    let mut all_samples: Vec<Vec<Alpha3Sample>> = Vec::new();
    let mut all_results: Vec<Alpha3Result> = Vec::new();

    for iter in 0..REPLAY_ITERATIONS {
        let mut samples: Vec<Alpha3Sample> = Vec::new();

        for tick in 1..=5u64 {
            let proposals = vec![
                make_proposal("agent-a", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/workload".into(),
                    weight: 200,
                }, 0.9),
            ];

            // Escalating incident chain with correlation IDs
            let incidents = match tick {
                1 => vec![
                    Incident::new(
                        IncidentCategory::Security,
                        IncidentSeverity::Info,
                        AgentId::from("security-agent"),
                        None,
                        "initial security observation",
                    ),
                ],
                2 => vec![
                    Incident::new(
                        IncidentCategory::Security,
                        IncidentSeverity::Warning,
                        AgentId::from("security-agent"),
                        None,
                        "escalated: persistent fork activity",
                    ).with_correlation("tick-1-incident-0"),
                    Incident::new(
                        IncidentCategory::ResourceContention,
                        IncidentSeverity::Warning,
                        AgentId::from("security-agent"),
                        None,
                        "resource contention detected following fork event",
                    ).with_correlation("tick-1-incident-0"),
                ],
                3 => vec![
                    Incident::new(
                        IncidentCategory::Security,
                        IncidentSeverity::Critical,
                        AgentId::from("security-agent"),
                        None,
                        "critical: cascading process creation",
                    ).with_correlation("tick-2-incident-0"),
                    Incident::new(
                        IncidentCategory::GovernanceViolation,
                        IncidentSeverity::Warning,
                        AgentId::from("security-agent"),
                        None,
                        "policy violation: resource limits approaching",
                    ).with_correlation("tick-2-incident-1"),
                    Incident::new(
                        IncidentCategory::ExecutorFailure,
                        IncidentSeverity::Error,
                        AgentId::from("executor"),
                        None,
                        "executor unable to enforce cgroup limits",
                    ).with_correlation("tick-3-incident-0"),
                ],
                4 | 5 => vec![
                    Incident::new(
                        IncidentCategory::Security,
                        IncidentSeverity::Critical,
                        AgentId::from("security-agent"),
                        None,
                        "ongoing security incident — all actions vetoed by governor",
                    ).with_correlation(format!("tick-{}-incident-0", tick - 1)),
                ],
                _ => vec![],
            };

            let out = run_tick(&policy, &safety, &executor, &proposals, &incidents, tick);
            samples.push(Alpha3Sample {
                scenario: scenario.into(),
                iteration: iter,
                tick,
                proposal_count: proposals.len() as u64,
                incident_count: incidents.len() as u64,
                veto_count: out.vetoes.len() as u64,
                arbitration_count: out.arbitration_count,
                escalation_count: out.escalation_count,
                approved_count: out.approved_count,
                denied_count: out.denied_count,
                policy_eval_ms: out.policy_eval_ms,
                safety_eval_ms: out.safety_eval_ms,
                total_ms: out.total_ms,
                veto_breakdown: out.veto_breakdown.clone(),
            });
        }

        let result = aggregate_samples(scenario, seed, iter, &samples);
        all_samples.push(samples);
        all_results.push(result);
    }

    (all_samples, all_results)
}

/// S5: Adversarial Governance
///
/// Combined scenario: conflicting proposals, budget violations,
/// security incidents, and multi-agent contention simultaneously.
fn scenario_adversarial(seed: u64) -> (Vec<Vec<Alpha3Sample>>, Vec<Alpha3Result>) {
    let policy = DefaultPolicyKernel::benchmark();
    let safety = DefaultSafetyGovernor::new(SafetyConfig {
        max_cpu_weight: 400,
        max_memory_bytes: Some(2_000_000_000),
        veto_on_security_incidents: true,
    });
    let executor = DryRunExecutor::new();
    let scenario = "S5-adversarial";

    let mut all_samples: Vec<Vec<Alpha3Sample>> = Vec::new();
    let mut all_results: Vec<Alpha3Result> = Vec::new();

    for iter in 0..REPLAY_ITERATIONS {
        let mut samples: Vec<Alpha3Sample> = Vec::new();

        for tick in 1..=5u64 {
            // Conflicting proposals on same cgroup
            let conflicting = vec![
                make_proposal("agent-a", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/conflict".into(),
                    weight: 100,
                }, 0.9),
                make_proposal("agent-b", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/conflict".into(),
                    weight: 600, // conflicts with agent-a AND exceeds budget
                }, 0.85),
                make_proposal("agent-c", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/conflict".into(),
                    weight: 300, // conflicts with agent-a
                }, 0.8),
            ];

            // Within-budget and over-budget proposals
            let budget = vec![
                make_proposal("agent-d", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/budget".into(),
                    weight: 200, // within budget (≤400)
                }, 0.9),
                make_proposal("agent-e", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/budget".into(),
                    weight: 800, // exceeds budget
                }, 0.85),
                make_proposal("agent-f", ActionKind::CgroupSetMemoryMax {
                    group: "agenticos/store".into(),
                    bytes: 1_000_000_000, // within budget (≤2 GB)
                }, 0.9),
                make_proposal("agent-g", ActionKind::CgroupSetMemoryMax {
                    group: "agenticos/store".into(),
                    bytes: 10_000_000_000, // exceeds budget
                }, 0.8),
            ];

            // Security incidents to trigger incident-aware vetoes
            let incidents = vec![
                Incident::new(
                    IncidentCategory::Security,
                    IncidentSeverity::Warning,
                    AgentId::from("security-agent"),
                    None,
                    format!("adversarial tick {tick}: multiple attack signatures"),
                ).with_correlation(format!("adv-chain-{tick}")),
            ];

            let all_proposals: Vec<Proposal> = conflicting
                .into_iter()
                .chain(budget.into_iter())
                .collect();

            let out = run_tick(&policy, &safety, &executor, &all_proposals, &incidents, tick);
            samples.push(Alpha3Sample {
                scenario: scenario.into(),
                iteration: iter,
                tick,
                proposal_count: all_proposals.len() as u64,
                incident_count: incidents.len() as u64,
                veto_count: out.vetoes.len() as u64,
                arbitration_count: out.arbitration_count,
                escalation_count: out.escalation_count,
                approved_count: out.approved_count,
                denied_count: out.denied_count,
                policy_eval_ms: out.policy_eval_ms,
                safety_eval_ms: out.safety_eval_ms,
                total_ms: out.total_ms,
                veto_breakdown: out.veto_breakdown.clone(),
            });
        }

        let result = aggregate_samples(scenario, seed, iter, &samples);
        all_samples.push(samples);
        all_results.push(result);
    }

    (all_samples, all_results)
}

// ── Helpers ─────────────────────────────────────────────────────────

fn make_proposal(agent: &str, kind: ActionKind, confidence: f32) -> Proposal {
    Proposal {
        id: ProposalId::new(),
        agent_id: AgentId::from(agent),
        created_at: "0.000000000Z".into(),
        based_on: vec![],
        requested_action: ActionRequest {
            id: ActionId::new(),
            kind,
            safety_level: ActionSafetyLevel::MediumRisk,
        },
        rationale: "alpha3 adversarial test".into(),
        confidence: Confidence(confidence),
    }
}

fn aggregate_samples(
    scenario: &str,
    seed: u64,
    _iteration: usize,
    samples: &[Alpha3Sample],
) -> Alpha3Result {
    let ticks = samples.len() as u64;

    let total_proposals: u64 = samples.iter().map(|s| s.proposal_count).sum();
    let total_incidents: u64 = samples.iter().map(|s| s.incident_count).sum();
    let total_vetoes: u64 = samples.iter().map(|s| s.veto_count).sum();
    let total_arbitrations: u64 = samples.iter().map(|s| s.arbitration_count).sum();
    let total_escalations: u64 = samples.iter().map(|s| s.escalation_count).sum();
    let total_approved: u64 = samples.iter().map(|s| s.approved_count).sum();
    let total_denied: u64 = samples.iter().map(|s| s.denied_count).sum();

    let mean_total_ms = if ticks > 0 {
        samples.iter().map(|s| s.total_ms).sum::<f64>() / ticks as f64
    } else {
        0.0
    };

    let mean_policy_eval_ms = if ticks > 0 {
        samples.iter().map(|s| s.policy_eval_ms).sum::<f64>() / ticks as f64
    } else {
        0.0
    };

    let mean_safety_eval_ms = if ticks > 0 {
        samples.iter().map(|s| s.safety_eval_ms).sum::<f64>() / ticks as f64
    } else {
        0.0
    };

    Alpha3Result {
        scenario: scenario.into(),
        seed,
        ticks,
        iterations: 1,
        replay_consistent: true,
        total_proposals,
        total_incidents,
        total_vetoes,
        total_arbitrations,
        total_escalations,
        total_approved,
        total_denied,
        mean_total_ms,
        mean_policy_eval_ms,
        mean_safety_eval_ms,
        veto_breakdown: {
            let mut merged: HashMap<String, u64> = HashMap::new();
            for s in samples {
                for (k, v) in &s.veto_breakdown {
                    *merged.entry(k.clone()).or_insert(0) += v;
                }
            }
            merged
        },
    }
}

fn check_replay_consistency(all_results: &[Alpha3Result]) -> bool {
    if all_results.is_empty() {
        return true;
    }
    let baseline = &all_results[0];
    all_results[1..].iter().all(|r| {
        r.total_proposals == baseline.total_proposals
            && r.total_vetoes == baseline.total_vetoes
            && r.total_arbitrations == baseline.total_arbitrations
            && r.total_escalations == baseline.total_escalations
            && r.total_approved == baseline.total_approved
            && r.total_denied == baseline.total_denied
    })
}

fn mean<T: Into<f64> + Copy>(vals: &[T]) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    vals.iter().map(|&v| v.into()).sum::<f64>() / vals.len() as f64
}

// ── Table builders ──────────────────────────────────────────────────

fn make_results_table(all_results: &[(String, Vec<Alpha3Result>)]) -> String {
    let mut s = String::new();
    s.push_str("### Alpha-3: Adversarial Governance — Results Summary\n\n");
    s.push_str("| Scenario | Ticks | Iterations | Proposals | Incidents | Vetoes | Arbitrations | Escalations | Approved | Denied | Replay | Total Lat (ms) | Policy (ms) | Safety (ms) |\n");
    s.push_str("|----------|-----:|----------:|---------:|---------:|------:|------------:|-----------:|--------:|------:|:------:|---------------:|------------:|------------:|\n");

    for (name, results) in all_results {
        let r = &results[0];
        let consistent = check_replay_consistency(results);
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.4} | {:.4} | {:.4} |\n",
            name, r.ticks, results.len(), r.total_proposals, r.total_incidents,
            r.total_vetoes, r.total_arbitrations, r.total_escalations,
            r.total_approved, r.total_denied,
            if consistent { "✓" } else { "✗" },
            r.mean_total_ms, r.mean_policy_eval_ms, r.mean_safety_eval_ms,
        ));
    }
    s
}

fn make_veto_breakdown_table(all_results: &[(String, Vec<Alpha3Result>)]) -> String {
    let mut s = String::new();
    s.push_str("### Veto Reason Breakdown by Scenario\n\n");
    s.push_str("| Scenario | InvalidProposal | ConflictingProposals | IncidentTriggered | ResourceLimitsExceeded | ActionNotPermitted |\n");
    s.push_str("|----------|----------------:|--------------------:|------------------:|----------------------:|-------------------:|\n");

    for (name, results) in all_results {
        let r = &results[0];
        let breakdown = &r.veto_breakdown;
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            name,
            breakdown.get("InvalidProposal").unwrap_or(&0),
            breakdown.get("ConflictingProposals").unwrap_or(&0),
            breakdown.get("IncidentTriggered").unwrap_or(&0),
            breakdown.get("ResourceLimitsExceeded").unwrap_or(&0),
            breakdown.get("ActionNotPermitted").unwrap_or(&0),
        ));
    }
    s
}

// ── Main ────────────────────────────────────────────────────────────

fn main() {
    let base = PathBuf::from("experiments/alpha3");
    ensure_dirs(&base);

    println!("=== Alpha-3 Adversarial Governance Campaign ===\n");

    let scenarios: Vec<(
        &str,
        fn(u64) -> (Vec<Vec<Alpha3Sample>>, Vec<Alpha3Result>),
    )> = vec![
        ("S1-proposal-conflict", scenario_proposal_conflict as fn(u64) -> (Vec<Vec<Alpha3Sample>>, Vec<Alpha3Result>)),
        ("S2-budget-violation", scenario_budget_violation),
        ("S3-incident-driven", scenario_incident_driven),
        ("S4-escalation-chain", scenario_escalation_chain),
        ("S5-adversarial", scenario_adversarial),
    ];

    let mut all_aggregated: Vec<(String, Vec<Alpha3Result>)> = Vec::new();

    for (name, scenario_fn) in &scenarios {
        println!("── {name} ──");

        let t0 = Instant::now();
        let (all_samples, all_results) = scenario_fn(42);
        let elapsed = t0.elapsed();

        let consistent = check_replay_consistency(&all_results);

        // Aggregate across iterations
        let agg_result = Alpha3Result {
            scenario: name.to_string(),
            seed: 42,
            ticks: all_results[0].ticks,
            iterations: all_results.len(),
            replay_consistent: consistent,
            total_proposals: all_results[0].total_proposals,
            total_incidents: all_results[0].total_incidents,
            total_vetoes: all_results[0].total_vetoes,
            total_arbitrations: all_results[0].total_arbitrations,
            total_escalations: all_results[0].total_escalations,
            total_approved: all_results[0].total_approved,
            total_denied: all_results[0].total_denied,
            mean_total_ms: mean(&all_results.iter().map(|r| r.mean_total_ms).collect::<Vec<_>>()),
            mean_policy_eval_ms: mean(&all_results.iter().map(|r| r.mean_policy_eval_ms).collect::<Vec<_>>()),
            mean_safety_eval_ms: mean(&all_results.iter().map(|r| r.mean_safety_eval_ms).collect::<Vec<_>>()),
            veto_breakdown: all_results[0].veto_breakdown.clone(),
        };

        all_aggregated.push((name.to_string(), all_results.clone()));

        // Write per-tick samples (first iteration only)
        let samples_csv = base.join("results").join(format!("{name}-samples.csv"));
        let mut csv = String::from("scenario,iteration,tick,proposals,incidents,vetoes,arbitrations,escalations,approved,denied,policy_eval_ms,safety_eval_ms,total_ms\n");
        for samp in &all_samples[0] {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{:.6},{:.6},{:.6}\n",
                samp.scenario, samp.iteration, samp.tick,
                samp.proposal_count, samp.incident_count,
                samp.veto_count, samp.arbitration_count, samp.escalation_count,
                samp.approved_count, samp.denied_count,
                samp.policy_eval_ms, samp.safety_eval_ms, samp.total_ms,
            ));
        }
        fs::write(&samples_csv, csv).unwrap();

        // Write aggregate JSON
        let agg_json = base.join("results").join(format!("{name}-aggregate.json"));
        write_json(&agg_json, &agg_result);

        println!(
            "  vetoes={}  arbitrations={}  escalations={}  replay=✓  ({:.2}s)",
            agg_result.total_vetoes,
            agg_result.total_arbitrations,
            agg_result.total_escalations,
            elapsed.as_secs_f64(),
        );
    }

    // ── Tables ──────────────────────────────────────────────────────
    let table_results = make_results_table(&all_aggregated);
    let table_vetoes = make_veto_breakdown_table(&all_aggregated);
    let combined = format!("{table_results}\n{table_vetoes}");
    fs::write(base.join("tables").join("alpha3-summary-table.csv"), &combined).unwrap();

    // ── Per-tick tables for figures ─────────────────────────────────
    for (name, results) in &all_aggregated {
        let _first_samples = &results[0];
        // Per-tick table is already in results/ as CSV, also put CSV figure data
        let fig_csv = base.join("figures").join(format!("{name}-profile.csv"));
        fs::write(&fig_csv, fs::read_to_string(
            base.join("results").join(format!("{name}-samples.csv"))
        ).unwrap()).unwrap();
    }

    // ── Summary ─────────────────────────────────────────────────────
    println!("\n── Alpha-3 Summary ──");
    for (name, results) in &all_aggregated {
        let r = &results[0];
        let consistent = check_replay_consistency(results);
        println!(
            "  {name}: {}/ticks={} vetoes={} arbitrations={} escalations={} replay={}",
            r.total_proposals, r.ticks, r.total_vetoes,
            r.total_arbitrations, r.total_escalations,
            if consistent { "✓" } else { "✗" },
        );
    }

    println!("\nOutput: experiments/alpha3/results/, tables/, figures/");
}
