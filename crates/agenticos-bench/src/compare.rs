use agenticos_domain::AgentId;
use agenticos_executor::{ApprovedActionExecutor, DryRunExecutor};
use agenticos_policy::{DefaultPolicyKernel, DeterministicPolicyKernel, PolicyInput};
use agenticos_runtime::{AgentRuntime, InMemoryAgentRuntime};
use agenticos_safety::{DefaultSafetyGovernor, SafetyConfig, SafetyInput};
use serde::Serialize;

use crate::workload::{WorkloadConfig, WorkloadKind};

/// Pipeline configuration for comparative evaluation.
#[derive(Clone, Debug, PartialEq)]
pub enum PipelineMode {
    /// Policy Kernel only (no agents, no safety governor).
    PolicyOnly,
    /// Full pipeline with agents + Policy Kernel (no safety governor).
    AgentsAndPolicy,
    /// Full pipeline with agents + Policy Kernel + Safety Governor.
    Full,
}

impl PipelineMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::PolicyOnly => "policy-only",
            Self::AgentsAndPolicy => "agents+policy",
            Self::Full => "full",
        }
    }
}

/// Run a single tick through a specific pipeline mode.
pub fn run_comparative_tick(
    mode: &PipelineMode,
    observations: &[agenticos_domain::Observation],
    tick: u64,
) -> ComparativeTickResult {
    let mut runtime = InMemoryAgentRuntime::new();
    let policy = DefaultPolicyKernel::benchmark();
    let safety = DefaultSafetyGovernor::new(SafetyConfig {
        max_cpu_weight: 10000,
        max_memory_bytes: Some(64u64 * 1024 * 1024 * 1024),
        veto_on_security_incidents: true,
    });
    let executor = DryRunExecutor::new();

    match mode {
        PipelineMode::PolicyOnly => {
            // No agents — directly build PolicyInput with empty proposals
            let proposals = vec![];
            let incidents = vec![];
            let policy_input = PolicyInput {
                tick,
                observations: observations.to_vec(),
                proposals,
                incidents,
                prior_decisions: vec![],
                metrics: agenticos_domain::MetricCollection {
                    source: "compare".into(),
                    samples: vec![],
                },
            };
            let decisions = policy.evaluate_tick(&policy_input).unwrap();
            return ComparativeTickResult {
                mode: mode.label().into(),
                tick,
                proposal_count: 0,
                incident_count: 0,
                veto_count: 0,
                decision_count: decisions.len() as u64,
                approved_count: 0,
                denied_count: 0,
                executor_count: 0,
            };
        }
        PipelineMode::AgentsAndPolicy => {
            // Register standard agents
            register_standard_agents(&mut runtime);
            let proposals = runtime.collect_proposals(observations).unwrap();
            let incidents = runtime.collect_incidents(observations).unwrap();
            let policy_input = PolicyInput {
                tick,
                observations: observations.to_vec(),
                proposals,
                incidents,
                prior_decisions: vec![],
                metrics: agenticos_domain::MetricCollection {
                    source: "compare".into(),
                    samples: vec![],
                },
            };
            let decisions = policy.evaluate_tick(&policy_input).unwrap();
            let approved = decisions
                .iter()
                .filter(|d| matches!(d.outcome, agenticos_domain::DecisionOutcome::Approved))
                .count() as u64;
            let denied = decisions
                .iter()
                .filter(|d| matches!(d.outcome, agenticos_domain::DecisionOutcome::Denied { .. }))
                .count() as u64;
            return ComparativeTickResult {
                mode: mode.label().into(),
                tick,
                proposal_count: policy_input.proposals.len() as u64,
                incident_count: policy_input.incidents.len() as u64,
                veto_count: 0,
                decision_count: decisions.len() as u64,
                approved_count: approved,
                denied_count: denied,
                executor_count: 0,
            };
        }
        PipelineMode::Full => {
            register_standard_agents(&mut runtime);
            let proposals = runtime.collect_proposals(observations).unwrap();
            let incidents = runtime.collect_incidents(observations).unwrap();
            let policy_input = PolicyInput {
                tick,
                observations: observations.to_vec(),
                proposals,
                incidents,
                prior_decisions: vec![],
                metrics: agenticos_domain::MetricCollection {
                    source: "compare".into(),
                    samples: vec![],
                },
            };
            let decisions = policy.evaluate_tick(&policy_input).unwrap();
            let safety_input = SafetyInput {
                policy_input: &policy_input,
                decisions: &decisions,
            };
            let safety_output = safety.evaluate(safety_input).unwrap();
            let safe_ids: std::collections::HashSet<_> = safety_output
                .approved
                .iter()
                .map(|d| d.proposal_id.clone())
                .collect();
            let mut executor_count = 0u64;
            for (prop, decision) in policy_input.proposals.iter().zip(decisions.iter()) {
                if !safe_ids.contains(&decision.proposal_id) {
                    continue;
                }
                if matches!(decision.outcome, agenticos_domain::DecisionOutcome::Approved) {
                    let approved = agenticos_domain::ApprovedAction {
                        request: prop.requested_action.clone(),
                        decision_id: decision.id.clone(),
                    };
                    let _ = executor.execute(approved);
                    executor_count += 1;
                }
            }
            ComparativeTickResult {
                mode: mode.label().into(),
                tick,
                proposal_count: policy_input.proposals.len() as u64,
                incident_count: policy_input.incidents.len() as u64,
                veto_count: safety_output.vetoes.len() as u64,
                decision_count: decisions.len() as u64,
                approved_count: safety_output
                    .approved
                    .iter()
                    .filter(|d| matches!(d.outcome, agenticos_domain::DecisionOutcome::Approved))
                    .count() as u64,
                denied_count: decisions
                    .iter()
                    .filter(|d| matches!(d.outcome, agenticos_domain::DecisionOutcome::Denied { .. }))
                    .count() as u64,
                executor_count,
            }
        }
    }
}

fn register_standard_agents(runtime: &mut InMemoryAgentRuntime) {
    use agenticos_agents::{MemoryAgent, ProcessAgent, SecurityAgent};
    // Register ProcessAgent
    if runtime
        .register(Box::new(ProcessAgent::new(AgentId::from("process-agent"))))
        .is_ok()
    {
        let _ = runtime.start(AgentId::from("process-agent"));
    }
    // Register MemoryAgent
    if runtime
        .register(Box::new(MemoryAgent::new(AgentId::from("memory-agent"))))
        .is_ok()
    {
        let _ = runtime.start(AgentId::from("memory-agent"));
    }
    // Register SecurityAgent
    if runtime
        .register(Box::new(SecurityAgent::new(AgentId::from("security-agent"))))
        .is_ok()
    {
        let _ = runtime.start(AgentId::from("security-agent"));
    }
}

/// Per-tick result for a comparative run.
#[derive(Clone, Debug, Serialize)]
pub struct ComparativeTickResult {
    pub mode: String,
    pub tick: u64,
    pub proposal_count: u64,
    pub incident_count: u64,
    pub veto_count: u64,
    pub decision_count: u64,
    pub approved_count: u64,
    pub denied_count: u64,
    pub executor_count: u64,
}

/// Run comparative evaluation across all modes for a given workload.
pub fn run_comparative(
    kind: WorkloadKind,
    seed: u64,
    ticks: u64,
) -> Vec<ComparativeTickResult> {
    let mut results: Vec<ComparativeTickResult> = Vec::new();
    let mut generator = crate::workload::WorkloadGenerator::new(WorkloadConfig {
        kind,
        seed,
        tick_count: ticks,
        ..WorkloadConfig::default()
    });

    // Generate all observations once (shared across all modes)
    let mut all_obs: Vec<Vec<agenticos_domain::Observation>> = Vec::new();
    for _ in 0..ticks {
        all_obs.push(generator.next_tick());
    }

    for mode in &[PipelineMode::PolicyOnly, PipelineMode::AgentsAndPolicy, PipelineMode::Full] {
        for (tick_idx, obs) in all_obs.iter().enumerate() {
            let result = run_comparative_tick(mode, obs, (tick_idx + 1) as u64);
            results.push(result);
        }
    }

    results
}

/// Write comparative results to CSV.
pub fn write_comparative_csv(
    results: &[ComparativeTickResult],
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record(&[
        "mode", "tick", "proposal_count", "incident_count", "veto_count",
        "decision_count", "approved_count", "denied_count", "executor_count",
    ])?;
    for r in results {
        wtr.write_record(&[
            &r.mode,
            &r.tick.to_string(),
            &r.proposal_count.to_string(),
            &r.incident_count.to_string(),
            &r.veto_count.to_string(),
            &r.decision_count.to_string(),
            &r.approved_count.to_string(),
            &r.denied_count.to_string(),
            &r.executor_count.to_string(),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

/// Write comparative results to JSON.
pub fn write_comparative_json(
    results: &[ComparativeTickResult],
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(results)?;
    std::fs::write(path, json)?;
    Ok(())
}
