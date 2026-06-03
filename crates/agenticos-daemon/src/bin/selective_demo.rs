//! Temporary test mode: demonstrate graduated incident-triggered veto.
//!
//! Run in terminal 1:
//!   cargo run --bin selective-demo
//!
//! In terminal 2, run to generate load:
//!   stress-ng --cpu 4 --timeout 30s
//!
//! SecurityAgent emits Error-severity incidents when process_count > 20.
//! SafetyGovernor responds: resource-modifying proposals get SelectiveVeto,
//! while advisory proposals (WorkloadClassifyRecommend) pass through.

use std::path::PathBuf;
use std::time::Duration;

use agenticos_agents::{MemoryAgent, ProcessAgent, SecurityAgent};
use agenticos_application::ObserverPort;
use agenticos_domain::{
    AgentId, ApprovedAction, MetricCollection, ObservationSource,
};
use agenticos_executor::{ApprovedActionExecutor, DryRunExecutor};
use agenticos_observe::SystemSampler;
use agenticos_policy::{DefaultPolicyKernel, DeterministicPolicyKernel, PolicyInput};
use agenticos_runtime::{AgentRuntime, InMemoryAgentRuntime};
use agenticos_safety::{DefaultSafetyGovernor, SafetyConfig, SafetyInput};

fn main() {
    if cfg!(not(target_os = "linux")) {
        eprintln!("error: selective-demo requires Linux (uses /proc and /sys/fs/cgroup)");
        std::process::exit(1);
    }

    println!("=== Selective Veto Demo ===");
    println!("Tick loop running (1 tick/sec, 40 ticks max).");
    println!("In another terminal, run:  stress-ng --cpu 4 --timeout 30s");
    println!("Watch for SelectiveVeto on resource-modifying proposals.\n");

    let observer = SystemSampler::new(Some(PathBuf::from("/sys/fs/cgroup")));

    let mut agent_runtime = InMemoryAgentRuntime::new();
    agent_runtime
        .register(Box::new(ProcessAgent::new(AgentId::from("process-agent"))))
        .unwrap();
    agent_runtime
        .register(Box::new(MemoryAgent::new(AgentId::from("mem-agent"))))
        .unwrap();
    agent_runtime
        .register(Box::new(SecurityAgent::new(AgentId::from("security-agent"))))
        .unwrap();
    agent_runtime
        .start(AgentId::from("process-agent"))
        .unwrap();
    agent_runtime.start(AgentId::from("mem-agent")).unwrap();
    agent_runtime
        .start(AgentId::from("security-agent"))
        .unwrap();

    let policy_kernel = DefaultPolicyKernel::benchmark();
    let safety_governor = DefaultSafetyGovernor::new(SafetyConfig {
        max_cpu_weight: 10000,
        max_memory_bytes: Some(64u64 * 1024 * 1024 * 1024),
        veto_on_security_incidents: true,
    });
    let executor = DryRunExecutor::new();

    let max_ticks = 40u64;
    let mut total_vetoes = 0u64;
    let mut total_selective = 0u64;
    let mut total_global = 0u64;
    let mut total_approved = 0u64;
    let mut total_executed = 0u64;
    let mut ticks_with_incidents = 0u64;
    let mut max_process_count = 0usize;

    println!(
        "{:>4}  {:>8}  {:>6}  {:>8}  {:>8}  {:>10}  {:>10}  {:>6}  {:>10}",
        "tick", "incidents", "props", "vetoes", "selective", "global", "approved", "exec", "proc_count"
    );
    println!("{}", "-".repeat(90));

    for tick in 1..=max_ticks {
        let observations = match observer.observe() {
            Ok(o) => o,
            Err(e) => {
                eprintln!("observe error: {e}");
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        let process_count = observations
            .iter()
            .filter(|o| matches!(o.source, ObservationSource::Process))
            .count();
        max_process_count = max_process_count.max(process_count);

        let proposals = match agent_runtime.collect_proposals(&observations) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("proposal error: {e}");
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        let incidents = match agent_runtime.collect_incidents(&observations) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("incident error: {e}");
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        let incident_count = incidents.len();
        if incident_count > 0 {
            ticks_with_incidents += 1;
        }

        let policy_input = PolicyInput {
            tick,
            observations,
            proposals,
            incidents,
            prior_decisions: vec![],
            metrics: MetricCollection {
                source: "selective-demo".into(),
                samples: vec![],
            },
        };

        let decisions = match policy_kernel.evaluate_tick(&policy_input) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("policy error: {e}");
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        let safety_input = SafetyInput {
            policy_input: &policy_input,
            decisions: &decisions,
        };
        let safety_output = match safety_governor.evaluate(safety_input) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("safety error: {e}");
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        let veto_count = safety_output.vetoes.len() as u64;
        let approved_count = safety_output.approved.len() as u64;
        total_vetoes += veto_count;
        total_selective += safety_output.metrics.selective_vetoes;
        total_global += safety_output.metrics.global_vetoes;
        total_approved += approved_count;

        let mut executed = 0u64;
        for decision in &safety_output.approved {
            let prop = policy_input
                .proposals
                .iter()
                .find(|p| p.id == decision.proposal_id);
            if let Some(p) = prop {
                let approved = ApprovedAction {
                    request: p.requested_action.clone(),
                    decision_id: decision.id.clone(),
                };
                if executor.execute(approved).is_ok() {
                    executed += 1;
                }
            }
        }
        total_executed += executed;

        println!(
            "{tick:>4}  {incident_count:>8}  {:>6}  {veto_count:>8}  {:>10}  {:>10}  {approved_count:>10}  {executed:>6}  {process_count:>10}",
            policy_input.proposals.len(),
            safety_output.metrics.selective_vetoes,
            safety_output.metrics.global_vetoes,
        );

        std::thread::sleep(Duration::from_secs(1));
    }

    println!();
    println!("=== Summary ===");
    println!("  Ticks run:              {max_ticks}");
    println!("  Ticks with incidents:   {ticks_with_incidents}");
    println!("  Max process count:      {max_process_count}");
    println!("  Total vetoes:           {total_vetoes}");
    println!("    SelectiveVeto:        {total_selective}");
    println!("    Global (IncidentTriggered): {total_global}");
    println!("  Total approved:         {total_approved}");
    println!("  Total executed:         {total_executed}");
    println!();
    println!("=== Verification ===");
    println!(
        "  ✅ Some proposals vetoed with SelectiveVeto: {}",
        if total_selective > 0 { "YES" } else { "NO" }
    );
    println!(
        "  ✅ Some proposals still executed:            {}",
        if total_executed > total_selective { "YES" } else { "NO" }
    );
    println!(
        "  ✅ executor_count > 0:                       {}",
        if total_executed > 0 { "YES" } else { "NO" }
    );
    println!(
        "  ✅ veto_count > 0:                           {}",
        if total_vetoes > 0 { "YES" } else { "NO" }
    );
}
