use std::time::{SystemTime, UNIX_EPOCH};

use agenticos_domain::{
    ActionId, ActionKind, ActionRequest, ActionSafetyLevel, Agent, AgentId, AgentKind,
    CapabilitySet, Confidence, Observation, ObservationPayload, Proposal, ProposalId,
};

pub struct MemoryAgent {
    id: AgentId,
    /// Fraction of total memory that triggers a proposal (0.0 – 1.0).
    threshold: f64,
}

impl MemoryAgent {
    pub fn new(id: AgentId) -> Self {
        Self {
            id,
            threshold: 0.80,
        }
    }

    pub fn with_threshold(id: AgentId, threshold: f64) -> Self {
        Self { id, threshold }
    }
}

impl Agent for MemoryAgent {
    fn id(&self) -> AgentId {
        self.id.clone()
    }

    fn kind(&self) -> AgentKind {
        AgentKind::Memory
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::default()
    }

    fn propose(&self, observations: &[Observation]) -> Vec<Proposal> {
        let mut proposals = Vec::new();
        let now = timestamp();

        for obs in observations {
            let mem = match &obs.payload {
                ObservationPayload::Memory(m) => m,
                _ => continue,
            };

            if mem.total_bytes == 0 {
                continue;
            }

            let usage_pct = mem.used_bytes as f64 / mem.total_bytes as f64;

            if usage_pct > self.threshold {
                let new_max = (mem.used_bytes as f64 * 1.2) as u64;

                proposals.push(Proposal {
                    id: ProposalId::new(),
                    agent_id: self.id.clone(),
                    created_at: now.clone(),
                    based_on: vec![obs.id.clone()],
                    requested_action: ActionRequest {
                        id: ActionId::new(),
                        kind: ActionKind::CgroupSetMemoryMax {
                            group: "agenticos".into(),
                            bytes: new_max,
                        },
                        safety_level: ActionSafetyLevel::MediumRisk,
                    },
                    rationale: format!(
                        "memory at {:.1}% ({}B / {}B), exceeding {:.0}% threshold",
                        usage_pct * 100.0,
                        mem.used_bytes,
                        mem.total_bytes,
                        self.threshold * 100.0,
                    ),
                    confidence: Confidence(0.9),
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
    use std::time::{SystemTime, UNIX_EPOCH};

    use agenticos_domain::{
        ActionKind, ActionSafetyLevel, DecisionOutcome, MemoryObservation, MetricCollection,
        Observation, ObservationId, ObservationPayload, ObservationSource,
    };
    use agenticos_application::AppError;
    use agenticos_executor::{ApprovedActionExecutor, DryRunExecutor};
    use agenticos_policy::{DefaultPolicyKernel, DeterministicPolicyKernel, PolicyInput};

    use super::*;

    /// Construct a memory observation with a given usage ratio.
    fn mem_obs(total_bytes: u64, used_bytes: u64) -> Observation {
        Observation {
            id: ObservationId::new(),
            source: ObservationSource::Memory,
            observed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()))
                .unwrap_or_else(|_| "0.000000000Z".to_owned()),
            collection_duration_ms: 0,
            payload: ObservationPayload::Memory(MemoryObservation {
                total_bytes,
                available_bytes: total_bytes - used_bytes,
                used_bytes,
                swap_total_bytes: 0,
                swap_used_bytes: 0,
                pressure_some_avg10: None,
                pressure_full_avg10: None,
            }),
        }
    }

    // ------------------------------------------------------------------
    // Agent rule tests
    // ------------------------------------------------------------------

    #[test]
    fn below_threshold_produces_no_proposal() {
        let agent = MemoryAgent::new(AgentId::from("mem-agent"));
        let obs = mem_obs(100, 50);
        let proposals = Agent::propose(&agent, &[obs]);
        assert!(proposals.is_empty());
    }

    #[test]
    fn above_threshold_produces_proposal() {
        let agent = MemoryAgent::new(AgentId::from("mem-agent"));
        let obs = mem_obs(100, 90);
        let proposals = Agent::propose(&agent, &[obs]);
        assert_eq!(proposals.len(), 1);

        let prop = &proposals[0];
        assert_eq!(prop.agent_id, AgentId::from("mem-agent"));
        assert_eq!(
            prop.requested_action.safety_level,
            ActionSafetyLevel::MediumRisk
        );

        match &prop.requested_action.kind {
            ActionKind::CgroupSetMemoryMax { group, bytes } => {
                assert_eq!(group, "agenticos");
                assert_eq!(*bytes, 108);
            }
            other => panic!("expected CgroupSetMemoryMax, got {other:?}"),
        }
    }

    #[test]
    fn multiple_observations_only_proposes_on_high_usage() {
        let agent = MemoryAgent::new(AgentId::from("mem-agent"));
        let obs_low = mem_obs(100, 30);
        let obs_high = mem_obs(100, 95);

        let proposals = Agent::propose(&agent, &[obs_low, obs_high]);
        assert_eq!(proposals.len(), 1);
    }

    #[test]
    fn non_memory_observation_is_ignored() {
        let agent = MemoryAgent::new(AgentId::from("mem-agent"));
        let obs = Observation {
            id: ObservationId::new(),
            source: ObservationSource::Process,
            observed_at: "0.000000000Z".into(),
            collection_duration_ms: 0,
            payload: ObservationPayload::Empty,
        };
        let proposals = Agent::propose(&agent, &[obs]);
        assert!(proposals.is_empty());
    }

    // ------------------------------------------------------------------
    // Full pipeline: Observe → Agent → Policy → Executor → ActionResult
    // ------------------------------------------------------------------

    #[test]
    fn pipeline_observe_to_action_result() -> Result<(), AppError> {
        let obs = mem_obs(100, 90);

        let agent = MemoryAgent::new(AgentId::from("mem-agent"));
        let proposals = Agent::propose(&agent, &[obs]);
        assert_eq!(proposals.len(), 1);
        let proposal = &proposals[0];

        let kernel = DefaultPolicyKernel::benchmark();
        let input = PolicyInput {
            tick: 1,
            observations: vec![],
            proposals: vec![proposal.clone()],
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
            .validate_action(proposal, decision)?
            .expect("approved decision should yield an ApprovedAction");

        let executor = DryRunExecutor::new();
        let result = executor.execute(approved)?;
        assert_eq!(result.status, agenticos_domain::ActionStatus::DryRun);
        assert!(!result.executed_at.is_empty());

        println!(
            "PIPELINE OK: observation → proposal → approved → {:?} ({})",
            result.status, result.message
        );

        Ok(())
    }
}
