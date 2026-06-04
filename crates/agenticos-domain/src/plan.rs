use std::collections::HashMap;

use crate::{IntentId, PlanId};

/// A single step within a TaskPlan.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlanStep {
    pub order: u32,
    pub action: String,
    pub parameters: HashMap<String, String>,
}

impl PlanStep {
    pub fn new(order: u32, action: impl Into<String>, parameters: HashMap<String, String>) -> Self {
        Self {
            order,
            action: action.into(),
            parameters,
        }
    }
}

/// A deterministic, persistent, replayable plan derived from an Intent.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskPlan {
    pub id: PlanId,
    pub source_intent_id: IntentId,
    pub steps: Vec<PlanStep>,
    pub status: String,
    pub timestamp: String,
}

impl TaskPlan {
    pub fn new(
        source_intent_id: IntentId,
        steps: Vec<PlanStep>,
        status: impl Into<String>,
    ) -> Self {
        assert!(!steps.is_empty(), "TaskPlan must have at least one step");
        Self {
            id: PlanId::new(),
            source_intent_id,
            steps,
            status: status.into(),
            timestamp: now_utc(),
        }
    }
}

fn now_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{}.{:09}Z", d.as_secs(), d.subsec_nanos()))
        .unwrap_or_else(|_| "0.000000000Z".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_step_constructs() {
        let mut params = HashMap::new();
        params.insert("application".into(), "vscode".into());
        let step = PlanStep::new(1, "launch_application", params);
        assert_eq!(step.order, 1);
        assert_eq!(step.action, "launch_application");
        assert_eq!(step.parameters.get("application").unwrap(), "vscode");
    }

    #[test]
    fn plan_step_round_trips_via_json() {
        let mut params = HashMap::new();
        params.insert("url".into(), "https://github.com".into());
        let step = PlanStep::new(1, "open_url", params);
        let json = serde_json::to_string(&step).unwrap();
        let back: PlanStep = serde_json::from_str(&json).unwrap();
        assert_eq!(step.order, back.order);
        assert_eq!(step.action, back.action);
        assert_eq!(step.parameters.get("url"), back.parameters.get("url"));
    }

    #[test]
    fn plan_constructs() {
        let intent_id = IntentId::new();
        let step = PlanStep::new(1, "launch_application", HashMap::new());
        let plan = TaskPlan::new(intent_id.clone(), vec![step], "pending");
        assert_eq!(plan.source_intent_id, intent_id);
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.status, "pending");
        assert!(!plan.timestamp.is_empty());
    }

    #[test]
    #[should_panic(expected = "at least one step")]
    fn plan_panics_on_no_steps() {
        TaskPlan::new(IntentId::new(), vec![], "pending");
    }

    #[test]
    fn plan_round_trips_via_json() {
        let intent_id = IntentId::new();
        let step = PlanStep::new(1, "launch_application", HashMap::new());
        let plan = TaskPlan::new(intent_id, vec![step], "pending");
        let json = serde_json::to_string(&plan).unwrap();
        let back: TaskPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(plan.id, back.id);
        assert_eq!(plan.status, back.status);
        assert_eq!(plan.steps.len(), back.steps.len());
    }

    #[test]
    fn plan_id_is_deterministic_format() {
        let id = PlanId::new();
        assert!(id.as_str().starts_with("PlanId-"));
    }
}
