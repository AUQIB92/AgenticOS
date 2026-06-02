use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use agenticos_application::AppError;
use agenticos_domain::{ActionKind, AgentId, Decision, Incident, IncidentCategory, IncidentSeverity, Proposal, ProposalId};
use agenticos_policy::PolicyInput;

use crate::veto::{VetoDecision, VetoReason};
use crate::{SafetyConfig, SafetyInput, SafetyMetrics, SafetyOutput};

/// Default safety governor implementing all five governance responsibilities:
///
/// 1. Proposal validation
/// 2. Incident-aware decision filtering
/// 3. Policy invariant enforcement
/// 4. Conflict arbitration
/// 5. Decision auditing
pub struct DefaultSafetyGovernor {
    config: SafetyConfig,
}

impl DefaultSafetyGovernor {
    pub fn new(config: SafetyConfig) -> Self {
        Self { config }
    }

    pub fn with_defaults() -> Self {
        Self::new(SafetyConfig::default())
    }

    pub fn evaluate(&self, input: SafetyInput) -> Result<SafetyOutput, AppError> {
        let mut vetoes: Vec<VetoDecision> = Vec::new();
        let mut escalations: Vec<Incident> = Vec::new();
        let mut approved: Vec<Decision> = Vec::new();

        // Map proposals by id for fast lookup
        let proposals: HashMap<ProposalId, &Proposal> = input
            .policy_input
            .proposals
            .iter()
            .map(|p| (p.id.clone(), p))
            .collect();

        // 1. Per-decision governance checks
        for decision in input.decisions {
            let proposal = match proposals.get(&decision.proposal_id) {
                Some(p) => p,
                None => {
                    return Err(AppError::Message(format!(
                        "safety governor: decision {} references unknown proposal {}",
                        decision.id, decision.proposal_id
                    )));
                }
            };

            let checks = self.check_proposal(proposal, decision, input.policy_input);

            if checks.is_empty() {
                approved.push(decision.clone());
            } else {
                for (reason, explanation) in checks {
                    let veto = VetoDecision {
                        decision_id: decision.id.clone(),
                        proposal_id: decision.proposal_id.clone(),
                        reason,
                        explanation,
                        timestamp: now_utc(),
                    };
                    vetoes.push(veto);
                }

                escalations.push(Incident::new(
                    IncidentCategory::GovernanceViolation,
                    IncidentSeverity::Warning,
                    AgentId::from("safety-governor"),
                    None,
                    format!(
                        "SafetyGovernor vetoed proposal {} from agent {}",
                        proposal.id, proposal.agent_id
                    ),
                ));
            }
        }

        // 2. Conflict arbitration
        let (conflict_vetoes, conflict_incidents) =
            self.arbritrate_conflicts(input.policy_input);
        let conflict_ids: HashSet<ProposalId> =
            conflict_vetoes.iter().map(|v| v.proposal_id.clone()).collect();
        approved.retain(|d| !conflict_ids.contains(&d.proposal_id));
        vetoes.extend(conflict_vetoes);
        escalations.extend(conflict_incidents);

        // 3. Safety metrics
        let metrics = SafetyMetrics {
            veto_count: vetoes.len() as u64,
            veto_reason_breakdown: count_reasons(&vetoes),
            safety_escalations: escalations.len() as u64,
            policy_violation_attempts: count_policy_violations(&vetoes),
        };

        Ok(SafetyOutput {
            vetoes,
            escalations,
            approved,
            metrics,
        })
    }

    fn check_proposal(
        &self,
        proposal: &Proposal,
        _decision: &Decision,
        policy_input: &PolicyInput,
    ) -> Vec<(VetoReason, String)> {
        let mut reasons: Vec<(VetoReason, String)> = Vec::new();

        // 1. Proposal validation
        if let Some(v) = self.validate_proposal(proposal) {
            reasons.push(v);
        }

        // 2. Security Agent cannot emit actions (ADR-0009)
        if proposal.agent_id.as_str() == "security-agent" {
            reasons.push((
                VetoReason::ActionNotPermitted,
                "SecurityAgent may not emit actions (advisory-only per ADR-0009)".into(),
            ));
        }

        // 3. Incident-triggered veto
        if let Some(v) = self.check_incident_trigger(policy_input) {
            reasons.push(v);
        }

        // 4. Resource limit enforcement
        if let Some(v) = self.check_resource_limits(proposal) {
            reasons.push(v);
        }

        reasons
    }

    /// Invariant 1: Proposals must have valid confidence and well-formed actions.
    fn validate_proposal(&self, proposal: &Proposal) -> Option<(VetoReason, String)> {
        if !(0.0..=1.0).contains(&proposal.confidence.0) {
            return Some((
                VetoReason::InvalidProposal,
                format!("confidence {} out of range [0,1]", proposal.confidence.0),
            ));
        }

        match &proposal.requested_action.kind {
            ActionKind::CgroupSetCpuWeight { weight, .. } => {
                if *weight == 0 || *weight > 10000 {
                    return Some((
                        VetoReason::InvalidProposal,
                        format!("cpu weight {weight} out of range [1, 10000]"),
                    ));
                }
            }
            ActionKind::CgroupSetMemoryMax { bytes, .. } => {
                if *bytes == 0 || *bytes > 1 << 40 {
                    return Some((
                        VetoReason::InvalidProposal,
                        format!("memory max {bytes} out of range [1, {}]", 1u64 << 40),
                    ));
                }
            }
            _ => {}
        }

        None
    }

    /// Invariant 2: If security incidents exist, veto non-essential actions.
    fn check_incident_trigger(
        &self,
        policy_input: &PolicyInput,
    ) -> Option<(VetoReason, String)> {
        if !self.config.veto_on_security_incidents {
            return None;
        }

        let has_security_incident = policy_input
            .incidents
            .iter()
            .any(|i| matches!(i.category, IncidentCategory::Security));

        if has_security_incident {
            Some((
                VetoReason::IncidentTriggered,
                "security incident active — all actions vetoed by safety governor".into(),
            ))
        } else {
            None
        }
    }

    /// Invariant 3: Resource limits cannot exceed configured bounds.
    fn check_resource_limits(&self, proposal: &Proposal) -> Option<(VetoReason, String)> {
        match &proposal.requested_action.kind {
            ActionKind::CgroupSetCpuWeight { weight, .. } => {
                if *weight > self.config.max_cpu_weight {
                    return Some((
                        VetoReason::ResourceLimitsExceeded,
                        format!(
                            "cpu weight {weight} exceeds max {}",
                            self.config.max_cpu_weight
                        ),
                    ));
                }
            }
            ActionKind::CgroupSetMemoryMax { bytes, .. } => {
                if let Some(max) = self.config.max_memory_bytes {
                    if *bytes > max {
                        return Some((
                            VetoReason::ResourceLimitsExceeded,
                            format!("memory max {bytes} exceeds limit {max}"),
                        ));
                    }
                }
            }
            _ => {}
        }

        None
    }

    /// Invariant 4: Conflicting proposals must be resolved before execution.
    fn arbritrate_conflicts(
        &self,
        policy_input: &PolicyInput,
    ) -> (Vec<VetoDecision>, Vec<Incident>) {
        let mut vetoes: Vec<VetoDecision> = Vec::new();
        let mut escalations: Vec<Incident> = Vec::new();

        let mut cpu_groups: HashMap<String, Vec<&Proposal>> = HashMap::new();
        let mut freeze_groups: HashMap<String, Vec<&Proposal>> = HashMap::new();

        for proposal in &policy_input.proposals {
            match &proposal.requested_action.kind {
                ActionKind::CgroupSetCpuWeight { group, .. } => {
                    cpu_groups
                        .entry(format!("cpu::{group}"))
                        .or_default()
                        .push(proposal);
                }
                ActionKind::CgroupSetMemoryMax { group, .. } => {
                    cpu_groups
                        .entry(format!("mem::{group}"))
                        .or_default()
                        .push(proposal);
                }
                ActionKind::ProcessFreezeGroup { group, .. } => {
                    freeze_groups
                        .entry(format!("freeze::{group}"))
                        .or_default()
                        .push(proposal);
                }
                ActionKind::ProcessThawGroup { group, .. } => {
                    freeze_groups
                        .entry(format!("thaw::{group}"))
                        .or_default()
                        .push(proposal);
                }
                _ => {}
            }
        }

        // CPU weight conflicts: different weights for same group
        for (_key, proposals) in &cpu_groups {
            if proposals.len() < 2 {
                continue;
            }
            let first = extract_cpu_weight(proposals[0]);
            for &p in &proposals[1..] {
                let current = extract_cpu_weight(p);
                if current != first {
                    vetoes.push(VetoDecision {
                        decision_id: agenticos_domain::DecisionId::new(),
                        proposal_id: p.id.clone(),
                        reason: VetoReason::ConflictingProposals,
                        explanation: format!(
                            "CPU/ memory weight conflict for group: proposals disagree on value"
                        ),
                        timestamp: now_utc(),
                    });
                }
            }
        }

        // Freeze/thaw conflicts
        for (_key, proposals) in &freeze_groups {
            if proposals.len() < 2 {
                continue;
            }
            let has_freeze = proposals
                .iter()
                .any(|p| matches!(p.requested_action.kind, ActionKind::ProcessFreezeGroup { .. }));
            let has_thaw = proposals
                .iter()
                .any(|p| matches!(p.requested_action.kind, ActionKind::ProcessThawGroup { .. }));
            if has_freeze && has_thaw {
                for &p in proposals {
                    vetoes.push(VetoDecision {
                        decision_id: agenticos_domain::DecisionId::new(),
                        proposal_id: p.id.clone(),
                        reason: VetoReason::ConflictingProposals,
                        explanation: format!(
                            "freeze/thaw conflict for group — proposal {} is ambiguous",
                            p.id
                        ),
                        timestamp: now_utc(),
                    });
                }
                escalations.push(Incident::new(
                    IncidentCategory::GovernanceViolation,
                    IncidentSeverity::Warning,
                    AgentId::from("safety-governor"),
                    None,
                    format!(
                        "freeze/thaw conflict detected among {} proposals — all vetoed",
                        proposals.len()
                    ),
                ));
            }
        }

        (vetoes, escalations)
    }
}

fn extract_cpu_weight(p: &Proposal) -> Option<u64> {
    match &p.requested_action.kind {
        ActionKind::CgroupSetCpuWeight { weight, .. } => Some(*weight),
        _ => None,
    }
}

fn count_reasons(vetoes: &[VetoDecision]) -> HashMap<String, u64> {
    let mut map: HashMap<String, u64> = HashMap::new();
    for v in vetoes {
        let key = format!("{:?}", v.reason);
        *map.entry(key).or_insert(0) += 1;
    }
    map
}

fn count_policy_violations(vetoes: &[VetoDecision]) -> u64 {
    vetoes
        .iter()
        .filter(|v| {
            matches!(
                v.reason,
                VetoReason::GovernanceInvariantViolation | VetoReason::ActionNotPermitted
            )
        })
        .count() as u64
}

fn now_utc() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()))
        .unwrap_or_else(|_| "0.000000000Z".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenticos_domain::{
        ActionId, ActionKind, ActionRequest, ActionSafetyLevel, AgentId, Confidence, DecisionId,
        DecisionOutcome, DenialReason, IncidentCategory, IncidentSeverity, MetricCollection,
        ObservationId, ProposalId,
    };
    use crate::{SafetyConfig, SafetyInput};

    fn proposal(
        agent_id: &str,
        action_kind: ActionKind,
        safety_level: ActionSafetyLevel,
        confidence: f32,
    ) -> Proposal {
        Proposal {
            id: ProposalId::new(),
            agent_id: AgentId::from(agent_id),
            created_at: "0.000000000Z".to_owned(),
            based_on: vec![ObservationId::from("obs-1")],
            requested_action: ActionRequest {
                id: ActionId::new(),
                kind: action_kind,
                safety_level,
            },
            rationale: "test".to_owned(),
            confidence: Confidence(confidence),
        }
    }

    fn approved_decision(proposal_id: &ProposalId) -> Decision {
        Decision {
            id: DecisionId::new(),
            proposal_id: proposal_id.clone(),
            decided_at: "0.000000000Z".to_owned(),
            decided_by: AgentId::from("policy-kernel"),
            outcome: DecisionOutcome::Approved,
            explanation: "approved by policy".into(),
        }
    }

    fn denied_decision(proposal_id: &ProposalId) -> Decision {
        Decision {
            id: DecisionId::new(),
            proposal_id: proposal_id.clone(),
            decided_at: "0.000000000Z".to_owned(),
            decided_by: AgentId::from("policy-kernel"),
            outcome: DecisionOutcome::Denied {
                reason: DenialReason::UnsafeAction,
            },
            explanation: "denied by policy".into(),
        }
    }

    fn policy_input(proposals: Vec<Proposal>, incidents: Vec<Incident>) -> PolicyInput {
        PolicyInput {
            tick: 1,
            observations: vec![],
            proposals,
            incidents,
            prior_decisions: vec![],
            metrics: MetricCollection {
                source: "test".into(),
                samples: vec![],
            },
        }
    }

    // --------------------------------------------------------------
    // 1. Invalid proposal veto
    // --------------------------------------------------------------
    #[test]
    fn invalid_confidence_vetoed() {
        let governor = DefaultSafetyGovernor::with_defaults();
        let p = proposal("agent-1", ActionKind::ObserveOnly, ActionSafetyLevel::ReadOnly, 1.5);
        let d = approved_decision(&p.id);

        let input = SafetyInput {
            policy_input: &policy_input(vec![p], vec![]),
            decisions: &[d],
        };
        let output = governor.evaluate(input).unwrap();

        assert_eq!(output.approved.len(), 0);
        // Only confidence fails; action is ObserveOnly so no range or limit check
        assert!(output.vetoes.iter().any(|v| v.reason == VetoReason::InvalidProposal));
    }

    #[test]
    fn cpu_weight_out_of_range_vetoed() {
        let governor = DefaultSafetyGovernor::with_defaults();
        let p = proposal(
            "agent-1",
            ActionKind::CgroupSetCpuWeight {
                group: "test".into(),
                weight: 99999,
            },
            ActionSafetyLevel::MediumRisk,
            0.9,
        );
        let d = approved_decision(&p.id);

        let input = SafetyInput {
            policy_input: &policy_input(vec![p], vec![]),
            decisions: &[d],
        };
        let output = governor.evaluate(input).unwrap();

        assert_eq!(output.approved.len(), 0);
        // Two checks fire: InvalidProposal (weight>10000) + ResourceLimitsExceeded (weight>1000)
        assert!(output.vetoes.iter().any(|v| v.reason == VetoReason::InvalidProposal
            || v.reason == VetoReason::ResourceLimitsExceeded));
    }

    // --------------------------------------------------------------
    // 2. Conflicting proposal arbitration
    // --------------------------------------------------------------
    #[test]
    fn conflicting_cpu_weights_vetoed() {
        let governor = DefaultSafetyGovernor::with_defaults();
        let p1 = proposal(
            "agent-1",
            ActionKind::CgroupSetCpuWeight {
                group: "test".into(),
                weight: 100,
            },
            ActionSafetyLevel::MediumRisk,
            0.9,
        );
        let p2 = proposal(
            "agent-2",
            ActionKind::CgroupSetCpuWeight {
                group: "test".into(),
                weight: 200,
            },
            ActionSafetyLevel::MediumRisk,
            0.9,
        );
        let d1 = approved_decision(&p1.id);
        let d2 = approved_decision(&p2.id);

        let input = SafetyInput {
            policy_input: &policy_input(vec![p1.clone(), p2.clone()], vec![]),
            decisions: &[d1, d2],
        };
        let output = governor.evaluate(input).unwrap();

        // p1 passes per-decision checks; p2 conflicts with p1 and is removed from approved
        assert!(output.vetoes.iter().any(|v| v.reason == VetoReason::ConflictingProposals));
        assert_eq!(output.approved.len(), 1);
        assert_eq!(output.approved[0].proposal_id, p1.id);
    }

    // --------------------------------------------------------------
    // 3. Incident-triggered veto
    // --------------------------------------------------------------
    #[test]
    fn incident_triggers_veto() {
        let governor = DefaultSafetyGovernor::with_defaults();
        let p = proposal(
            "agent-1",
            ActionKind::CgroupSetCpuWeight {
                group: "test".into(),
                weight: 100,
            },
            ActionSafetyLevel::MediumRisk,
            0.9,
        );
        let d = approved_decision(&p.id);

        let incident = Incident::new(
            IncidentCategory::Security,
            IncidentSeverity::Warning,
            AgentId::from("security-agent"),
            None,
            "fork storm detected",
        );

        let input = SafetyInput {
            policy_input: &policy_input(vec![p], vec![incident]),
            decisions: &[d],
        };
        let output = governor.evaluate(input).unwrap();

        assert_eq!(output.approved.len(), 0);
        assert_eq!(output.vetoes.len(), 1);
        assert_eq!(output.vetoes[0].reason, VetoReason::IncidentTriggered);
        assert_eq!(output.escalations.len(), 1);
    }

    // --------------------------------------------------------------
    // 4. Governance invariant enforcement
    // --------------------------------------------------------------
    #[test]
    fn resource_limit_exceeded_vetoed() {
        let config = SafetyConfig {
            max_cpu_weight: 500,
            ..SafetyConfig::default()
        };
        let governor = DefaultSafetyGovernor::new(config);
        let p = proposal(
            "agent-1",
            ActionKind::CgroupSetCpuWeight {
                group: "test".into(),
                weight: 999,
            },
            ActionSafetyLevel::MediumRisk,
            0.9,
        );
        let d = approved_decision(&p.id);

        let input = SafetyInput {
            policy_input: &policy_input(vec![p], vec![]),
            decisions: &[d],
        };
        let output = governor.evaluate(input).unwrap();

        assert_eq!(output.approved.len(), 0);
        assert_eq!(output.vetoes.len(), 1);
        assert_eq!(output.vetoes[0].reason, VetoReason::ResourceLimitsExceeded);
    }

    // --------------------------------------------------------------
    // 5. Security Agent action veto
    // --------------------------------------------------------------
    #[test]
    fn security_agent_action_vetoed() {
        let governor = DefaultSafetyGovernor::with_defaults();
        let p = proposal(
            "security-agent",
            ActionKind::ObserveOnly,
            ActionSafetyLevel::ReadOnly,
            1.0,
        );
        let d = approved_decision(&p.id);

        let input = SafetyInput {
            policy_input: &policy_input(vec![p], vec![]),
            decisions: &[d],
        };
        let output = governor.evaluate(input).unwrap();

        assert_eq!(output.approved.len(), 0);
        assert_eq!(output.vetoes.len(), 1);
        assert_eq!(output.vetoes[0].reason, VetoReason::ActionNotPermitted);
    }

    // --------------------------------------------------------------
    // 6. Approved-by-policy-and-safety passes through
    // --------------------------------------------------------------
    #[test]
    fn valid_proposal_passes_safety() {
        let governor = DefaultSafetyGovernor::with_defaults();
        let p = proposal(
            "agent-1",
            ActionKind::ObserveOnly,
            ActionSafetyLevel::ReadOnly,
            1.0,
        );
        let d = approved_decision(&p.id);

        let input = SafetyInput {
            policy_input: &policy_input(vec![p], vec![]),
            decisions: &[d.clone()],
        };
        let output = governor.evaluate(input).unwrap();

        assert_eq!(output.approved.len(), 1);
        assert_eq!(output.approved[0].id, d.id);
        assert!(output.vetoes.is_empty());
    }

    // --------------------------------------------------------------
    // 7. Safety metrics
    // --------------------------------------------------------------
    #[test]
    fn safety_metrics_produced() {
        let governor = DefaultSafetyGovernor::with_defaults();
        let p1 = proposal(
            "security-agent",
            ActionKind::CgroupSetCpuWeight {
                group: "test".into(),
                weight: 99999,
            },
            ActionSafetyLevel::MediumRisk,
            1.5,
        );
        let d1 = approved_decision(&p1.id);

        let input = SafetyInput {
            policy_input: &policy_input(vec![p1], vec![]),
            decisions: &[d1],
        };
        let output = governor.evaluate(input).unwrap();

        // Three vetoes: InvalidProposal (confidence), InvalidProposal (weight), ActionNotPermitted, ResourceLimitsExceeded
        assert!(output.metrics.veto_count >= 3);
        assert!(output.metrics.veto_reason_breakdown.contains_key("InvalidProposal"));
        assert!(output.metrics.veto_reason_breakdown.contains_key("ActionNotPermitted"));
    }

    // --------------------------------------------------------------
    // 8. Unknown proposal returns error
    // --------------------------------------------------------------
    #[test]
    fn decision_references_unknown_proposal() {
        let governor = DefaultSafetyGovernor::with_defaults();
        let dummy_proposal_id = ProposalId::from("nonexistent");
        let d = approved_decision(&dummy_proposal_id);

        let input = SafetyInput {
            policy_input: &policy_input(vec![], vec![]),
            decisions: &[d],
        };
        let result = governor.evaluate(input);

        assert!(result.is_err());
    }
}

