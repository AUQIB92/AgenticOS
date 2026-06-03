use std::sync::Mutex;

use agenticos_domain::{
    ActionId, ActionKind, ActionRequest, ActionSafetyLevel, Agent, AgentId, AgentKind,
    CapabilitySet, CapabilityScope, Confidence, Observation, Proposal, ProposalId,
    Recommendation, WorkloadClass,
};

pub struct RecommendationConsumerAgent {
    id: AgentId,
    pending: Mutex<Vec<Recommendation>>,
    consumed_count: Mutex<u64>,
    ignored_count: Mutex<u64>,
    converted_count: Mutex<u64>,
}

impl RecommendationConsumerAgent {
    pub fn new(id: AgentId) -> Self {
        Self {
            id,
            pending: Mutex::new(Vec::new()),
            consumed_count: Mutex::new(0),
            ignored_count: Mutex::new(0),
            converted_count: Mutex::new(0),
        }
    }

    pub fn consume_recommendation(&self, rec: Recommendation) {
        let mut pending = self.pending.lock().unwrap();
        pending.push(rec);
    }

    pub fn recommendations_consumed(&self) -> u64 {
        *self.consumed_count.lock().unwrap()
    }

    pub fn recommendations_ignored(&self) -> u64 {
        *self.ignored_count.lock().unwrap()
    }

    pub fn recommendations_converted(&self) -> u64 {
        *self.converted_count.lock().unwrap()
    }

    fn drain_pending(&self) -> Vec<Recommendation> {
        let mut pending = self.pending.lock().unwrap();
        std::mem::take(&mut *pending)
    }

    fn recommendation_to_proposals(rec: &Recommendation) -> Vec<Proposal> {
        let class = extract_class_from_summary(&rec.summary);
        Self::class_to_proposals(&class, rec)
    }

    fn class_to_proposals(class: &WorkloadClass, rec: &Recommendation) -> Vec<Proposal> {
        let (kind, rationale_suffix) = match class {
            WorkloadClass::Database => (
                ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/classified/database".into(),
                    weight: 200,
                },
                "Database classified workload → set cpu.weight=200",
            ),
            WorkloadClass::Interactive => (
                ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/classified/interactive".into(),
                    weight: 150,
                },
                "Interactive classified workload → set cpu.weight=150",
            ),
            WorkloadClass::Batch => (
                ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/classified/batch".into(),
                    weight: 50,
                },
                "Batch classified workload → set cpu.weight=50",
            ),
            WorkloadClass::Build => (
                ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/classified/build".into(),
                    weight: 300,
                },
                "Build classified workload → set cpu.weight=300",
            ),
            WorkloadClass::SystemService => (
                ActionKind::CgroupSetCpuWeight {
                    group: "agenticos/classified/system-service".into(),
                    weight: 100,
                },
                "SystemService classified workload → set cpu.weight=100",
            ),
            WorkloadClass::Unknown => return vec![],
        };

        vec![Proposal {
            id: ProposalId::new(),
            agent_id: rec.source_agent.clone(),
            created_at: rec.timestamp.clone(),
            based_on: vec![],
            requested_action: ActionRequest {
                id: ActionId::new(),
                kind,
                safety_level: ActionSafetyLevel::LowRisk,
            },
            rationale: format!("Recommendation {}: {}", rec.id, rationale_suffix),
            confidence: Confidence(rec.confidence as f32),
        }]
    }
}

fn extract_class_from_summary(summary: &str) -> WorkloadClass {
    if summary.contains("Database") {
        WorkloadClass::Database
    } else if summary.contains("Interactive") {
        WorkloadClass::Interactive
    } else if summary.contains("Build") {
        WorkloadClass::Build
    } else if summary.contains("Batch") {
        WorkloadClass::Batch
    } else if summary.contains("SystemService") {
        WorkloadClass::SystemService
    } else {
        WorkloadClass::Unknown
    }
}

impl Agent for RecommendationConsumerAgent {
    fn id(&self) -> AgentId {
        self.id.clone()
    }

    fn kind(&self) -> AgentKind {
        AgentKind::Custom("RecommendationConsumer".into())
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet {
            capabilities: vec![agenticos_domain::Capability {
                name: "recommendation-to-proposal".into(),
                scope: CapabilityScope::ProposalOnly,
            }],
        }
    }

    fn propose(&self, _observations: &[Observation]) -> Vec<Proposal> {
        let pending = self.drain_pending();
        let mut all_proposals = Vec::new();

        for rec in &pending {
            *self.consumed_count.lock().unwrap() += 1;
            let proposals = Self::recommendation_to_proposals(rec);
            if proposals.is_empty() {
                *self.ignored_count.lock().unwrap() += 1;
            } else {
                *self.converted_count.lock().unwrap() += proposals.len() as u64;
                all_proposals.extend(proposals);
            }
        }

        all_proposals
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{EventPayload, TraceId};
    use agenticos_bus::{InMemoryTraceStore, Topic, TraceStore};
    use agenticos_policy::{DefaultPolicyKernel, DeterministicPolicyKernel, PolicyInput};

    fn make_rec(summary: &str, confidence: f64) -> Recommendation {
        Recommendation::new(
            AgentId::from("classifier"),
            confidence,
            summary,
            "test reasoning",
        )
    }

    // ── Mapping Rule Tests ──────────────────────────────────────────

    #[test]
    fn database_generates_proposal() {
        let rec = make_rec("Workload classified as Database", 0.92);
        let proposals = RecommendationConsumerAgent::recommendation_to_proposals(&rec);
        assert_eq!(proposals.len(), 1);
        let p = &proposals[0];
        assert!(p.rationale.contains("Database"));
        assert!(p.rationale.contains(&rec.id.to_string()));
        match &p.requested_action.kind {
            ActionKind::CgroupSetCpuWeight { weight, .. } => {
                assert_eq!(*weight, 200);
            }
            _ => panic!("expected CgroupSetCpuWeight"),
        }
        assert_eq!(p.requested_action.safety_level, ActionSafetyLevel::LowRisk);
    }

    #[test]
    fn interactive_generates_proposal() {
        let rec = make_rec("Workload classified as Interactive", 0.85);
        let proposals = RecommendationConsumerAgent::recommendation_to_proposals(&rec);
        assert_eq!(proposals.len(), 1);
        match &proposals[0].requested_action.kind {
            ActionKind::CgroupSetCpuWeight { weight, .. } => {
                assert_eq!(*weight, 150);
            }
            _ => panic!("expected CgroupSetCpuWeight"),
        }
    }

    #[test]
    fn batch_generates_proposal() {
        let rec = make_rec("Workload classified as Batch", 0.75);
        let proposals = RecommendationConsumerAgent::recommendation_to_proposals(&rec);
        assert_eq!(proposals.len(), 1);
        match &proposals[0].requested_action.kind {
            ActionKind::CgroupSetCpuWeight { weight, .. } => {
                assert_eq!(*weight, 50);
            }
            _ => panic!("expected CgroupSetCpuWeight"),
        }
    }

    #[test]
    fn build_generates_proposal() {
        let rec = make_rec("Workload classified as Build", 0.88);
        let proposals = RecommendationConsumerAgent::recommendation_to_proposals(&rec);
        assert_eq!(proposals.len(), 1);
        match &proposals[0].requested_action.kind {
            ActionKind::CgroupSetCpuWeight { weight, .. } => {
                assert_eq!(*weight, 300);
            }
            _ => panic!("expected CgroupSetCpuWeight"),
        }
    }

    #[test]
    fn system_service_generates_proposal() {
        let rec = make_rec("Workload classified as SystemService", 0.80);
        let proposals = RecommendationConsumerAgent::recommendation_to_proposals(&rec);
        assert_eq!(proposals.len(), 1);
        match &proposals[0].requested_action.kind {
            ActionKind::CgroupSetCpuWeight { weight, .. } => {
                assert_eq!(*weight, 100);
            }
            _ => panic!("expected CgroupSetCpuWeight"),
        }
    }

    #[test]
    fn unknown_generates_no_proposal() {
        let rec = make_rec("Workload classified as Unknown", 0.50);
        let proposals = RecommendationConsumerAgent::recommendation_to_proposals(&rec);
        assert!(proposals.is_empty());
    }

    #[test]
    fn proposal_confidence_matches_recommendation() {
        let rec = make_rec("Workload classified as Database", 0.92);
        let proposals = RecommendationConsumerAgent::recommendation_to_proposals(&rec);
        assert!((proposals[0].confidence.0 - 0.92).abs() < 0.001);
    }

    // ── Agent Integration Tests ───────────────────────────────────

    #[test]
    fn consume_and_propose_round_trip() {
        let agent = RecommendationConsumerAgent::new(AgentId::from("bridge"));

        let rec = make_rec("Workload classified as Database", 0.92);
        agent.consume_recommendation(rec);

        assert_eq!(agent.recommendations_consumed(), 0);
        assert_eq!(agent.recommendations_converted(), 0);

        let proposals = agent.propose(&[]);

        assert_eq!(proposals.len(), 1);
        assert_eq!(agent.recommendations_consumed(), 1);
        assert_eq!(agent.recommendations_converted(), 1);
        assert_eq!(agent.recommendations_ignored(), 0);
    }

    #[test]
    fn unknown_recommendation_ignored() {
        let agent = RecommendationConsumerAgent::new(AgentId::from("bridge"));

        let rec = make_rec("Workload classified as Unknown", 0.50);
        agent.consume_recommendation(rec);

        let proposals = agent.propose(&[]);
        assert!(proposals.is_empty());
        assert_eq!(agent.recommendations_consumed(), 1);
        assert_eq!(agent.recommendations_converted(), 0);
        assert_eq!(agent.recommendations_ignored(), 1);
    }

    #[test]
    fn multiple_recommendations_batched() {
        let agent = RecommendationConsumerAgent::new(AgentId::from("bridge"));

        agent.consume_recommendation(make_rec("Workload classified as Database", 0.92));
        agent.consume_recommendation(make_rec("Workload classified as Build", 0.88));
        agent.consume_recommendation(make_rec("Workload classified as Unknown", 0.50));
        agent.consume_recommendation(make_rec("Workload classified as Interactive", 0.85));

        let proposals = agent.propose(&[]);
        assert_eq!(proposals.len(), 3);
        assert_eq!(agent.recommendations_consumed(), 4);
        assert_eq!(agent.recommendations_converted(), 3);
        assert_eq!(agent.recommendations_ignored(), 1);

        // Verify weights in order
        match &proposals[0].requested_action.kind {
            ActionKind::CgroupSetCpuWeight { weight, .. } => assert_eq!(*weight, 200),
            _ => panic!("expected CgroupSetCpuWeight"),
        }
        match &proposals[1].requested_action.kind {
            ActionKind::CgroupSetCpuWeight { weight, .. } => assert_eq!(*weight, 300),
            _ => panic!("expected CgroupSetCpuWeight"),
        }
        match &proposals[2].requested_action.kind {
            ActionKind::CgroupSetCpuWeight { weight, .. } => assert_eq!(*weight, 150),
            _ => panic!("expected CgroupSetCpuWeight"),
        }
    }

    #[test]
    fn propose_drains_pending() {
        let agent = RecommendationConsumerAgent::new(AgentId::from("bridge"));

        agent.consume_recommendation(make_rec("Workload classified as Database", 0.92));
        let first = agent.propose(&[]);
        assert_eq!(first.len(), 1);

        // Second propose should be empty (buffer drained)
        let second = agent.propose(&[]);
        assert!(second.is_empty());
        // Consumed count should remain 1 (not incremented again)
        assert_eq!(agent.recommendations_consumed(), 1);
    }

    // ── Trace Persistence Tests ─────────────────────────────────────

    #[test]
    fn recommendation_and_proposal_in_trace_store() {
        use agenticos_domain::EventEnvelope;

        let store = InMemoryTraceStore::new();
        let trace_id = TraceId::new();

        // Store the recommendation
        let rec = make_rec("Workload classified as Database", 0.92);
        let rec_env = EventEnvelope {
            id: agenticos_domain::MessageId::new(),
            trace_id: trace_id.clone(),
            causation_id: None,
            topic: Topic::new("recommendations.classifier"),
            timestamp: "2026-06-03T00:00:00Z".into(),
            payload: EventPayload::Recommendation(rec.clone()),
        };
        store.append(rec_env).unwrap();

        // Convert to proposal and store it
        let proposals = RecommendationConsumerAgent::recommendation_to_proposals(&rec);
        assert_eq!(proposals.len(), 1);
        let prop_env = EventEnvelope {
            id: agenticos_domain::MessageId::new(),
            trace_id: trace_id.clone(),
            causation_id: None,
            topic: Topic::new("proposals.bridge"),
            timestamp: "2026-06-03T00:00:01Z".into(),
            payload: EventPayload::Proposal(proposals[0].clone()),
        };
        store.append(prop_env).unwrap();

        // Verify replay contains both events
        let events = store.replay(trace_id).unwrap();
        assert_eq!(events.len(), 2);

        // First event: Recommendation
        match &events[0].payload {
            EventPayload::Recommendation(r) => {
                assert_eq!(r.summary, "Workload classified as Database");
            }
            _ => panic!("expected Recommendation"),
        }

        // Second event: Proposal with causation link via rationale
        match &events[1].payload {
            EventPayload::Proposal(p) => {
                assert!(p.rationale.contains(&rec.id.to_string()));
                assert!(p.rationale.contains("Database"));
                match &p.requested_action.kind {
                    ActionKind::CgroupSetCpuWeight { weight, .. } => {
                        assert_eq!(*weight, 200);
                    }
                    _ => panic!("expected CgroupSetCpuWeight"),
                }
            }
            _ => panic!("expected Proposal"),
        }
    }

    // ── Agent Trait Tests ───────────────────────────────────────────

    #[test]
    fn agent_trait_id_and_kind() {
        let agent = RecommendationConsumerAgent::new(AgentId::from("bridge-1"));
        assert_eq!(agent.id().as_str(), "bridge-1");
        assert_eq!(agent.kind(), AgentKind::Custom("RecommendationConsumer".to_string()));
        assert!(!agent.capabilities().capabilities.is_empty());
    }

    // ── Policy Integration Tests ────────────────────────────────────

    #[test]
    fn proposal_passes_through_policy_kernel() {
        use agenticos_domain::{DecisionOutcome, MetricCollection};

        let agent = RecommendationConsumerAgent::new(AgentId::from("bridge"));
        agent.consume_recommendation(make_rec("Workload classified as Database", 0.92));
        let proposals = agent.propose(&[]);
        assert_eq!(proposals.len(), 1);

        let policy = DefaultPolicyKernel::benchmark();
        let input = PolicyInput {
            tick: 1,
            observations: vec![],
            proposals,
            incidents: vec![],
            prior_decisions: vec![],
            metrics: MetricCollection {
                source: "test".into(),
                samples: vec![],
            },
        };

        let decisions = policy.evaluate_tick(&input).unwrap();
        assert_eq!(decisions.len(), 1);

        match &decisions[0].outcome {
            DecisionOutcome::Approved => {}
            DecisionOutcome::Denied { reason } => {
                panic!("proposal should be approved, got Denied({reason:?})");
            }
            DecisionOutcome::RequiresApproval => {
                panic!("proposal should be approved, got RequiresApproval");
            }
        }
    }

    #[test]
    fn multiple_proposals_through_policy() {
        use agenticos_domain::{DecisionOutcome, MetricCollection};

        let agent = RecommendationConsumerAgent::new(AgentId::from("bridge"));
        agent.consume_recommendation(make_rec("Workload classified as Database", 0.92));
        agent.consume_recommendation(make_rec("Workload classified as Build", 0.88));
        agent.consume_recommendation(make_rec("Workload classified as Batch", 0.75));
        agent.consume_recommendation(make_rec("Workload classified as Interactive", 0.85));
        let proposals = agent.propose(&[]);
        assert_eq!(proposals.len(), 4);

        let policy = DefaultPolicyKernel::benchmark();
        let input = PolicyInput {
            tick: 1,
            observations: vec![],
            proposals,
            incidents: vec![],
            prior_decisions: vec![],
            metrics: MetricCollection {
                source: "test".into(),
                samples: vec![],
            },
        };

        let decisions = policy.evaluate_tick(&input).unwrap();
        assert_eq!(decisions.len(), 4);
        for d in &decisions {
            assert!(
                matches!(d.outcome, DecisionOutcome::Approved),
                "expected all proposals approved, got {:?}",
                d.outcome
            );
        }
    }

    // ── Safety Integration Tests ────────────────────────────────────

    #[test]
    fn proposal_passes_through_safety_governor() {
        use agenticos_domain::MetricCollection;
        use agenticos_safety::{DefaultSafetyGovernor, SafetyConfig, SafetyInput};

        let agent = RecommendationConsumerAgent::new(AgentId::from("bridge"));
        agent.consume_recommendation(make_rec("Workload classified as Database", 0.92));
        let proposals = agent.propose(&[]);
        assert_eq!(proposals.len(), 1);

        let policy = DefaultPolicyKernel::benchmark();
        let input = PolicyInput {
            tick: 1,
            observations: vec![],
            proposals,
            incidents: vec![],
            prior_decisions: vec![],
            metrics: MetricCollection {
                source: "test".into(),
                samples: vec![],
            },
        };
        let decisions = policy.evaluate_tick(&input).unwrap();

        let governor = DefaultSafetyGovernor::new(SafetyConfig {
            max_cpu_weight: 1000,
            max_memory_bytes: None,
            veto_on_security_incidents: true,
        });

        let safety_input = SafetyInput {
            policy_input: &input,
            decisions: &decisions,
        };

        let output = governor.evaluate(safety_input).unwrap();
        assert_eq!(output.approved.len(), 1, "proposal should be approved by safety");
        assert!(output.vetoes.is_empty(), "no vetoes expected");
    }

    #[test]
    fn proposal_within_resource_limits() {
        use agenticos_domain::MetricCollection;
        use agenticos_safety::{DefaultSafetyGovernor, SafetyConfig, SafetyInput};

        let agent = RecommendationConsumerAgent::new(AgentId::from("bridge"));
        agent.consume_recommendation(make_rec("Workload classified as Build", 0.88));
        let proposals = agent.propose(&[]);

        let policy = DefaultPolicyKernel::benchmark();
        let input = PolicyInput {
            tick: 1,
            observations: vec![],
            proposals,
            incidents: vec![],
            prior_decisions: vec![],
            metrics: MetricCollection {
                source: "test".into(),
                samples: vec![],
            },
        };
        let decisions = policy.evaluate_tick(&input).unwrap();

        let governor = DefaultSafetyGovernor::new(SafetyConfig {
            max_cpu_weight: 500,
            max_memory_bytes: None,
            veto_on_security_incidents: true,
        });

        let safety_input = SafetyInput {
            policy_input: &input,
            decisions: &decisions,
        };

        let output = governor.evaluate(safety_input).unwrap();
        assert_eq!(output.approved.len(), 1, "weight=300 should be within limit of 500");
    }

    #[test]
    fn agent_satisfies_send_sync() {
        // Compile-time check: Agent trait requires Send + Sync
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RecommendationConsumerAgent>();
    }
}
