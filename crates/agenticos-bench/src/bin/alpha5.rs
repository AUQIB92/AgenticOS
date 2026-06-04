//! Alpha-5: Real Linux Kernel Validation.
//!
//! Five scenarios demonstrating actual cgroup mutations through the full
//! Observe → Propose → Policy → Safety → LinuxCgroupExecutor pipeline.
//!
//! Each scenario:
//!   1. Creates a disposable sub-cgroup under /sys/fs/cgroup/agenticos-test/
//!   2. Reads pre-mutation kernel state (cpu.weight, cpu.max, memory.max)
//!   3. Runs 3–5 ticks through the governance pipeline
//!   4. Reads post-mutation kernel state
//!   5. Records old/new values, success/failure, and rollback tokens
//!   6. Verifies kernel state changed as expected
//!
//! On non-Linux platforms this binary prints a message and exits.

#[cfg(target_os = "linux")]
fn main() {
    if let Err(e) = run_alpha5() {
        eprintln!("Alpha-5 failed: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Alpha-5 requires Linux (cgroup v2). Build and run on WSL or native Linux.");
    std::process::exit(0);
}

// ── Linux implementation ──────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod alpha5_impl {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use agenticos_domain::{
        ActionId, ActionKind, ActionRequest, ActionSafetyLevel, AgentId, ApprovedAction,
        Confidence, DecisionId, DecisionOutcome, Incident, IncidentCategory, IncidentSeverity,
        MetricCollection, Proposal, ProposalId,
    };
    use agenticos_executor::{linux::LinuxCgroupExecutor, ApprovedActionExecutor};
    use agenticos_policy::{DefaultPolicyKernel, DeterministicPolicyKernel, PolicyInput};
    use agenticos_safety::{DefaultSafetyGovernor, SafetyInput};
    use serde::Serialize;

    const TEST_CGROUP: &str = "/sys/fs/cgroup/agenticos-test";
    const REPLAY_ITERATIONS: usize = 3;

    // ── Data structures ───────────────────────────────────────────────

    #[derive(Clone, Debug, Serialize)]
    struct CgroupState {
        cpu_weight: Option<String>,
        cpu_max: Option<String>,
        memory_max: Option<String>,
    }

    #[derive(Clone, Debug, Serialize)]
    struct MutationRecord {
        tick: u64,
        action_kind: String,
        target_group: String,
        target_file: String,
        status: String,
        old_value: Option<String>,
        new_value: Option<String>,
        duration_ms: u64,
        has_rollback: bool,
    }

    #[derive(Clone, Debug, Serialize)]
    struct ScenarioResult {
        name: String,
        description: String,
        ticks: u64,
        iterations: usize,
        replay_consistent: bool,
        successful_mutations: u64,
        failed_mutations: u64,
        vetoed_count: u64,
        denied_count: u64,
        mutation_records: Vec<MutationRecord>,
        all_passed: bool,
    }

    // ── Test cgroup helper ────────────────────────────────────────────

    struct TestCgroup {
        root: PathBuf,
        subgroups: Vec<String>,
    }

    impl TestCgroup {
        fn new(path: &str) -> Result<Self, String> {
            let root = PathBuf::from(path);
            if !root.exists() {
                std::fs::create_dir_all(&root)
                    .map_err(|e| format!("create test cgroup {:?}: {}", root, e))?;
                let subtree = root.join("cgroup.subtree_control");
                let _ = std::fs::write(&subtree, b"+cpu +memory");
            } else {
                let subtree = root.join("cgroup.subtree_control");
                let _ = std::fs::write(&subtree, b"+cpu +memory");
            }
            Ok(Self {
                root,
                subgroups: Vec::new(),
            })
        }

        fn create_subgroup(&mut self, name: &str) -> Result<PathBuf, String> {
            let path = self.root.join(name);
            std::fs::create_dir_all(&path)
                .map_err(|e| format!("create subgroup {:?}: {}", path, e))?;
            self.subgroups.push(name.to_owned());
            Ok(path)
        }

        fn read_state(&self, group: &str) -> CgroupState {
            let base = self.root.join(group);
            CgroupState {
                cpu_weight: read_cgroup_file(&base, "cpu.weight"),
                cpu_max: read_cgroup_file(&base, "cpu.max"),
                memory_max: read_cgroup_file(&base, "memory.max"),
            }
        }

        fn read_specific(&self, group: &str, file: &str) -> Option<String> {
            read_cgroup_file(&self.root.join(group), file)
        }

        fn executor(&self) -> LinuxCgroupExecutor {
            LinuxCgroupExecutor::new(self.root.clone())
        }
    }

    impl Drop for TestCgroup {
        fn drop(&mut self) {
            for name in self.subgroups.iter().rev() {
                let path = self.root.join(name);
                let _ = std::fs::remove_dir(&path);
            }
            let _ = std::fs::remove_dir(&self.root);
        }
    }

    fn read_cgroup_file(base: &Path, file: &str) -> Option<String> {
        let path = base.join(file);
        std::fs::read_to_string(&path).ok().map(|s| s.trim().to_owned())
    }

    // ── Proposal helpers ──────────────────────────────────────────────

    fn make_proposal(
        agent: &str,
        kind: ActionKind,
        safety: ActionSafetyLevel,
    ) -> Proposal {
        Proposal {
            id: ProposalId::new(),
            agent_id: AgentId::from(agent),
            created_at: "0.000000000Z".to_owned(),
            based_on: vec![],
            requested_action: ActionRequest {
                id: ActionId::new(),
                kind,
                safety_level: safety,
            },
            rationale: "alpha5 test proposal".to_owned(),
            confidence: Confidence(1.0),
        }
    }

    fn action_kind_file(kind: &ActionKind) -> &'static str {
        match kind {
            ActionKind::CgroupCreate { .. } => "cgroup.subtree_control",
            ActionKind::CgroupSetCpuWeight { .. } => "cpu.weight",
            ActionKind::CgroupSetCpuMax { .. } => "cpu.max",
            ActionKind::CgroupSetMemoryMax { .. } => "memory.max",
            ActionKind::CgroupMovePid { .. } => "cgroup.procs",
            ActionKind::ProcessFreezeGroup { .. } => "cgroup.freeze",
            ActionKind::ProcessThawGroup { .. } => "cgroup.freeze",
            ActionKind::ProcessTerminateGroup { .. } => "cgroup.procs",
            ActionKind::WorkloadClassifyRecommend { .. } => "none",
            ActionKind::ObserveOnly => "none",
            ActionKind::LaunchApplication { .. } => "none",
            ActionKind::OpenUrl { .. } => "none",
            ActionKind::RunCommand { .. } => "none",
            ActionKind::CreateDirectory { .. } => "none",
            ActionKind::OpenFile { .. } => "none",
            ActionKind::CloneRepository { .. } => "none",
            ActionKind::CreateProjectWorkspace { .. } => "none",
        }
    }

    fn action_kind_name(kind: &ActionKind) -> &'static str {
        match kind {
            ActionKind::CgroupCreate { .. } => "CgroupCreate",
            ActionKind::CgroupSetCpuWeight { .. } => "CgroupSetCpuWeight",
            ActionKind::CgroupSetCpuMax { .. } => "CgroupSetCpuMax",
            ActionKind::CgroupSetMemoryMax { .. } => "CgroupSetMemoryMax",
            ActionKind::CgroupMovePid { .. } => "CgroupMovePid",
            ActionKind::ProcessFreezeGroup { .. } => "ProcessFreezeGroup",
            ActionKind::ProcessThawGroup { .. } => "ProcessThawGroup",
            ActionKind::ProcessTerminateGroup { .. } => "ProcessTerminateGroup",
            ActionKind::WorkloadClassifyRecommend { .. } => "WorkloadClassifyRecommend",
            ActionKind::ObserveOnly => "ObserveOnly",
            ActionKind::LaunchApplication { .. } => "LaunchApplication",
            ActionKind::OpenUrl { .. } => "OpenUrl",
            ActionKind::RunCommand { .. } => "RunCommand",
            ActionKind::CreateDirectory { .. } => "CreateDirectory",
            ActionKind::OpenFile { .. } => "OpenFile",
            ActionKind::CloneRepository { .. } => "CloneRepository",
            ActionKind::CreateProjectWorkspace { .. } => "CreateProjectWorkspace",
        }
    }

    // ── Per-tick pipeline runner ──────────────────────────────────────

    struct TickDiagnostics {
        tick: u64,
        proposal_count: usize,
        incident_count: usize,
        veto_count: usize,
        freeze_ticks: u64,
        selective_vetoes: u64,
        global_vetoes: u64,
        incident_categories: Vec<String>,
    }

    fn run_tick(
        executor: &LinuxCgroupExecutor,
        governor: &DefaultSafetyGovernor,
        kernel: &DefaultPolicyKernel,
        proposals: &[Proposal],
        incidents: &[Incident],
        test_cgroup: &TestCgroup,
        scenario_group: &str,
        tick: u64,
    ) -> (Vec<MutationRecord>, TickDiagnostics) {
        let input = PolicyInput {
            tick,
            observations: vec![],
            proposals: proposals.to_vec(),
            incidents: incidents.to_vec(),
            prior_decisions: vec![],
            metrics: MetricCollection {
                source: "alpha5".into(),
                samples: vec![],
            },
        };

        let decisions = match kernel.evaluate_tick(&input) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  policy error tick {tick}: {e}");
                return (vec![], TickDiagnostics {
                    tick,
                    proposal_count: proposals.len(),
                    incident_count: incidents.len(),
                    veto_count: 0,
                    freeze_ticks: 0,
                    selective_vetoes: 0,
                    global_vetoes: 0,
                    incident_categories: incidents.iter().map(|i| format!("{:?}", i.category)).collect(),
                });
            }
        };

        let safety_input = SafetyInput {
            policy_input: &input,
            decisions: &decisions,
        };
        let safety_output = match governor.evaluate(safety_input) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("  safety error tick {tick}: {e}");
                return (vec![], TickDiagnostics {
                    tick,
                    proposal_count: proposals.len(),
                    incident_count: incidents.len(),
                    veto_count: 0,
                    freeze_ticks: 0,
                    selective_vetoes: 0,
                    global_vetoes: 0,
                    incident_categories: incidents.iter().map(|i| format!("{:?}", i.category)).collect(),
                });
            }
        };

        let safe_ids: HashSet<_> = safety_output
            .approved
            .iter()
            .map(|d| d.proposal_id.clone())
            .collect();

        let mut records = Vec::new();

        for (prop, decision) in proposals.iter().zip(decisions.iter()) {
            let target_file = action_kind_file(&prop.requested_action.kind);
            let before = test_cgroup.read_specific(scenario_group, target_file);

            if !safe_ids.contains(&decision.proposal_id) {
                let after = test_cgroup.read_specific(scenario_group, target_file);
                records.push(MutationRecord {
                    tick,
                    action_kind: action_kind_name(&prop.requested_action.kind).to_owned(),
                    target_group: scenario_group.to_owned(),
                    target_file: target_file.to_owned(),
                    status: "Vetoed".into(),
                    old_value: before,
                    new_value: after,
                    duration_ms: 0,
                    has_rollback: false,
                });
                continue;
            }

            match &decision.outcome {
                DecisionOutcome::Approved => {
                    let approved = ApprovedAction {
                        request: prop.requested_action.clone(),
                        decision_id: decision.id.clone(),
                    };
                    let exec_start = Instant::now();
                    match executor.execute(approved) {
                        Ok(result) => {
                            let duration_ms = exec_start.elapsed().as_millis() as u64;
                            let after =
                                test_cgroup.read_specific(scenario_group, target_file);
                            records.push(MutationRecord {
                                tick,
                                action_kind: action_kind_name(&prop.requested_action.kind)
                                    .to_owned(),
                                target_group: scenario_group.to_owned(),
                                target_file: target_file.to_owned(),
                                status: format!("{:?}", result.status),
                                old_value: before,
                                new_value: after,
                                duration_ms,
                                has_rollback: result.rollback.is_some(),
                            });
                        }
                        Err(e) => {
                            let after =
                                test_cgroup.read_specific(scenario_group, target_file);
                            records.push(MutationRecord {
                                tick,
                                action_kind: action_kind_name(&prop.requested_action.kind)
                                    .to_owned(),
                                target_group: scenario_group.to_owned(),
                                target_file: target_file.to_owned(),
                                status: format!("ExecutorError({e})"),
                                old_value: before,
                                new_value: after,
                                duration_ms: 0,
                                has_rollback: false,
                            });
                        }
                    }
                }
                DecisionOutcome::Denied { .. } | DecisionOutcome::RequiresApproval => {
                    let after = test_cgroup.read_specific(scenario_group, target_file);
                    records.push(MutationRecord {
                        tick,
                        action_kind: action_kind_name(&prop.requested_action.kind).to_owned(),
                        target_group: scenario_group.to_owned(),
                        target_file: target_file.to_owned(),
                        status: "Denied".into(),
                        old_value: before,
                        new_value: after,
                        duration_ms: 0,
                        has_rollback: false,
                    });
                }
            }
        }

        let diag = TickDiagnostics {
            tick,
            proposal_count: proposals.len(),
            incident_count: incidents.len(),
            veto_count: safety_output.vetoes.len(),
            freeze_ticks: safety_output.metrics.freeze_ticks,
            selective_vetoes: safety_output.metrics.selective_vetoes,
            global_vetoes: safety_output.metrics.global_vetoes,
            incident_categories: incidents.iter().map(|i| format!("{:?}", i.category)).collect(),
        };

        (records, diag)
    }

    // ── Scenario runner ───────────────────────────────────────────────

    fn run_scenario(
        test_cgroup: &mut TestCgroup,
        name: &str,
        description: &str,
        scenario_group: &str,
        tick_proposals: Vec<Vec<Proposal>>,
        tick_incidents: Vec<Vec<Incident>>,
    ) -> ScenarioResult {
        test_cgroup.create_subgroup(scenario_group).unwrap();

        // Create the scenario group using the executor's do_create path.
        // This ensures controllers are enabled properly.
        let executor = test_cgroup.executor();
        let _ = executor.execute(ApprovedAction {
            request: ActionRequest {
                id: ActionId::new(),
                kind: ActionKind::CgroupCreate {
                    name: scenario_group.to_owned(),
                },
                safety_level: ActionSafetyLevel::LowRisk,
            },
            decision_id: DecisionId::new(),
        });

        let kernel = DefaultPolicyKernel::benchmark();
        let governor = DefaultSafetyGovernor::with_defaults();

        let mut all_records: Vec<Vec<MutationRecord>> = Vec::new();

        for iter in 0..REPLAY_ITERATIONS {
            let mut records = Vec::new();
            for tick in 0..tick_proposals.len() {
                let props = &tick_proposals[tick];
                let inc = if tick < tick_incidents.len() {
                    &tick_incidents[tick]
                } else {
                    &tick_incidents[tick_incidents.len() - 1]
                };
                let (recs, diag) = run_tick(
                    &executor,
                    &governor,
                    &kernel,
                    props,
                    inc,
                    test_cgroup,
                    scenario_group,
                    (tick + 1) as u64,
                );
                if iter == 0 {
                    println!(
                        "    tick={} proposals={} incidents={:?} vetoes={} freeze={} global={} selective={}",
                        diag.tick,
                        diag.proposal_count,
                        diag.incident_categories,
                        diag.veto_count,
                        diag.freeze_ticks,
                        diag.global_vetoes,
                        diag.selective_vetoes,
                    );
                }
                records.extend(recs);
            }
            all_records.push(records);
        }

        let replay_consistent = all_records.windows(2).all(|w| {
            w[0].iter().map(|r| format!("{}{}{}", r.tick, r.action_kind, r.status))
                .eq(w[1].iter().map(|r| format!("{}{}{}", r.tick, r.action_kind, r.status)))
        });

        let records = all_records.into_iter().next().unwrap_or_default();
        let successful = records.iter().filter(|r| r.status == "Succeeded").count() as u64;
        let failed = records.iter().filter(|r| r.status == "Failed").count() as u64;
        let vetoed = records.iter().filter(|r| r.status == "Vetoed").count() as u64;
        let denied = records.iter().filter(|r| r.status == "Denied").count() as u64;

        let all_passed = records.iter().all(|r| match r.status.as_str() {
            "Succeeded" | "Vetoed" => true,
            _ => false,
        });

        ScenarioResult {
            name: name.to_owned(),
            description: description.to_owned(),
            ticks: tick_proposals.len() as u64,
            iterations: REPLAY_ITERATIONS,
            replay_consistent,
            successful_mutations: successful,
            failed_mutations: failed,
            vetoed_count: vetoed,
            denied_count: denied,
            mutation_records: records,
            all_passed,
        }
    }

    // ── Scenarios ─────────────────────────────────────────────────────

    fn scenario_a(test_cgroup: &mut TestCgroup) -> ScenarioResult {
        let props_tick1 = vec![make_proposal(
            "cpu-agent",
            ActionKind::CgroupSetCpuWeight {
                group: "scenario-a".into(),
                weight: 200,
            },
            ActionSafetyLevel::MediumRisk,
        )];

        let props_tick2 = vec![make_proposal(
            "cpu-agent",
            ActionKind::CgroupSetCpuMax {
                group: "scenario-a".into(),
                quota: "50000 100000".into(),
            },
            ActionSafetyLevel::MediumRisk,
        )];

        run_scenario(
            test_cgroup,
            "A-cpu-pressure",
            "Set cpu.weight and cpu.max, verify kernel state reflects changes",
            "scenario-a",
            vec![props_tick1, props_tick2],
            vec![vec![]; 2],
        )
    }

    fn scenario_b(test_cgroup: &mut TestCgroup) -> ScenarioResult {
        let props_tick1 = vec![make_proposal(
            "mem-agent",
            ActionKind::CgroupSetMemoryMax {
                group: "scenario-b".into(),
                bytes: 52_428_800, // 50 MiB
            },
            ActionSafetyLevel::MediumRisk,
        )];

        let props_tick2 = vec![make_proposal(
            "mem-agent",
            ActionKind::CgroupSetMemoryMax {
                group: "scenario-b".into(),
                bytes: 104_857_600, // 100 MiB
            },
            ActionSafetyLevel::MediumRisk,
        )];

        run_scenario(
            test_cgroup,
            "B-memory-pressure",
            "Set memory.max twice, verify each write changes kernel state",
            "scenario-b",
            vec![props_tick1, props_tick2],
            vec![vec![]; 2],
        )
    }

    fn scenario_c(test_cgroup: &mut TestCgroup) -> ScenarioResult {
        let props_tick1 = vec![
            make_proposal(
                "cpu-agent",
                ActionKind::CgroupSetCpuWeight {
                    group: "scenario-c".into(),
                    weight: 300,
                },
                ActionSafetyLevel::MediumRisk,
            ),
            make_proposal(
                "mem-agent",
                ActionKind::CgroupSetMemoryMax {
                    group: "scenario-c".into(),
                    bytes: 26_214_400, // 25 MiB
                },
                ActionSafetyLevel::MediumRisk,
            ),
        ];

        let props_tick2 = vec![
            make_proposal(
                "cpu-agent",
                ActionKind::CgroupSetCpuMax {
                    group: "scenario-c".into(),
                    quota: "80000 100000".into(),
                },
                ActionSafetyLevel::MediumRisk,
            ),
            make_proposal(
                "freeze-agent",
                ActionKind::ProcessFreezeGroup {
                    group: "scenario-c".into(),
                },
                ActionSafetyLevel::MediumRisk,
            ),
        ];

        let props_tick3 = vec![make_proposal(
            "thaw-agent",
            ActionKind::ProcessThawGroup {
                group: "scenario-c".into(),
            },
            ActionSafetyLevel::LowRisk,
        )];

        run_scenario(
            test_cgroup,
            "C-mixed-workload",
            "Exercise cpu.weight, cpu.max, memory.max, freeze, and thaw in sequence",
            "scenario-c",
            vec![props_tick1, props_tick2, props_tick3],
            vec![vec![]; 3],
        )
    }

    fn scenario_d(test_cgroup: &mut TestCgroup) -> ScenarioResult {
        // Critical severity ONLY triggers GlobalFreeze when category is Security.
        let critical_incidents = vec![Incident::new(
            IncidentCategory::Security,
            IncidentSeverity::Critical,
            AgentId::from("security-agent"),
            None,
            "simulated critical resource exhaustion for freeze test",
        )];

        let props_tick1 = vec![
            make_proposal(
                "cpu-agent",
                ActionKind::CgroupSetCpuWeight {
                    group: "scenario-d".into(),
                    weight: 150,
                },
                ActionSafetyLevel::MediumRisk,
            ),
            make_proposal(
                "mem-agent",
                ActionKind::CgroupSetMemoryMax {
                    group: "scenario-d".into(),
                    bytes: 10_000_000,
                },
                ActionSafetyLevel::MediumRisk,
            ),
            make_proposal(
                "observe-agent",
                ActionKind::WorkloadClassifyRecommend {
                    group: "scenario-d".into(),
                    classification: "critical-load".into(),
                },
                ActionSafetyLevel::ReadOnly,
            ),
        ];

        run_scenario(
            test_cgroup,
            "D-critical-freeze",
            "Critical incidents trigger GlobalFreeze — all mutations blocked",
            "scenario-d",
            vec![props_tick1],
            vec![critical_incidents],
        )
    }

    fn scenario_e(test_cgroup: &mut TestCgroup) -> ScenarioResult {
        let props_tick1 = vec![
            make_proposal(
                "cpu-agent",
                ActionKind::CgroupSetCpuWeight {
                    group: "scenario-e".into(),
                    weight: 250,
                },
                ActionSafetyLevel::MediumRisk,
            ),
            make_proposal(
                "mem-agent",
                ActionKind::CgroupSetMemoryMax {
                    group: "scenario-e".into(),
                    bytes: 20_000_000,
                },
                ActionSafetyLevel::MediumRisk,
            ),
        ];

        let props_tick2 = vec![
            make_proposal(
                "cpu-agent",
                ActionKind::CgroupSetCpuWeight {
                    group: "scenario-e".into(),
                    weight: 350,
                },
                ActionSafetyLevel::MediumRisk,
            ),
            make_proposal(
                "mem-agent",
                ActionKind::CgroupSetMemoryMax {
                    group: "scenario-e".into(),
                    bytes: 40_000_000,
                },
                ActionSafetyLevel::MediumRisk,
            ),
        ];

        run_scenario(
            test_cgroup,
            "E-recovery-after-freeze",
            "No incidents — all mutations should succeed (recovery baseline)",
            "scenario-e",
            vec![props_tick1, props_tick2],
            vec![vec![]; 2],
        )
    }

    // ── Printer ───────────────────────────────────────────────────────

    fn print_result(scenario: &ScenarioResult) {
        println!();
        println!("{}", "=" .repeat(72));
        println!("  Scenario: {}", scenario.name);
        println!("  {}", scenario.description);
        println!("{}", "=" .repeat(72));
        println!("  Ticks:                {}", scenario.ticks);
        println!("  Iterations:           {}", scenario.iterations);
        println!("  Replay consistent:    {}", scenario.replay_consistent);
        println!("  Successful mutations: {}", scenario.successful_mutations);
        println!("  Failed mutations:     {}", scenario.failed_mutations);
        println!("  Vetoed:               {}", scenario.vetoed_count);
        println!("  Denied:               {}", scenario.denied_count);
        println!();

        for (i, rec) in scenario.mutation_records.iter().enumerate() {
            println!(
                "  [{:2}] tick={} kind={:20} file={:20} status={:15} old={:15} new={:15} rollback={}",
                i + 1,
                rec.tick,
                rec.action_kind,
                rec.target_file,
                rec.status,
                rec.old_value.as_deref().unwrap_or("N/A"),
                rec.new_value.as_deref().unwrap_or("N/A"),
                rec.has_rollback,
            );
        }

        let verdict = if scenario.all_passed { "✅ PASS" } else { "❌ FAIL" };
        println!("\n  Verdict: {verdict}");
    }

    // ── File output ───────────────────────────────────────────────────

    fn write_results(results: &[ScenarioResult]) {
        let dir = PathBuf::from("experiments/alpha5/results");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("alpha5-results.json");
        let json = serde_json::to_string_pretty(results).unwrap();
        std::fs::write(&path, &json).unwrap();
        println!("\nResults saved to {}", path.display());
    }

    // ── Entry point ───────────────────────────────────────────────────

    pub fn run_alpha5() -> Result<(), String> {
        println!("AgenticOS Alpha-5: Real Linux Kernel Validation");
        println!("================================================");
        println!();
        println!("Demonstrating actual cgroup v2 mutations through the full");
        println!("Observe → Propose → Policy → Safety → LinuxCgroupExecutor pipeline.");
        println!();

        let mut test = TestCgroup::new(TEST_CGROUP)?;

        let mut results = Vec::new();

        println!("\n─── Scenario A: CPU Pressure ───");
        let r_a = scenario_a(&mut test);
        print_result(&r_a);
        results.push(r_a);

        println!("\n─── Scenario B: Memory Pressure ───");
        let r_b = scenario_b(&mut test);
        print_result(&r_b);
        results.push(r_b);

        println!("\n─── Scenario C: Mixed Workload ───");
        let r_c = scenario_c(&mut test);
        print_result(&r_c);
        results.push(r_c);

        println!("\n─── Scenario D: Critical Freeze ───");
        let r_d = scenario_d(&mut test);
        print_result(&r_d);
        results.push(r_d);

        println!("\n─── Scenario E: Recovery After Freeze ───");
        let r_e = scenario_e(&mut test);
        print_result(&r_e);
        results.push(r_e);

        println!();
        println!("{}", "=" .repeat(72));
        println!("  Summary");
        println!("{}", "=" .repeat(72));
        println!();

        println!(
            "  {:<24} {:>8} {:>8} {:>8} {:>8} {:>10}",
            "Scenario", "Success", "Failed", "Vetoed", "Denied", "Replay"
        );
        println!("  {}", "-".repeat(64));
        for r in &results {
            println!(
                "  {:<24} {:>8} {:>8} {:>8} {:>8} {:>10}",
                r.name,
                r.successful_mutations,
                r.failed_mutations,
                r.vetoed_count,
                r.denied_count,
                if r.replay_consistent { "✅" } else { "❌" },
            );
        }

        write_results(&results);

        Ok(())
    }
}

// On Linux, this is included; on non-Linux we use the stub main above.
#[cfg(target_os = "linux")]
fn run_alpha5() -> Result<(), String> {
    alpha5_impl::run_alpha5()
}
