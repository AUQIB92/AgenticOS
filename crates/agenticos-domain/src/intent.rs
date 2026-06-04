use std::collections::HashMap;

use crate::IntentId;

/// A parsed user intent — the structured output of natural language understanding.
///
/// Intents are advisory only. They may NOT:
/// - Trigger OS mutations directly
/// - Bypass Policy or Safety Governor
/// - Create executable actions
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Intent {
    pub id: IntentId,
    pub source_text: String,
    pub intent_type: String,
    pub parameters: HashMap<String, String>,
    pub confidence: f64,
    pub timestamp: String,
}

impl Intent {
    pub fn new(
        source_text: impl Into<String>,
        intent_type: impl Into<String>,
        parameters: HashMap<String, String>,
        confidence: f64,
    ) -> Self {
        assert!(
            (0.0..=1.0).contains(&confidence),
            "intent confidence must be in [0.0, 1.0]"
        );
        Self {
            id: IntentId::new(),
            source_text: source_text.into(),
            intent_type: intent_type.into(),
            parameters,
            confidence,
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
    fn intent_constructs() {
        let mut params = HashMap::new();
        params.insert("application".into(), "vscode".into());
        let intent = Intent::new("Open VS Code", "launch_application", params, 0.9);
        assert_eq!(intent.intent_type, "launch_application");
        assert_eq!(intent.parameters.get("application").unwrap(), "vscode");
        assert!((intent.confidence - 0.9).abs() < 0.01);
        assert!(!intent.timestamp.is_empty());
    }

    #[test]
    #[should_panic(expected = "confidence must be in")]
    fn intent_panics_on_bad_confidence() {
        Intent::new("test", "unknown", HashMap::new(), 1.5);
    }

    #[test]
    fn intent_round_trips_via_json() {
        let mut params = HashMap::new();
        params.insert("app".into(), "firefox".into());
        let intent = Intent::new("Open Firefox", "launch_application", params, 0.85);
        let json = serde_json::to_string(&intent).unwrap();
        let back: Intent = serde_json::from_str(&json).unwrap();
        assert_eq!(intent.id, back.id);
        assert_eq!(intent.intent_type, back.intent_type);
        assert_eq!(intent.parameters.get("app"), back.parameters.get("app"));
    }

    #[test]
    fn intent_id_is_deterministic_format() {
        let id = IntentId::new();
        assert!(id.as_str().starts_with("IntentId-"));
    }
}
