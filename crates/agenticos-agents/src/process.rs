use std::time::{SystemTime, UNIX_EPOCH};

use agenticos_domain::{
    ActionId, ActionKind, ActionRequest, ActionSafetyLevel, Agent, AgentId, AgentKind,
    CapabilitySet, Confidence, Observation, ObservationPayload, Proposal, ProposalId,
};

pub struct ProcessAgent {
    id: AgentId,
    /// CPU usage fraction threshold (0.0–1.0). If a cgroup's usage exceeds this,
    /// the agent proposes a cpu.max adjustment.
    cpu_threshold: f64,
    /// If nr_throttled exceeds this value, the agent proposes a cpu.weight increase.
    throttling_threshold: u64,
}

impl ProcessAgent {
    pub fn new(id: AgentId) -> Self {
        Self {
            id,
            cpu_threshold: 0.80,
            throttling_threshold: 5,
        }
    }

    pub fn with_thresholds(id: AgentId, cpu_threshold: f64, throttling_threshold: u64) -> Self {
        Self {
            id,
            cpu_threshold,
            throttling_threshold,
        }
    }

}

impl Agent for ProcessAgent {
    fn id(&self) -> AgentId {
        self.id.clone()
    }

    fn kind(&self) -> AgentKind {
        AgentKind::Process
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::default()
    }

    fn propose(&self, observations: &[Observation]) -> Vec<Proposal> {
        let mut proposals = Vec::new();
        let now = timestamp();

        // Collect cgroup observations for CPU analysis.
        let cgroup_obs: Vec<_> = observations
            .iter()
            .filter_map(|o| match &o.payload {
                ObservationPayload::Cgroup(cg) => Some((o, cg)),
                _ => None,
            })
            .collect();

        // Check each cgroup for CPU pressure.
        for (_obs, cg) in &cgroup_obs {
            let group = cg.cgroup_path.trim_start_matches("/sys/fs/cgroup/");

            // Rule 1: sustained CPU utilization above threshold
            // Estimate from cpu_usage_usec assuming 1 Hz sampling.
            if cg.cpu_usage_usec > (self.cpu_threshold * 1_000_000.0) as u64 {
                let quota = format!("{} 100000", (self.cpu_threshold * 100_000.0) as u64);
                proposals.push(Proposal {
                    id: ProposalId::new(),
                    agent_id: self.id.clone(),
                    created_at: now.clone(),
                    based_on: vec![],
                    requested_action: ActionRequest {
                        id: ActionId::new(),
                        kind: ActionKind::CgroupSetCpuMax {
                            group: group.to_owned(),
                            quota: quota.clone(),
                        },
                        safety_level: ActionSafetyLevel::MediumRisk,
                    },
                    rationale: format!(
                        "high CPU usage in '{}': {} usec, capping at {}",
                        group, cg.cpu_usage_usec, quota
                    ),
                    confidence: Confidence(0.85),
                });
            }

            // Rule 2: cgroup throttling detected
            if cg.cpu_nr_throttled > self.throttling_threshold {
                let weight = 100u64.min(cg.cpu_nr_throttled.saturating_mul(10));
                proposals.push(Proposal {
                    id: ProposalId::new(),
                    agent_id: self.id.clone(),
                    created_at: now.clone(),
                    based_on: vec![],
                    requested_action: ActionRequest {
                        id: ActionId::new(),
                        kind: ActionKind::CgroupSetCpuWeight {
                            group: group.to_owned(),
                            weight,
                        },
                        safety_level: ActionSafetyLevel::MediumRisk,
                    },
                    rationale: format!(
                        "throttling in '{}': {} events ({} usec), boosting cpu.weight to {}",
                        group, cg.cpu_nr_throttled, cg.cpu_throttled_usec, weight
                    ),
                    confidence: Confidence(0.8),
                });
            }
        }

        // Rule 3: excessive run queue growth — check CPU pressure
        for obs in observations {
            let cpu = match &obs.payload {
                ObservationPayload::Cpu(c) => c,
                _ => continue,
            };

            let pressure = cpu.pressure_some_avg10.unwrap_or(0.0);
            if pressure > 0.5 {
                proposals.push(Proposal {
                    id: ProposalId::new(),
                    agent_id: self.id.clone(),
                    created_at: now.clone(),
                    based_on: vec![],
                    requested_action: ActionRequest {
                        id: ActionId::new(),
                        kind: ActionKind::WorkloadClassifyRecommend {
                            group: "system".into(),
                            classification: format!(
                                "cpu_pressure_some={:.1} — consider isolating workload",
                                pressure
                            ),
                        },
                        safety_level: ActionSafetyLevel::LowRisk,
                    },
                    rationale: format!(
                        "system-wide CPU pressure some avg10 = {:.1}%, above 50% threshold",
                        pressure * 100.0
                    ),
                    confidence: Confidence(0.75),
                });
            }
        }

        proposals
    }
}

fn timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()),
        Err(_) => "0.000000000Z".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use agenticos_domain::{
        ActionKind, ActionSafetyLevel, DecisionOutcome, MetricCollection, Observation,
        ObservationId, ObservationSource,
    };
    use agenticos_application::AppError;
    use agenticos_executor::{ApprovedActionExecutor, DryRunExecutor};
    use agenticos_policy::{DefaultPolicyKernel, DeterministicPolicyKernel, PolicyInput};

    use super::*;

    /// Helper: build a cgroup observation with specific CPU stats.
    fn cgroup_obs(
        path: &str,
        usage_usec: u64,
        nr_throttled: u64,
        throttled_usec: u64,
    ) -> Observation {
        Observation {
            id: ObservationId::new(),
            source: ObservationSource::Cgroup,
            observed_at: "0.000000000Z".into(),
            collection_duration_ms: 5,
            payload: ObservationPayload::Cgroup(agenticos_domain::CgroupObservation {
                cgroup_path: path.to_owned(),
                memory_current_bytes: 0,
                memory_swap_current_bytes: 0,
                cpu_usage_usec: usage_usec,
                cpu_user_usec: usage_usec / 2,
                cpu_system_usec: usage_usec / 4,
                cpu_nr_throttled: nr_throttled,
                cpu_throttled_usec: throttled_usec,
                processes: 2,
            }),
        }
    }

    /// Helper: build a CPU pressure observation.
    fn cpu_pressure_obs(pressure_some: f64, nr_running: u64) -> Observation {
        Observation {
            id: ObservationId::new(),
            source: ObservationSource::Cpu,
            observed_at: "0.000000000Z".into(),
            collection_duration_ms: 5,
            payload: ObservationPayload::Cpu(agenticos_domain::CpuObservation {
                pressure_some_avg10: Some(pressure_some),
                pressure_full_avg10: None,
                nr_running: Some(nr_running),
            }),
        }
    }

    // ------------------------------------------------------------------
    // Rule tests
    // ------------------------------------------------------------------

    #[test]
    fn low_cpu_usage_yields_no_proposals() {
        let agent = ProcessAgent::new(AgentId::from("proc-agent"));
        let obs = cgroup_obs("/sys/fs/cgroup/agenticos/test", 100_000, 0, 0);
        let proposals = Agent::propose(&agent, &[obs]);
        assert!(proposals.is_empty());
    }

    #[test]
    fn high_cpu_usage_proposes_cpu_max() {
        let agent = ProcessAgent::new(AgentId::from("proc-agent"));
        let obs = cgroup_obs("/sys/fs/cgroup/agenticos/test", 900_000, 0, 0);
        let proposals = Agent::propose(&agent, &[obs]);
        assert_eq!(proposals.len(), 1);

        let prop = &proposals[0];
        assert_eq!(prop.agent_id, AgentId::from("proc-agent"));
        match &prop.requested_action.kind {
            ActionKind::CgroupSetCpuMax { group, quota } => {
                assert_eq!(group, "agenticos/test");
                assert_eq!(quota, "80000 100000");
            }
            other => panic!("expected CgroupSetCpuMax, got {other:?}"),
        }
    }

    #[test]
    fn throttling_detected_proposes_cpu_weight() {
        let agent = ProcessAgent::new(AgentId::from("proc-agent"));
        let obs = cgroup_obs("/sys/fs/cgroup/agenticos/test", 100_000, 10, 50_000);
        let proposals = Agent::propose(&agent, &[obs]);
        assert_eq!(proposals.len(), 1);

        let prop = &proposals[0];
        match &prop.requested_action.kind {
            ActionKind::CgroupSetCpuWeight { group, weight } => {
                assert_eq!(group, "agenticos/test");
                assert_eq!(*weight, 100);
            }
            other => panic!("expected CgroupSetCpuWeight, got {other:?}"),
        }
    }

    #[test]
    fn high_cpu_pressure_proposes_workload_classification() {
        let agent = ProcessAgent::new(AgentId::from("proc-agent"));
        let obs = cpu_pressure_obs(0.85, 8);
        let proposals = Agent::propose(&agent, &[obs]);
        assert_eq!(proposals.len(), 1);

        let prop = &proposals[0];
        assert_eq!(
            prop.requested_action.safety_level,
            ActionSafetyLevel::LowRisk
        );
        match &prop.requested_action.kind {
            ActionKind::WorkloadClassifyRecommend {
                group,
                classification,
            } => {
                assert_eq!(group, "system");
                assert!(classification.contains("cpu_pressure_some="));
            assert!(classification.contains("consider isolating workload"));
            }
            other => panic!("expected WorkloadClassifyRecommend, got {other:?}"),
        }
    }

    // ------------------------------------------------------------------
    // Full pipeline: ProcessAgent → Policy → Executor
    // ------------------------------------------------------------------

    #[test]
    fn process_agent_pipeline_cpu_max() -> Result<(), AppError> {
        let agent = ProcessAgent::new(AgentId::from("proc-agent"));
        let obs = cgroup_obs("/sys/fs/cgroup/agenticos/test", 900_000, 0, 0);
        let proposals = Agent::propose(&agent, &[obs]);
        assert_eq!(proposals.len(), 1);

        let kernel = DefaultPolicyKernel::benchmark();
        let input = PolicyInput {
            tick: 1,
            observations: vec![],
            proposals: proposals.clone(),
            incidents: vec![],
            prior_decisions: vec![],
            metrics: MetricCollection {
                source: "test".into(),
                samples: vec![],
            },
        };
        let decisions = kernel.evaluate_tick(&input)?;
        let decision = &decisions[0];
        assert_eq!(decision.outcome, DecisionOutcome::Approved);

        let approved = kernel
            .validate_action(&proposals[0], decision)?
            .expect("approved");
        let executor = DryRunExecutor::new();
        let result = executor.execute(approved)?;
        assert_eq!(result.status, agenticos_domain::ActionStatus::DryRun);

        Ok(())
    }

    #[test]
    fn process_agent_pipeline_workload_classify() -> Result<(), AppError> {
        let agent = ProcessAgent::new(AgentId::from("proc-agent"));
        let obs = cpu_pressure_obs(0.85, 8);
        let proposals = Agent::propose(&agent, &[obs]);
        assert_eq!(proposals.len(), 1);

        let kernel = DefaultPolicyKernel::benchmark();
        let input = PolicyInput {
            tick: 1,
            observations: vec![],
            proposals: proposals.clone(),
            incidents: vec![],
            prior_decisions: vec![],
            metrics: MetricCollection {
                source: "test".into(),
                samples: vec![],
            },
        };
        let decisions = kernel.evaluate_tick(&input)?;
        let decision = &decisions[0];
        assert_eq!(decision.outcome, DecisionOutcome::Approved);

        let approved = kernel
            .validate_action(&proposals[0], decision)?
            .expect("approved");
        let executor = DryRunExecutor::new();
        let result = executor.execute(approved)?;
        assert_eq!(result.status, agenticos_domain::ActionStatus::DryRun);

        Ok(())
    }

    // ------------------------------------------------------------------
    // Rule: CPU usage below threshold → no proposal
    // ------------------------------------------------------------------

    #[test]
    fn cpu_usage_below_threshold_no_proposal() {
        let agent = ProcessAgent::with_thresholds(AgentId::from("proc-agent"), 0.90, 5);
        let obs = cgroup_obs("/sys/fs/cgroup/test", 500_000, 0, 0);
        let proposals = Agent::propose(&agent, &[obs]);
        assert!(proposals.is_empty());
    }

    #[test]
    fn no_cgroup_observations_returns_empty() {
        let agent = ProcessAgent::new(AgentId::from("proc-agent"));
        let obs = Observation {
            id: ObservationId::new(),
            source: ObservationSource::Memory,
            observed_at: "0.000000000Z".into(),
            collection_duration_ms: 0,
            payload: ObservationPayload::Empty,
        };
        let proposals = Agent::propose(&agent, &[obs]);
        assert!(proposals.is_empty());
    }
}
