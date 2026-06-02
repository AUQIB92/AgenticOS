use std::time::{SystemTime, UNIX_EPOCH};

use agenticos_agents::{DummyAgentA, DummyAgentB};
use agenticos_application::AppError;
use agenticos_bus::TraceStore;
use agenticos_domain::{
    DecisionOutcome, MemoryObservation, Observation, ObservationId, ObservationPayload,
    ObservationSource, TraceId,
};
use agenticos_executor::{ApprovedActionExecutor, DryRunExecutor};
use agenticos_policy::{DefaultPolicyKernel, DeterministicPolicyKernel};
use agenticos_runtime::{AgentRuntime, InMemoryAgentRuntime};

/// Build a memory observation with given total/used.
fn mem_obs(total_bytes: u64, used_bytes: u64) -> Observation {
    Observation {
        id: ObservationId::new(),
        source: ObservationSource::Memory,
        observed_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()))
            .unwrap_or_else(|_| "0.000000000Z".to_owned()),
        collection_duration_ms: 5,
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

// -----------------------------------------------------------------------
// 1. Concurrent proposals — two agents both fire from the same observation
// -----------------------------------------------------------------------
#[test]
fn two_dummy_agents_produce_two_proposals_from_one_observation() -> Result<(), AppError> {
    let runtime = InMemoryAgentRuntime::new();
    runtime.register(Box::new(DummyAgentA::new("dummy-a".into())))?;
    runtime.register(Box::new(DummyAgentB::new("dummy-b".into())))?;

    let obs = mem_obs(100, 80);
    let proposals = runtime.collect_proposals(&[obs])?;

    assert_eq!(proposals.len(), 2, "both agents should propose");
    assert!(proposals.iter().any(|p| p.agent_id.as_str() == "dummy-a"));
    assert!(proposals.iter().any(|p| p.agent_id.as_str() == "dummy-b"));
    Ok(())
}

// -----------------------------------------------------------------------
// 2. Proposal ordering — insertion order is preserved when both fire
// -----------------------------------------------------------------------
#[test]
fn proposals_are_returned_in_registration_order() -> Result<(), AppError> {
    let runtime = InMemoryAgentRuntime::new();
    runtime.register(Box::new(DummyAgentA::new("dummy-a".into())))?;
    runtime.register(Box::new(DummyAgentB::new("dummy-b".into())))?;

    let obs = mem_obs(100, 80);
    let proposals = runtime.collect_proposals(&[obs])?;

    assert_eq!(proposals.len(), 2);
    assert_eq!(
        proposals[0].agent_id.as_str(),
        "dummy-a",
        "first registered = first returned"
    );
    assert_eq!(
        proposals[1].agent_id.as_str(),
        "dummy-b",
        "second registered = second returned"
    );
    Ok(())
}

// -----------------------------------------------------------------------
// 3. Trace integrity — all pipeline events end up in the trace store
// -----------------------------------------------------------------------
#[test]
fn full_pipeline_traces_all_events() -> Result<(), AppError> {
    let runtime = InMemoryAgentRuntime::new();
    runtime.register(Box::new(DummyAgentA::new("dummy-a".into())))?;
    runtime.register(Box::new(DummyAgentB::new("dummy-b".into())))?;

    let trace_store = agenticos_bus::InMemoryTraceStore::new();
    let trace_id = TraceId::from("pipeline-test-3");
    let policy = DefaultPolicyKernel::benchmark();
    let executor = DryRunExecutor::new();

    let obs = mem_obs(100, 85);
    let proposals = runtime.collect_proposals(&[obs])?;

    for prop in &proposals {
        trace_store.append(agenticos_domain::EventEnvelope::new(
            agenticos_domain::Topic::new("proposals.test"),
            trace_id.clone(),
            agenticos_domain::EventPayload::Proposal(prop.clone()),
        ))?;

        let input = agenticos_policy::PolicyInput {
            tick: 1,
            observations: vec![],
            proposals: vec![prop.clone()],
            incidents: vec![],
            prior_decisions: vec![],
            metrics: agenticos_domain::MetricCollection {
                source: "test".into(),
                samples: vec![],
            },
        };
        let decisions = policy.evaluate_tick(&input)?;
        let decision = &decisions[0];
        trace_store.append(agenticos_domain::EventEnvelope::new(
            agenticos_domain::Topic::new("decisions.test"),
            trace_id.clone(),
            agenticos_domain::EventPayload::Decision(decision.clone()),
        ))?;

        if let DecisionOutcome::Approved = &decision.outcome {
            let approved = agenticos_domain::ApprovedAction {
                request: prop.requested_action.clone(),
                decision_id: decision.id.clone(),
            };
            let result = executor.execute(approved)?;
            trace_store.append(agenticos_domain::EventEnvelope::new(
                agenticos_domain::Topic::new("results.test"),
                trace_id.clone(),
                agenticos_domain::EventPayload::ActionResult(result),
            ))?;
        }
    }

    let replayed = trace_store.replay(trace_id)?;
    assert_eq!(replayed.len(), 6, "all 6 pipeline events should be traced");
    Ok(())
}

// -----------------------------------------------------------------------
// 4. Policy arbitration — benchmark policy approves both medium-risk
// -----------------------------------------------------------------------
#[test]
fn benchmark_policy_arbitrates_multiple_agents() -> Result<(), AppError> {
    let runtime = InMemoryAgentRuntime::new();
    runtime.register(Box::new(DummyAgentA::new("dummy-a".into())))?;
    runtime.register(Box::new(DummyAgentB::new("dummy-b".into())))?;

    let policy = DefaultPolicyKernel::benchmark();
    let executor = DryRunExecutor::new();

    let obs = mem_obs(100, 90);
    let proposals = runtime.collect_proposals(&[obs])?;

    let mut approved_count = 0u64;
    let mut denied_count = 0u64;

    let input = agenticos_policy::PolicyInput {
        tick: 1,
        observations: vec![],
        proposals: proposals.clone(),
        incidents: vec![],
        prior_decisions: vec![],
        metrics: agenticos_domain::MetricCollection {
            source: "test".into(),
            samples: vec![],
        },
    };
    let decisions = policy.evaluate_tick(&input)?;

    for (prop, decision) in proposals.iter().zip(decisions.iter()) {
        match &decision.outcome {
            DecisionOutcome::Approved => {
                let approved = agenticos_domain::ApprovedAction {
                    request: prop.requested_action.clone(),
                    decision_id: decision.id.clone(),
                };
                let result = executor.execute(approved)?;
                assert_eq!(result.status, agenticos_domain::ActionStatus::DryRun);
                approved_count += 1;
            }
            DecisionOutcome::Denied { .. } | DecisionOutcome::RequiresApproval => {
                denied_count += 1;
            }
        }
    }

    assert_eq!(approved_count, 2);
    assert_eq!(denied_count, 0);
    Ok(())
}

// -----------------------------------------------------------------------
// 5. Policy arbitration — safe-local denies all mutations
// -----------------------------------------------------------------------
#[test]
fn safe_local_policy_denies_all_mutations() -> Result<(), AppError> {
    let runtime = InMemoryAgentRuntime::new();
    runtime.register(Box::new(DummyAgentA::new("dummy-a".into())))?;
    runtime.register(Box::new(DummyAgentB::new("dummy-b".into())))?;

    let policy = DefaultPolicyKernel::safe_local();

    let obs = mem_obs(100, 90);
    let proposals = runtime.collect_proposals(&[obs])?;

    let mut approved_count = 0u64;
    let mut denied_count = 0u64;

    let input = agenticos_policy::PolicyInput {
        tick: 1,
        observations: vec![],
        proposals: proposals.clone(),
        incidents: vec![],
        prior_decisions: vec![],
        metrics: agenticos_domain::MetricCollection {
            source: "test".into(),
            samples: vec![],
        },
    };
    let decisions = policy.evaluate_tick(&input)?;

    for decision in &decisions {
        match &decision.outcome {
            DecisionOutcome::Approved => approved_count += 1,
            DecisionOutcome::Denied { .. } | DecisionOutcome::RequiresApproval => {
                denied_count += 1
            }
        }
    }

    assert_eq!(approved_count, 0);
    assert_eq!(denied_count, 2);
    Ok(())
}

// -----------------------------------------------------------------------
// PolicyInput tick evaluation tests
// -----------------------------------------------------------------------
mod tick_eval {
    use agenticos_policy::PolicyInput;

    use super::*;

    fn empty_metrics() -> agenticos_domain::MetricCollection {
        agenticos_domain::MetricCollection {
            source: "test".into(),
            samples: vec![],
        }
    }

    #[test]
    fn evaluate_tick_two_proposals_benchmark() -> Result<(), AppError> {
        let runtime = InMemoryAgentRuntime::new();
        runtime.register(Box::new(DummyAgentA::new("dummy-a".into())))?;
        runtime.register(Box::new(DummyAgentB::new("dummy-b".into())))?;

        let policy = DefaultPolicyKernel::benchmark();
        let obs = mem_obs(100, 90);
        let proposals = runtime.collect_proposals(&[obs])?;

        let input = PolicyInput {
            tick: 1,
            observations: vec![],
            proposals: proposals.clone(),
            incidents: vec![],
            prior_decisions: vec![],
            metrics: empty_metrics(),
        };
        let decisions = policy.evaluate_tick(&input)?;

        assert_eq!(decisions.len(), 2);
        for d in &decisions {
            assert_eq!(d.outcome, DecisionOutcome::Approved);
        }
        Ok(())
    }

    #[test]
    fn evaluate_tick_two_proposals_safe_local() -> Result<(), AppError> {
        let runtime = InMemoryAgentRuntime::new();
        runtime.register(Box::new(DummyAgentA::new("dummy-a".into())))?;
        runtime.register(Box::new(DummyAgentB::new("dummy-b".into())))?;

        let policy = DefaultPolicyKernel::safe_local();
        let obs = mem_obs(100, 90);
        let proposals = runtime.collect_proposals(&[obs])?;

        let input = PolicyInput {
            tick: 1,
            observations: vec![],
            proposals: proposals.clone(),
            incidents: vec![],
            prior_decisions: vec![],
            metrics: empty_metrics(),
        };
        let decisions = policy.evaluate_tick(&input)?;

        assert_eq!(decisions.len(), 2);
        for d in &decisions {
            assert!(matches!(d.outcome, DecisionOutcome::Denied { .. }));
        }
        Ok(())
    }

    #[test]
    fn evaluate_tick_empty_proposals_returns_empty() -> Result<(), AppError> {
        let policy = DefaultPolicyKernel::benchmark();
        let input = PolicyInput {
            tick: 1,
            observations: vec![],
            proposals: vec![],
            incidents: vec![],
            prior_decisions: vec![],
            metrics: empty_metrics(),
        };
        let decisions = policy.evaluate_tick(&input)?;
        assert!(decisions.is_empty());
        Ok(())
    }

    #[test]
    fn evaluate_tick_deterministic_replay() -> Result<(), AppError> {
        let policy = DefaultPolicyKernel::benchmark();
        let runtime = InMemoryAgentRuntime::new();
        runtime.register(Box::new(DummyAgentA::new("dummy-a".into())))?;
        runtime.register(Box::new(DummyAgentB::new("dummy-b".into())))?;

        let obs = mem_obs(100, 90);
        let proposals = runtime.collect_proposals(&[obs.clone()])?;

        let input = PolicyInput {
            tick: 1,
            observations: vec![obs],
            proposals: proposals.clone(),
            incidents: vec![],
            prior_decisions: vec![],
            metrics: empty_metrics(),
        };

        // Same input → same decisions (run twice)
        let first = policy.evaluate_tick(&input)?;
        let second = policy.evaluate_tick(&input)?;

        assert_eq!(first.len(), second.len());
        for (i, (a, b)) in first.iter().zip(second.iter()).enumerate() {
            assert_eq!(
                a.outcome, b.outcome,
                "non-deterministic replay at index {}",
                i
            );
        }
        Ok(())
    }
}
