use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agenticos_application::AppError;
use agenticos_domain::{
    ActionKind, ActionStatus, AgentId, DecisionOutcome, EventEnvelope, EventPayload, Incident,
    IncidentCategory, IncidentSeverity, MetricCollection, MetricLabel, MetricSample, MetricValue,
    ObservationSource, TraceId,
};
use agenticos_policy::DeterministicPolicyKernel;
use agenticos_runtime::AgentRuntime;
use agenticos_safety::SafetyInput;
use tokio::time::interval;

use crate::bootstrap::DaemonContext;

pub struct DaemonService {
    ctx: Arc<DaemonContext>,
}

impl DaemonService {
    pub fn new(ctx: DaemonContext) -> Self {
        Self {
            ctx: Arc::new(ctx),
        }
    }

    pub async fn run(&self) -> Result<(), AppError> {
        let mut ticker = interval(Duration::from_secs(1));
        let ctx = self.ctx.clone();
        let trace_id = TraceId::from("daemon-main");
        let mut tick_number = 0u64;

        loop {
            ticker.tick().await;
            let tick_start = Instant::now();
            tick_number += 1;

            // ---------------------------------------------------------------
            // 1. Observe
            // ---------------------------------------------------------------
            let observations = match ctx.observer.observe() {
                Ok(obs) => obs,
                Err(e) => {
                    emit_error(&ctx, &trace_id, &format!("observation failed: {e}"));
                    continue;
                }
            };

            for obs in &observations {
                let envelope = observation_envelope(obs, &trace_id);
                publish_and_trace(&ctx, envelope);
            }

            let obs_count = observations.len();

            // ---------------------------------------------------------------
            // 1b. Workload Classification (intelligence)
            // ---------------------------------------------------------------
            let (recommendation_count, classifications_skipped_total) = {
                let mut classifier = ctx.classifier.lock().unwrap();
                let rec = classifier.classify_workload(&observations);
                let envelope = EventEnvelope::new(
                    agenticos_domain::Topic::new(format!(
                        "recommendations.{}",
                        classifier.agent_id().as_str()
                    )),
                    trace_id.clone(),
                    EventPayload::Recommendation(rec),
                );
                publish_and_trace(&ctx, envelope);
                (classifier.classification_count(), classifier.classifications_skipped())
            };

            // ---------------------------------------------------------------
            // 2. Agent proposals
            // ---------------------------------------------------------------
            let proposals = match ctx.agent_runtime.collect_proposals(&observations) {
                Ok(p) => p,
                Err(e) => {
                    emit_error(&ctx, &trace_id, &format!("proposal collection failed: {e}"));
                    continue;
                }
            };

            for prop in &proposals {
                let envelope = EventEnvelope::new(
                    agenticos_domain::Topic::new(format!(
                        "proposals.{}",
                        prop.agent_id.as_str()
                    )),
                    trace_id.clone(),
                    EventPayload::Proposal(prop.clone()),
                );
                publish_and_trace(&ctx, envelope);
            }

            let proposal_depth = proposals.len();

            // ---------------------------------------------------------------
            // 2b. Agent incidents
            // ---------------------------------------------------------------
            let incidents = match ctx.agent_runtime.collect_incidents(&observations) {
                Ok(i) => i,
                Err(e) => {
                    emit_error(&ctx, &trace_id, &format!("incident collection failed: {e}"));
                    continue;
                }
            };

            for incident in &incidents {
                let envelope = EventEnvelope::new(
                    agenticos_domain::Topic::new(format!(
                        "incidents.{}",
                        incident.category.description()
                    )),
                    trace_id.clone(),
                    EventPayload::Incident(incident.clone()),
                );
                publish_and_trace(&ctx, envelope);
            }

            let incident_depth = incidents.len();

            // ---------------------------------------------------------------
            // 3. Build PolicyInput snapshot
            // ---------------------------------------------------------------
            let metrics = build_metrics(
                2.0,
                proposal_depth as f64,
                0.0,
                0.0,
                tick_number,
            );
            let policy_input = agenticos_policy::PolicyInput {
                tick: tick_number,
                observations: observations.clone(),
                proposals: proposals.clone(),
                incidents: incidents.clone(),
                prior_decisions: vec![],
                metrics,
            };

            // ---------------------------------------------------------------
            // 4. Policy evaluation
            // ---------------------------------------------------------------
            let mut approvals = 0u64;
            let mut denials = 0u64;
            let mut successful_mutations = 0u64;
            let mut failed_mutations = 0u64;
            let mut rollback_count = 0u64;
            let mut cpu_weight_changes = 0u64;
            let mut cpu_max_changes = 0u64;
            let mut memory_max_changes = 0u64;
            let decision_total_ms;
            let mut executor_total_ms = 0u64;

            let decision_start = Instant::now();
            let decisions = match DeterministicPolicyKernel::evaluate_tick(
                &*ctx.policy_kernel,
                &policy_input,
            ) {
                Ok(d) => d,
                Err(e) => {
                    emit_error(&ctx, &trace_id, &format!("policy evaluation failed: {e}"));
                    continue;
                }
            };
            decision_total_ms = decision_start.elapsed().as_millis() as u64;

            // Trace all raw decisions
            for (prop, decision) in proposals.iter().zip(decisions.iter()) {
                let envelope = EventEnvelope::new(
                    agenticos_domain::Topic::new(format!(
                        "decisions.{}",
                        prop.agent_id.as_str()
                    )),
                    trace_id.clone(),
                    EventPayload::Decision(decision.clone()),
                );
                publish_and_trace(&ctx, envelope);
            }

            // ---------------------------------------------------------------
            // 4b. Safety Governor — filters decisions before execution
            // ---------------------------------------------------------------
            let safety_input = SafetyInput {
                policy_input: &policy_input,
                decisions: &decisions,
            };
            let safety_output = match ctx.safety_governor.evaluate(safety_input) {
                Ok(o) => o,
                Err(e) => {
                    emit_error(&ctx, &trace_id, &format!("safety governor failed: {e}"));
                    continue;
                }
            };

            // Trace vetoes
            for veto in &safety_output.vetoes {
                let envelope = EventEnvelope::new(
                    agenticos_domain::Topic::new(format!("vetoes.{}", veto.reason.as_ref())),
                    trace_id.clone(),
                    EventPayload::Trace(agenticos_domain::TraceEvent {
                        message: format!(
                            "veto proposal={} reason={:?} explanation={}",
                            veto.proposal_id, veto.reason, veto.explanation
                        ),
                    }),
                );
                publish_and_trace(&ctx, envelope);
            }

            // Trace escalation incidents
            for escalation in &safety_output.escalations {
                let envelope = EventEnvelope::new(
                    agenticos_domain::Topic::new(format!(
                        "incidents.{}",
                        escalation.category.description()
                    )),
                    trace_id.clone(),
                    EventPayload::Incident(escalation.clone()),
                );
                publish_and_trace(&ctx, envelope);
            }

            let veto_count = safety_output.vetoes.len() as u64;

            // ---------------------------------------------------------------
            // 4c. Execute only decisions passing both policy + safety
            // ---------------------------------------------------------------
            // Build a set of proposal IDs that passed safety
            let safe_proposal_ids: std::collections::HashSet<_> = safety_output
                .approved
                .iter()
                .map(|d| d.proposal_id.clone())
                .collect();

            for (prop, decision) in proposals.iter().zip(decisions.iter()) {
                // Skip if safety governor vetoed this decision
                if !safe_proposal_ids.contains(&decision.proposal_id) {
                    // Not counted as denial — it's a safety intervention
                    continue;
                }

                match &decision.outcome {
                    DecisionOutcome::Approved => {
                        let exec_start = Instant::now();

                        let approved = agenticos_domain::ApprovedAction {
                            request: prop.requested_action.clone(),
                            decision_id: decision.id.clone(),
                        };

                        match ctx.executor.execute(approved) {
                            Ok(result) => {
                                let exec_ms = exec_start.elapsed().as_millis() as u64;
                                executor_total_ms += exec_ms;

                                match result.status {
                                    ActionStatus::Succeeded => {
                                        successful_mutations += 1;
                                        match &prop.requested_action.kind {
                                            ActionKind::CgroupSetCpuWeight { .. } => {
                                                cpu_weight_changes += 1;
                                            }
                                            ActionKind::CgroupSetCpuMax { .. } => {
                                                cpu_max_changes += 1;
                                            }
                                            ActionKind::CgroupSetMemoryMax { .. } => {
                                                memory_max_changes += 1;
                                            }
                                            _ => {}
                                        }
                                        if result.rollback.is_some() {
                                            rollback_count += 1;
                                        }
                                    }
                                    ActionStatus::Failed => {
                                        failed_mutations += 1;
                                    }
                                    _ => {}
                                }

                                let envelope = EventEnvelope::new(
                                    agenticos_domain::Topic::new(format!(
                                        "results.{}",
                                        prop.agent_id.as_str()
                                    )),
                                    trace_id.clone(),
                                    EventPayload::ActionResult(result),
                                );
                                publish_and_trace(&ctx, envelope);
                                approvals += 1;
                            }
                            Err(e) => {
                                emit_error(
                                    &ctx,
                                    &trace_id,
                                    &format!("execution failed: {e}"),
                                );
                                failed_mutations += 1;
                            }
                        }
                    }
                    DecisionOutcome::Denied { .. } => {
                        denials += 1;
                    }
                    DecisionOutcome::RequiresApproval => {
                        denials += 1;
                    }
                }
            }

            // ---------------------------------------------------------------
            // 5. Metrics
            // ---------------------------------------------------------------
            let tick_duration_ms = tick_start.elapsed().as_millis() as u64;

            let metrics = build_metrics(
                2.0,
                proposal_depth as f64,
                decision_total_ms as f64,
                executor_total_ms as f64,
                tick_number,
            )
            .with_incident_count(incident_depth as f64)
            .with_safety_veto_count(safety_output.metrics.veto_count as f64)
            .with_safety_escalations(safety_output.metrics.safety_escalations as f64)
            .with_safety_freeze_ticks(safety_output.metrics.freeze_ticks as f64)
            .with_safety_selective_vetoes(safety_output.metrics.selective_vetoes as f64)
            .with_safety_global_vetoes(safety_output.metrics.global_vetoes as f64)
            .with_executor_successful_mutations(successful_mutations as f64)
            .with_executor_failed_mutations(failed_mutations as f64)
            .with_executor_rollback_count(rollback_count as f64)
            .with_executor_cpu_weight_changes(cpu_weight_changes as f64)
                .with_executor_cpu_max_changes(cpu_max_changes as f64)
                .with_executor_memory_max_changes(memory_max_changes as f64)
                .with_classifications_skipped(classifications_skipped_total as f64);

            let envelope = EventEnvelope::new(
                agenticos_domain::Topic::new("metrics.daemon"),
                trace_id.clone(),
                EventPayload::Trace(agenticos_domain::TraceEvent {
                    message: serde_json::to_string(&metrics).unwrap_or_default(),
                }),
            );
            publish_and_trace(&ctx, envelope);

            println!(
                "[agenticos] observe={obs_count} proposals={proposal_depth} \
                 incidents={incident_depth} vetoes={veto_count} \
                 approved={approvals} denied={denials} \
                 mutations={successful_mutations} failed_muts={failed_mutations} \
                 rollbacks={rollback_count} recs={recommendation_count} \
                 skipped={classifications_skipped_total} \
                 tick={tick_duration_ms}ms decision_latency={decision_total_ms}ms \
                 exec_latency={executor_total_ms}ms"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn observation_envelope(obs: &agenticos_domain::Observation, trace_id: &TraceId) -> EventEnvelope {
    let topic = match &obs.source {
        ObservationSource::Process => "observations.process",
        ObservationSource::Memory => "observations.memory",
        ObservationSource::Cpu => "observations.cpu",
        ObservationSource::Cgroup => "observations.cgroup",
        ObservationSource::File => "observations.file",
        ObservationSource::Device => "observations.device",
        ObservationSource::Network => "observations.network",
        ObservationSource::Security => "observations.security",
        ObservationSource::Benchmark => "observations.benchmark",
    };
    EventEnvelope::new(
        agenticos_domain::Topic::new(topic),
        trace_id.clone(),
        EventPayload::Observation(obs.clone()),
    )
}

fn publish_and_trace(ctx: &DaemonContext, envelope: EventEnvelope) {
    if let Err(e) = ctx.event_bus.publish(envelope.clone()) {
        eprintln!("publish error: {e}");
    }
    if let Err(e) = ctx.trace_store.append(envelope) {
        eprintln!("trace store error: {e}");
    }
}

fn emit_error(ctx: &DaemonContext, trace_id: &TraceId, description: &str) {
    let incident = EventEnvelope::new(
        agenticos_domain::Topic::new("system.error"),
        trace_id.clone(),
        EventPayload::Incident(Incident::new(
            IncidentCategory::ExecutorFailure,
            IncidentSeverity::Error,
            AgentId::from("daemon"),
            None,
            description,
        )),
    );
    let _ = ctx.event_bus.publish(incident);
    eprintln!("[agenticos] error: {description}");
}

fn build_metrics(
    active_agents: f64,
    proposal_depth: f64,
    decision_latency_ms: f64,
    executor_latency_ms: f64,
    _tick: u64,
) -> MetricCollection {
    MetricCollection {
        source: "daemon.service".into(),
        samples: vec![
            MetricSample {
                name: "active_agents".into(),
                value: MetricValue::Gauge(active_agents),
                labels: labels(),
                timestamp: now(),
            },
            MetricSample {
                name: "proposal_queue_depth".into(),
                value: MetricValue::Gauge(proposal_depth),
                labels: labels(),
                timestamp: now(),
            },
            MetricSample {
                name: "decision_latency_ms".into(),
                value: MetricValue::Histogram(vec![decision_latency_ms]),
                labels: labels(),
                timestamp: now(),
            },
            MetricSample {
                name: "executor_latency_ms".into(),
                value: MetricValue::Histogram(vec![executor_latency_ms]),
                labels: labels(),
                timestamp: now(),
            },
        ],
    }
}

fn labels() -> Vec<MetricLabel> {
    vec![MetricLabel {
        name: "service".into(),
        value: "daemon".into(),
    }]
}

fn now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()))
        .unwrap_or_else(|_| "0.000000000Z".to_owned())
}
