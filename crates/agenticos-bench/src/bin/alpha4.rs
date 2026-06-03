//! Alpha-4 Governance Validation Campaign.
//!
//! Three scenarios demonstrating all three incident-response modes:
//!   A: Warning  → Execute (executions > 0, vetoes = 0)
//!   B: Error    → SelectiveVeto (executions > 0, selective_vetoes > 0)
//!   C: Critical → GlobalFreeze (veto_count ≈ proposal_count, executor_count = 0)
//!
//! Each scenario runs controlled synthetic workloads through the full pipeline:
//!   agent proposals → policy → safety governor → executor
//!
//! Output: experiments/alpha4/

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use agenticos_agents::{ProcessAgent, SecurityAgent};
use agenticos_domain::{
    ActionKind, ActionSafetyLevel, Agent, AgentId, Confidence, Decision,
    DecisionOutcome, Incident, MetricCollection, Observation, ObservationId,
    ObservationPayload, ObservationSource, ProcessObservation, Proposal, ProposalId,
};
use agenticos_executor::{ApprovedActionExecutor, DryRunExecutor};
use agenticos_policy::{DefaultPolicyKernel, DeterministicPolicyKernel, PolicyInput};
use agenticos_safety::{
    DefaultSafetyGovernor, SafetyInput, VetoReason,
};
use serde::Serialize;

const REPLAY_ITERATIONS: usize = 5;

fn ensure_dirs(base: &PathBuf) {
    let _ = fs::create_dir_all(base.join("results"));
}

fn write_json<T: Serialize>(path: &PathBuf, data: &T) {
    fs::write(path, serde_json::to_string_pretty(data).unwrap()).unwrap();
}

// ── Sample types ────────────────────────────────────────────────────

fn action_kind_name(kind: &ActionKind) -> &'static str {
    match kind {
        ActionKind::CgroupCreate { .. } => "CgroupCreate",
        ActionKind::CgroupSetCpuMax { .. } => "CgroupSetCpuMax",
        ActionKind::CgroupSetCpuWeight { .. } => "CgroupSetCpuWeight",
        ActionKind::CgroupSetMemoryMax { .. } => "CgroupSetMemoryMax",
        ActionKind::CgroupMovePid { .. } => "CgroupMovePid",
        ActionKind::ProcessFreezeGroup { .. } => "ProcessFreezeGroup",
        ActionKind::ProcessThawGroup { .. } => "ProcessThawGroup",
        ActionKind::ProcessTerminateGroup { .. } => "ProcessTerminateGroup",
        ActionKind::WorkloadClassifyRecommend { .. } => "WorkloadClassifyRecommend",
        ActionKind::ObserveOnly => "ObserveOnly",
    }
}

fn count_by_action_kind(items: &[Proposal]) -> HashMap<String, u64> {
    let mut map = HashMap::new();
    for p in items {
        let key = action_kind_name(&p.requested_action.kind).to_owned();
        *map.entry(key).or_insert(0) += 1;
    }
    map
}

fn merge_breakdowns(into: &mut HashMap<String, u64>, from: &HashMap<String, u64>) {
    for (k, v) in from {
        *into.entry(k.clone()).or_insert(0) += v;
    }
}

#[derive(Clone, Debug, Serialize)]
struct Alpha4Sample {
    scenario: String,
    tick: u64,
    proposal_count: u64,
    incident_count: u64,
    veto_count: u64,
    selective_vetoes: u64,
    global_vetoes: u64,
    approved_count: u64,
    denied_count: u64,
    executor_count: u64,
    veto_breakdown: HashMap<String, u64>,
    proposal_type_breakdown: HashMap<String, u64>,
    execution_breakdown: HashMap<String, u64>,
    severity_labels: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct Alpha4Result {
    scenario: String,
    ticks: u64,
    iterations: usize,
    replay_consistent: bool,
    total_proposals: u64,
    total_incidents: u64,
    total_vetoes: u64,
    total_selective: u64,
    total_global: u64,
    total_approved: u64,
    total_denied: u64,
    total_executions: u64,
    veto_breakdown: HashMap<String, u64>,
    proposal_type_breakdown: HashMap<String, u64>,
    execution_breakdown: HashMap<String, u64>,
    resource_modifying_count: u64,
    advisory_count: u64,
}

// ── Tick runner ─────────────────────────────────────────────────────

struct TickOutput {
    vetoes: Vec<agenticos_safety::VetoDecision>,
    approved_count: u64,
    denied_count: u64,
    executor_count: u64,
    selective_vetoes: u64,
    global_vetoes: u64,
    veto_breakdown: HashMap<String, u64>,
}

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
            source: "alpha4".into(),
            samples: vec![],
        },
    };

    let decisions: Vec<Decision> = policy.evaluate_tick(&policy_input).unwrap();

    let safety_input = SafetyInput {
        policy_input: &policy_input,
        decisions: &decisions,
    };
    let safety_output = safety.evaluate(safety_input).unwrap();

    let executor = DryRunExecutor::new();
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

    let approved_count = safety_output
        .approved
        .iter()
        .filter(|d| matches!(d.outcome, DecisionOutcome::Approved))
        .count() as u64;

    let denied_count = decisions
        .iter()
        .filter(|d| matches!(d.outcome, DecisionOutcome::Denied { .. }))
        .count() as u64;

    let selective_vetoes = safety_output
        .vetoes
        .iter()
        .filter(|v| v.reason == VetoReason::SelectiveVeto)
        .count() as u64;

    let global_vetoes = safety_output
        .vetoes
        .iter()
        .filter(|v| v.reason == VetoReason::IncidentTriggered)
        .count() as u64;

    let veto_breakdown: HashMap<String, u64> = {
        let mut map = HashMap::new();
        for v in &safety_output.vetoes {
            let key = format!("{:?}", v.reason);
            *map.entry(key).or_insert(0) += 1;
        }
        map
    };

    TickOutput {
        vetoes: safety_output.vetoes,
        approved_count,
        denied_count,
        executor_count,
        selective_vetoes,
        global_vetoes,
        veto_breakdown,
    }
}

fn aggregate(
    scenario: &str,
    all_samples: &[Vec<Alpha4Sample>],
) -> Alpha4Result {
    let iterations = all_samples.len();
    let last = &all_samples[0];
    let ticks = last.len() as u64;

    let replay_consistent = all_samples
        .windows(2)
        .all(|w| samples_equal(&w[0], &w[1]));

    let total_proposals: u64 = last.iter().map(|s| s.proposal_count).sum();
    let total_incidents: u64 = last.iter().map(|s| s.incident_count).sum();
    let total_vetoes: u64 = last.iter().map(|s| s.veto_count).sum();
    let total_selective: u64 = last.iter().map(|s| s.selective_vetoes).sum();
    let total_global: u64 = last.iter().map(|s| s.global_vetoes).sum();
    let total_approved: u64 = last.iter().map(|s| s.approved_count).sum();
    let total_denied: u64 = last.iter().map(|s| s.denied_count).sum();
    let total_executions: u64 = last.iter().map(|s| s.executor_count).sum();

    let mut veto_breakdown: HashMap<String, u64> = HashMap::new();
    let mut proposal_type_breakdown: HashMap<String, u64> = HashMap::new();
    let mut execution_breakdown: HashMap<String, u64> = HashMap::new();

    let mut resource_modifying_count = 0u64;
    let mut advisory_count = 0u64;

    for s in last {
        merge_breakdowns(&mut veto_breakdown, &s.veto_breakdown);
        merge_breakdowns(&mut proposal_type_breakdown, &s.proposal_type_breakdown);
        merge_breakdowns(&mut execution_breakdown, &s.execution_breakdown);
    }

    // Count resource-modifying vs advisory from proposal_type_breakdown
    for (kind, count) in &proposal_type_breakdown {
        let is_rm = matches!(
            kind.as_str(),
            "CgroupSetCpuMax" | "CgroupSetCpuWeight" | "CgroupSetMemoryMax"
        );
        if is_rm {
            resource_modifying_count += count;
        } else {
            advisory_count += count;
        }
    }

    Alpha4Result {
        scenario: scenario.into(),
        ticks,
        iterations,
        replay_consistent,
        total_proposals,
        total_incidents,
        total_vetoes,
        total_selective,
        total_global,
        total_approved,
        total_denied,
        total_executions,
        veto_breakdown,
        proposal_type_breakdown,
        execution_breakdown,
        resource_modifying_count,
        advisory_count,
    }
}

fn samples_equal(a: &[Alpha4Sample], b: &[Alpha4Sample]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (sa, sb) in a.iter().zip(b.iter()) {
        if sa.veto_count != sb.veto_count
            || sa.approved_count != sb.approved_count
            || sa.executor_count != sb.executor_count
        {
            return false;
        }
    }
    true
}

// ── Helpers ─────────────────────────────────────────────────────────

fn make_proposal(agent: &str, kind: ActionKind, confidence: f32) -> Proposal {
    let safety_level = match &kind {
        ActionKind::ObserveOnly => ActionSafetyLevel::ReadOnly,
        ActionKind::WorkloadClassifyRecommend { .. } => ActionSafetyLevel::LowRisk,
        _ => ActionSafetyLevel::MediumRisk,
    };
    Proposal {
        id: ProposalId::new(),
        agent_id: AgentId::from(agent),
        created_at: "0.000000000Z".into(),
        based_on: vec![],
        requested_action: agenticos_domain::ActionRequest {
            id: agenticos_domain::ActionId::new(),
            kind,
            safety_level,
        },
        rationale: "alpha4 test proposal".into(),
        confidence: Confidence(confidence),
    }
}

fn process_obs(pid: u32, ppid: u32) -> Observation {
    Observation {
        id: ObservationId::new(),
        source: ObservationSource::Process,
        observed_at: "0.000000000Z".into(),
        collection_duration_ms: 5,
        payload: ObservationPayload::Process(ProcessObservation {
            pid,
            parent_pid: ppid,
            command: format!("proc-{pid}"),
            cpu_percent: 0.0,
            memory_bytes: 0,
            state: "R".into(),
        }),
    }
}

fn print_separator(title: &str) {
    println!();
    println!("{}", "=".repeat(72));
    println!("  {title}");
    println!("{}", "=".repeat(72));
}

fn print_breakdown(title: &str, map: &HashMap<String, u64>) {
    if map.is_empty() {
        println!("  {title}: (none)");
        return;
    }
    println!("  {title}:");
    let mut items: Vec<_> = map.iter().collect();
    items.sort_by(|a, b| b.1.cmp(a.1));
    for (k, v) in &items {
        println!("    {k}: {v}");
    }
}

fn print_result(result: &Alpha4Result) {
    println!("  Scenario:             {}", result.scenario);
    println!("  Ticks:                {}", result.ticks);
    println!("  Iterations:           {}", result.iterations);
    println!("  Replay consistent:    {}", result.replay_consistent);
    println!();
    println!("  Total proposals:      {}", result.total_proposals);
    println!("    Resource-modifying: {}", result.resource_modifying_count);
    println!("    Advisory:           {}", result.advisory_count);
    println!("  Total incidents:      {}", result.total_incidents);
    println!("  Total vetoes:         {}", result.total_vetoes);
    println!("    SelectiveVeto:      {}", result.total_selective);
    println!("    IncidentTriggered:  {}", result.total_global);
    println!("  Total approved:       {}", result.total_approved);
    println!("  Total denied:         {}", result.total_denied);
    println!("  Total executions:     {}", result.total_executions);
    println!();
    print_breakdown("Proposal type breakdown", &result.proposal_type_breakdown);
    print_breakdown("Execution breakdown", &result.execution_breakdown);
    print_breakdown("Veto reason breakdown", &result.veto_breakdown);
    println!();

    // Saturation analysis
    let rm_ratio = if result.total_proposals > 0 {
        result.resource_modifying_count as f64 / result.total_proposals as f64 * 100.0
    } else {
        0.0
    };
    println!("  Resource-modifying ratio: {rm_ratio:.1}%");

    println!();
    println!("  Verification:");
    match result.scenario.as_str() {
        "A-warning-execute" => {
            println!("    executions > 0:       {}", result.total_executions > 0);
            println!("    vetoes = 0:           {}", result.total_vetoes == 0);
            if result.total_vetoes == 0 && result.total_executions > 0 {
                println!("    ✓ PASS: Warning incidents do not trigger vetoes");
            } else {
                println!("    ✗ FAIL");
            }
        }
        "B-error-selective" => {
            println!("    executions > 0:       {}", result.total_executions > 0);
            println!("    selective_vetoes > 0: {}", result.total_selective > 0);
            if result.total_executions > 0 && result.total_selective > 0 {
                println!("    ✓ PASS: Error incidents trigger SelectiveVeto while preserving liveness");
            } else {
                println!("    ✗ FAIL — check proposal-type mix");
                if result.advisory_count == 0 {
                    println!("      → All proposals are resource-modifying; no advisory proposals to execute.");
                    println!("      → This is the selective-veto saturation case.");
                }
            }
        }
        "C-critical-freeze" => {
            println!("    proposals > 0:        {}", result.total_proposals > 0);
            let veto_ratio = if result.total_proposals > 0 {
                result.total_vetoes as f64 / result.total_proposals as f64 * 100.0
            } else {
                0.0
            };
            println!("    veto ≈ proposals:     {veto_ratio:.1}% ({}/{})",
                result.total_vetoes, result.total_proposals);
            println!("    executions = 0:       {}", result.total_executions == 0);
            println!("    reason=IncidentTriggered: {}", result.total_global > 0);
            if veto_ratio > 0.0 && result.total_executions == 0 && result.total_global > 0 {
                println!("    ✓ PASS: Critical incidents trigger global freeze");
            } else {
                println!("    ✗ FAIL");
            }
        }
        _ => {}
    }
}

// ── Scenario A: Warning → Execute ───────────────────────────────────

fn scenario_warning_execute(_seed: u64) -> (Vec<Vec<Alpha4Sample>>, Alpha4Result) {
    let scenario = "A-warning-execute";
    let policy = DefaultPolicyKernel::benchmark();
    let safety = DefaultSafetyGovernor::with_defaults();
    let _executor = DryRunExecutor::new();
    let sec_agent = SecurityAgent::new(AgentId::from("security-agent"));

    let mut all_samples: Vec<Vec<Alpha4Sample>> = Vec::new();

    for _iter in 0..REPLAY_ITERATIONS {
        let mut samples: Vec<Alpha4Sample> = Vec::new();

        for tick in 1..=5u64 {
            let mut observations: Vec<Observation> = (1..=7)
                .map(|i| process_obs(1000 + i as u32, 100))
                .collect();
            observations.push(Observation {
                id: ObservationId::new(),
                source: ObservationSource::Cpu,
                observed_at: "0.000000000Z".into(),
                collection_duration_ms: 5,
                payload: ObservationPayload::Cpu(agenticos_domain::CpuObservation {
                    pressure_some_avg10: Some(0.65),
                    pressure_full_avg10: Some(0.30),
                    nr_running: Some(8),
                }),
            });

            let incidents = sec_agent.collect_incidents(&observations);
            let severity_labels: Vec<String> = incidents
                .iter()
                .map(|i| format!("{:?}", i.severity))
                .collect();

            let proc_agent = ProcessAgent::new(AgentId::from("process-agent"));
            let proposals = proc_agent.propose(&observations);
            let proposal_type_breakdown = count_by_action_kind(&proposals);

            let out = run_tick(&policy, &safety, &_executor, &proposals, &incidents, tick);

            // Build execution breakdown from safety-approved proposals
            let mut execution_breakdown = HashMap::new();
            for prop in &proposals {
                if proposal_executed(&out, prop) {
                    let key = action_kind_name(&prop.requested_action.kind).to_owned();
                    *execution_breakdown.entry(key).or_insert(0) += 1;
                }
            }

            samples.push(Alpha4Sample {
                scenario: scenario.into(),
                tick,
                proposal_count: proposals.len() as u64,
                incident_count: incidents.len() as u64,
                veto_count: out.vetoes.len() as u64,
                selective_vetoes: out.selective_vetoes,
                global_vetoes: out.global_vetoes,
                approved_count: out.approved_count,
                denied_count: out.denied_count,
                executor_count: out.executor_count,
                veto_breakdown: out.veto_breakdown,
                proposal_type_breakdown,
                execution_breakdown,
                severity_labels,
            });
        }

        all_samples.push(samples);
    }

    let result = aggregate(scenario, &all_samples);
    (all_samples, result)
}

/// Check if a proposal was NOT vetoed (passed safety and was executed).
fn proposal_executed(out: &TickOutput, prop: &Proposal) -> bool {
    !out.vetoes.iter().any(|v| v.proposal_id == prop.id)
}

// ── Scenario B: Error → SelectiveVeto ───────────────────────────────

fn scenario_error_selective(_seed: u64) -> (Vec<Vec<Alpha4Sample>>, Alpha4Result) {
    let scenario = "B-error-selective";
    let policy = DefaultPolicyKernel::benchmark();
    let safety = DefaultSafetyGovernor::with_defaults();
    let _executor = DryRunExecutor::new();
    let sec_agent = SecurityAgent::new(AgentId::from("security-agent"));

    let mut all_samples: Vec<Vec<Alpha4Sample>> = Vec::new();

    for _iter in 0..REPLAY_ITERATIONS {
        let mut samples: Vec<Alpha4Sample> = Vec::new();

        for tick in 1..=5u64 {
            let mut observations: Vec<Observation> = (1..=35)
                .map(|i| process_obs(2000 + i as u32, 1))
                .collect();
            observations.push(Observation {
                id: ObservationId::new(),
                source: ObservationSource::Cpu,
                observed_at: "0.000000000Z".into(),
                collection_duration_ms: 5,
                payload: ObservationPayload::Cpu(agenticos_domain::CpuObservation {
                    pressure_some_avg10: Some(0.75),
                    pressure_full_avg10: Some(0.40),
                    nr_running: Some(12),
                }),
            });

            let incidents = sec_agent.collect_incidents(&observations);
            let severity_labels: Vec<String> = incidents
                .iter()
                .map(|i| format!("{:?}", i.severity))
                .collect();

            // Mix: CgroupSetCpuMax (resource-modifying), WorkloadClassifyRecommend (advisory)
            let proposals = vec![
                make_proposal("agent-a", ActionKind::CgroupSetCpuMax {
                    group: "agenticos/workload".into(),
                    quota: "80000 100000".into(),
                }, 0.85),
                make_proposal("agent-b", ActionKind::WorkloadClassifyRecommend {
                    group: "system".into(),
                    classification: "cpu_pressure=0.75 — consider isolating".into(),
                }, 0.75),
            ];

            let proposal_type_breakdown = count_by_action_kind(&proposals);
            let out = run_tick(&policy, &safety, &_executor, &proposals, &incidents, tick);

            let mut execution_breakdown = HashMap::new();
            for prop in &proposals {
                if proposal_executed(&out, prop) {
                    let key = action_kind_name(&prop.requested_action.kind).to_owned();
                    *execution_breakdown.entry(key).or_insert(0) += 1;
                }
            }

            samples.push(Alpha4Sample {
                scenario: scenario.into(),
                tick,
                proposal_count: proposals.len() as u64,
                incident_count: incidents.len() as u64,
                veto_count: out.vetoes.len() as u64,
                selective_vetoes: out.selective_vetoes,
                global_vetoes: out.global_vetoes,
                approved_count: out.approved_count,
                denied_count: out.denied_count,
                executor_count: out.executor_count,
                veto_breakdown: out.veto_breakdown,
                proposal_type_breakdown,
                execution_breakdown,
                severity_labels,
            });
        }

        all_samples.push(samples);
    }

    let result = aggregate(scenario, &all_samples);
    (all_samples, result)
}

// ── Scenario C: Critical → GlobalFreeze ─────────────────────────────

fn scenario_critical_freeze(_seed: u64) -> (Vec<Vec<Alpha4Sample>>, Alpha4Result) {
    let scenario = "C-critical-freeze";
    let policy = DefaultPolicyKernel::benchmark();
    let safety = DefaultSafetyGovernor::with_defaults();
    let _executor = DryRunExecutor::new();
    let sec_agent =
        SecurityAgent::with_critical_threshold(AgentId::from("security-agent"), 50);

    let mut all_samples: Vec<Vec<Alpha4Sample>> = Vec::new();

    for _iter in 0..REPLAY_ITERATIONS {
        let mut samples: Vec<Alpha4Sample> = Vec::new();

        for tick in 1..=5u64 {
            let mut observations: Vec<Observation> = (1..=80)
                .map(|i| process_obs(3000 + i as u32, 1))
                .collect();
            observations.push(Observation {
                id: ObservationId::new(),
                source: ObservationSource::Cpu,
                observed_at: "0.000000000Z".into(),
                collection_duration_ms: 5,
                payload: ObservationPayload::Cpu(agenticos_domain::CpuObservation {
                    pressure_some_avg10: Some(0.85),
                    pressure_full_avg10: Some(0.50),
                    nr_running: Some(20),
                }),
            });

            let incidents = sec_agent.collect_incidents(&observations);
            let severity_labels: Vec<String> = incidents
                .iter()
                .map(|i| format!("{:?}", i.severity))
                .collect();

            let proposals = vec![
                make_proposal("agent-a", ActionKind::CgroupSetCpuMax {
                    group: "agenticos/workload".into(),
                    quota: "80000 100000".into(),
                }, 0.85),
                make_proposal("agent-b", ActionKind::WorkloadClassifyRecommend {
                    group: "system".into(),
                    classification: "cpu_pressure=0.85".into(),
                }, 0.75),
                make_proposal("agent-c", ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/workload".into(),
                    weight: 200,
                }, 0.9),
            ];

            let proposal_type_breakdown = count_by_action_kind(&proposals);
            let out = run_tick(&policy, &safety, &_executor, &proposals, &incidents, tick);

            let mut execution_breakdown = HashMap::new();
            for prop in &proposals {
                if proposal_executed(&out, prop) {
                    let key = action_kind_name(&prop.requested_action.kind).to_owned();
                    *execution_breakdown.entry(key).or_insert(0) += 1;
                }
            }

            samples.push(Alpha4Sample {
                scenario: scenario.into(),
                tick,
                proposal_count: proposals.len() as u64,
                incident_count: incidents.len() as u64,
                veto_count: out.vetoes.len() as u64,
                selective_vetoes: out.selective_vetoes,
                global_vetoes: out.global_vetoes,
                approved_count: out.approved_count,
                denied_count: out.denied_count,
                executor_count: out.executor_count,
                veto_breakdown: out.veto_breakdown,
                proposal_type_breakdown,
                execution_breakdown,
                severity_labels,
            });
        }

        all_samples.push(samples);
    }

    let result = aggregate(scenario, &all_samples);
    (all_samples, result)
}

// ── Main ────────────────────────────────────────────────────────────

fn main() {
    let base = PathBuf::from("experiments/alpha4");
    ensure_dirs(&base);

    println!("AgenticOS Alpha-4 Governance Validation");
    println!("=======================================");
    println!("Demonstrating all three incident-response modes:");
    println!("  A: Warning  → Execute (no vetoes)");
    println!("  B: Error    → SelectiveVeto (partial veto)");
    println!("  C: Critical → GlobalFreeze (all vetoed)");
    println!();
    println!("Includes proposal-type breakdown and execution breakdown");
    println!("to diagnose selective-veto saturation.");
    println!();

    let (_samples_a, result_a) = scenario_warning_execute(42);
    print_separator("Scenario A: Warning → Execute");
    print_result(&result_a);

    let (_samples_b, result_b) = scenario_error_selective(42);
    print_separator("Scenario B: Error → SelectiveVeto");
    print_result(&result_b);

    let (_samples_c, result_c) = scenario_critical_freeze(42);
    print_separator("Scenario C: Critical → GlobalFreeze");
    print_result(&result_c);

    // ── Summary ────────────────────────────────────────────────────
    print_separator("Summary");
    println!();
    println!("  {:<14} | {:>6} | {:>9} | {:>6} | {:>10} | {:>10} | {:<6}",
        "Mode", "Vetoes", "Selective", "Global", "Executions", "RM-ratio", "Liveness");
    println!("  {}", "-".repeat(78));
    fn liveness_icon(exec: u64, scenario: &str) -> &'static str {
        match scenario {
            "C-critical-freeze" => if exec == 0 { "✅" } else { "❌" },
            _ => if exec > 0 { "✅" } else { "❌" },
        }
    }
    println!(
        "  {:<14} | {:>6} | {:>9} | {:>6} | {:>10} | {:>9.0}% | {:<6}",
        "Warning", result_a.total_vetoes, result_a.total_selective,
        result_a.total_global, result_a.total_executions,
        result_a.resource_modifying_count as f64 / result_a.total_proposals.max(1) as f64 * 100.0,
        liveness_icon(result_a.total_executions, "A-warning-execute")
    );
    println!(
        "  {:<14} | {:>6} | {:>9} | {:>6} | {:>10} | {:>9.0}% | {:<6}",
        "Error", result_b.total_vetoes, result_b.total_selective,
        result_b.total_global, result_b.total_executions,
        result_b.resource_modifying_count as f64 / result_b.total_proposals.max(1) as f64 * 100.0,
        liveness_icon(result_b.total_executions, "B-error-selective")
    );
    println!(
        "  {:<14} | {:>6} | {:>9} | {:>6} | {:>10} | {:>9.0}% | {:<6}",
        "Critical", result_c.total_vetoes, result_c.total_selective,
        result_c.total_global, result_c.total_executions,
        result_c.resource_modifying_count as f64 / result_c.total_proposals.max(1) as f64 * 100.0,
        liveness_icon(result_c.total_executions, "C-critical-freeze")
    );

    // ── Saturation diagnostic ─────────────────────────────────────
    print_separator("Selective-Veto Saturation Diagnostic");
    println!();
    println!("Root cause: when 100% of proposals are resource-modifying,");
    println!("SelectiveVeto vetoes everything, yielding executor_count = 0.");
    println!();
    println!("This occurs when the environment does not produce the observation");
    println!("types needed for advisory proposals (e.g., CPU pressure for");
    println!("WorkloadClassifyRecommend).");
    println!();
    println!("Proposal-type breakdowns above show the mix per scenario.");
    println!("If a real system shows 100% resource-modifying proposals,");
    println!("check:");
    println!("  1. Does /proc/pressure/cpu exist and return valid data?");
    println!("  2. Are non-resource-modifying agents (e.g., ObserveOnly) registered?");
    println!("  3. Is the workload generating the right observation types?");

    let results = vec![&result_a, &result_b, &result_c];
    write_json(&base.join("results/alpha4-results.json"), &results);
    println!();
    println!("Results saved to experiments/alpha4/results/alpha4-results.json");
}
