use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    ActionRequest, ActionResult, AgentId, Decision, IncidentId, MessageId, Observation,
    ObservationId, Proposal, Recommendation, TraceId,
};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Topic(pub String);

impl Topic {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn matches(&self, filter: &Topic) -> bool {
        if filter.0 == "*" {
            return true;
        }

        if let Some(prefix) = filter.0.strip_suffix(".*") {
            return self.0 == prefix || self.0.starts_with(&format!("{prefix}."));
        }

        self == filter
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EventEnvelope {
    pub id: MessageId,
    pub trace_id: TraceId,
    pub causation_id: Option<MessageId>,
    pub topic: Topic,
    pub timestamp: String,
    pub payload: EventPayload,
}

impl EventEnvelope {
    pub fn new(topic: Topic, trace_id: TraceId, payload: EventPayload) -> Self {
        Self {
            id: MessageId::new(),
            trace_id,
            causation_id: None,
            topic,
            timestamp: unix_timestamp_string(),
            payload,
        }
    }

    pub fn with_causation(mut self, causation_id: MessageId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum EventPayload {
    Observation(Observation),
    Proposal(Proposal),
    Decision(Decision),
    ActionRequest(ActionRequest),
    ActionResult(ActionResult),
    Incident(Incident),
    Recommendation(Recommendation),
    Trace(TraceEvent),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Incident {
    pub incident_id: IncidentId,
    pub category: IncidentCategory,
    pub severity: IncidentSeverity,
    pub source_agent: AgentId,
    pub source_observation: Option<ObservationId>,
    pub correlation_id: Option<String>,
    pub timestamp: String,
    pub description: String,
}

impl Incident {
    pub fn new(
        category: IncidentCategory,
        severity: IncidentSeverity,
        source_agent: AgentId,
        source_observation: Option<ObservationId>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            incident_id: IncidentId::new(),
            category,
            severity,
            source_agent,
            source_observation,
            correlation_id: None,
            timestamp: unix_timestamp_string(),
            description: description.into(),
        }
    }

    pub fn with_correlation(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum IncidentCategory {
    Security,
    ResourceContention,
    GovernanceViolation,
    PolicyViolation,
    ExecutorFailure,
    AgentFailure,
}

impl IncidentCategory {
    /// Human-readable label for use in topic names and logs.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Security => "security",
            Self::ResourceContention => "resource-contention",
            Self::GovernanceViolation => "governance-violation",
            Self::PolicyViolation => "policy-violation",
            Self::ExecutorFailure => "executor-failure",
            Self::AgentFailure => "agent-failure",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum IncidentSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TraceEvent {
    pub message: String,
}

fn unix_timestamp_string() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("{}.{:09}Z", duration.as_secs(), duration.subsec_nanos()),
        Err(_) => "0.000000000Z".to_owned(),
    }
}
