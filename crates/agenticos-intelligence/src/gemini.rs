use std::time::{Duration, Instant};

use agenticos_domain::{AgentId, ProviderMetadata, Recommendation, WorkloadClass};

use crate::provider::LlmProvider;
use crate::types::RecommendationContext;

const GEMINI_ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1/models";

pub struct GeminiProvider {
    api_key: String,
    model: String,
    client: reqwest::blocking::Client,
    timeout: Duration,
}

impl GeminiProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        timeout_seconds: u64,
    ) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .build()
            .unwrap_or_default();
        Self {
            api_key: api_key.into(),
            model: model.into(),
            client,
            timeout: Duration::from_secs(timeout_seconds),
        }
    }

    fn build_prompt(&self, context: &RecommendationContext) -> String {
        format!(
            "You are a workload classification system. Classify the workload based on these inputs:

Observation Summary:
{}

Agent: {}

System State:
{}

Return ONLY valid JSON with no markdown formatting. Use this exact schema:
{{
  \"classification\": \"Database\" or \"Interactive\" or \"Build\" or \"Batch\" or \"SystemService\" or \"Unknown\",
  \"confidence\": <0.0 to 1.0>,
  \"reasoning\": \"brief explanation of the classification\"
}}",
            context.observation_summary, context.agent_name, context.system_state_summary
        )
    }

    fn call_gemini(&self, prompt: &str) -> Result<String, String> {
        let url = format!(
            "{}/{}:generateContent?key={}",
            GEMINI_ENDPOINT, self.model, self.api_key
        );

        let body = serde_json::json!({
            "contents": [{
                "parts": [{"text": prompt}]
            }],
            "generationConfig": {
                "temperature": 0.0,
                "responseMimeType": "application/json"
            }
        });

        let start = Instant::now();
        let response = self
            .client
            .post(&url)
            .json(&body)
            .timeout(self.timeout)
            .send()
            .map_err(|e| format!("request failed: {e}"))?;
        let _latency = start.elapsed().as_millis() as u64;

        let status = response.status();
        let body_text = response
            .text()
            .map_err(|e| format!("failed to read response body: {e}"))?;

        if !status.is_success() {
            return Err(format!("API error ({}): {}", status, body_text));
        }

        let parsed: serde_json::Value =
            serde_json::from_str(&body_text).map_err(|e| format!("failed to parse response JSON: {e}"))?;

        let text = parsed["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| "no text in Gemini response".to_string())?;

        Ok(text.to_owned())
    }

    fn parse_gemini_response(text: &str) -> Result<(WorkloadClass, f64, String), String> {
        let cleaned = text.trim();
        let json: serde_json::Value =
            serde_json::from_str(cleaned).map_err(|e| format!("invalid JSON in response: {e}"))?;

        let class_str = json["classification"]
            .as_str()
            .ok_or_else(|| "missing classification field".to_string())?;

        let class = match class_str {
            "Database" => WorkloadClass::Database,
            "Interactive" => WorkloadClass::Interactive,
            "Build" => WorkloadClass::Build,
            "Batch" => WorkloadClass::Batch,
            "SystemService" => WorkloadClass::SystemService,
            "Unknown" => WorkloadClass::Unknown,
            other => {
                return Err(format!("unknown classification: {other}"));
            }
        };

        let confidence = json["confidence"].as_f64().unwrap_or(0.0);

        if !(0.0..=1.0).contains(&confidence) {
            return Err(format!("confidence out of range: {confidence}"));
        }

        let reasoning = json["reasoning"]
            .as_str()
            .unwrap_or("no reasoning provided")
            .to_owned();

        Ok((class, confidence, reasoning))
    }

    fn fallback_recommendation(agent_name: &str) -> Recommendation {
        Recommendation::new(
            AgentId::from(agent_name),
            0.0,
            "Workload classified as Unknown",
            "Gemini API call failed, returning Unknown classification",
        )
    }
}

impl LlmProvider for GeminiProvider {
    fn generate_recommendation(&self, context: RecommendationContext) -> Recommendation {
        let provider_meta = ProviderMetadata::new(
            "gemini",
            &self.model,
            false,
            0,
        );

        let prompt = self.build_prompt(&context);
        match self.call_gemini(&prompt) {
            Ok(text) => match Self::parse_gemini_response(&text) {
                Ok((class, confidence, reasoning)) => {
                    let summary = format!("Workload classified as {}", class.label());
                    Recommendation::new(
                        AgentId::from(context.agent_name.as_str()),
                        confidence,
                        summary,
                        reasoning,
                    )
                    .with_provider(provider_meta)
                }
                Err(e) => {
                    eprintln!("warning: Gemini parse error: {e}");
                    Self::fallback_recommendation(&context.agent_name)
                        .with_provider(provider_meta)
                }
            },
            Err(e) => {
                eprintln!("warning: Gemini API error: {e}");
                Self::fallback_recommendation(&context.agent_name)
                    .with_provider(provider_meta)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context() -> RecommendationContext {
        RecommendationContext::new("cpu 85% procs 14 postgres", "classifier", "CPU 85% | 14 procs")
    }

    #[test]
    fn parse_valid_gemini_response() {
        let text = r#"{
            "classification": "Database",
            "confidence": 0.92,
            "reasoning": "High CPU with postgres process"
        }"#;
        let (class, confidence, reasoning) = GeminiProvider::parse_gemini_response(text).unwrap();
        assert_eq!(class, WorkloadClass::Database);
        assert!((confidence - 0.92).abs() < 0.01);
        assert!(reasoning.contains("postgres"));
    }

    #[test]
    fn parse_valid_response_unknown() {
        let text = r#"{
            "classification": "Unknown",
            "confidence": 0.3,
            "reasoning": "no clear pattern detected"
        }"#;
        let (class, confidence, reasoning) = GeminiProvider::parse_gemini_response(text).unwrap();
        assert_eq!(class, WorkloadClass::Unknown);
        assert!((confidence - 0.3).abs() < 0.01);
        assert!(reasoning.contains("no clear pattern"));
    }

    #[test]
    fn parse_malformed_json() {
        let text = "not valid json at all";
        let result = GeminiProvider::parse_gemini_response(text);
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_classification_field() {
        let text = r#"{"confidence": 0.5, "reasoning": "test"}"#;
        let result = GeminiProvider::parse_gemini_response(text);
        assert!(result.is_err());
    }

    #[test]
    fn parse_invalid_classification() {
        let text = r#"{
            "classification": "InvalidType",
            "confidence": 0.5,
            "reasoning": "test"
        }"#;
        let result = GeminiProvider::parse_gemini_response(text);
        assert!(result.is_err());
    }

    #[test]
    fn parse_confidence_out_of_range() {
        let text = r#"{
            "classification": "Database",
            "confidence": 1.5,
            "reasoning": "test"
        }"#;
        let result = GeminiProvider::parse_gemini_response(text);
        assert!(result.is_err());
    }

    #[test]
    fn parse_build_classification() {
        let text = r#"{
            "classification": "Build",
            "confidence": 0.88,
            "reasoning": "multiple compiler processes"
        }"#;
        let (class, confidence, _) = GeminiProvider::parse_gemini_response(text).unwrap();
        assert_eq!(class, WorkloadClass::Build);
        assert!((confidence - 0.88).abs() < 0.01);
    }

    #[test]
    fn parse_interactive_classification() {
        let text = r#"{
            "classification": "Interactive",
            "confidence": 0.85,
            "reasoning": "user-facing processes detected"
        }"#;
        let (class, _, _) = GeminiProvider::parse_gemini_response(text).unwrap();
        assert_eq!(class, WorkloadClass::Interactive);
    }

    #[test]
    fn build_prompt_contains_context() {
        let provider =
            GeminiProvider::new("test-key".to_string(), "gemini-2.5-flash".to_string(), 10);
        let ctx = test_context();
        let prompt = provider.build_prompt(&ctx);
        assert!(prompt.contains("cpu 85%"));
        assert!(prompt.contains("classifier"));
        assert!(prompt.contains("CPU 85%"));
        assert!(prompt.contains("JSON"));
    }

    #[test]
    fn fallback_returns_unknown() {
        let rec = GeminiProvider::fallback_recommendation("classifier");
        assert!(rec.summary.contains("Unknown"));
        assert!((rec.confidence - 0.0).abs() < 0.01);
        assert_eq!(rec.source_agent.as_str(), "classifier");
    }

    #[test]
    fn parse_response_with_extra_whitespace() {
        let text = "  \n  {\n  \"classification\": \"Batch\",\n  \"confidence\": 0.75,\n  \"reasoning\": \"high cpu many processes\"\n}  \n  ";
        let (class, confidence, _) = GeminiProvider::parse_gemini_response(text).unwrap();
        assert_eq!(class, WorkloadClass::Batch);
        assert!((confidence - 0.75).abs() < 0.01);
    }

    #[test]
    fn provider_metadata_included() {
        let _ctx = test_context();
        let _provider = GeminiProvider::new(
            "test-key".to_string(),
            "gemini-2.5-flash".to_string(),
            10,
        );
        let meta = ProviderMetadata::new("gemini", "gemini-2.5-flash", false, 42);
        assert_eq!(meta.provider_name, "gemini");
        assert_eq!(meta.model_name, "gemini-2.5-flash");
        assert!(!meta.cache_hit);
        assert_eq!(meta.generation_latency_ms, 42);
    }

    // ── Replay Test ────────────────────────────────────────────────

    #[test]
    fn gemini_recommendation_round_trips_via_json() {
        let rec = Recommendation::new(
            AgentId::from("classifier"),
            0.92,
            "Workload classified as Database",
            "High CPU with database process",
        );
        let json = serde_json::to_string(&rec).unwrap();
        let back: Recommendation = serde_json::from_str(&json).unwrap();
        assert_eq!(rec.id, back.id);
        assert_eq!(rec.summary, back.summary);
        assert_eq!(rec.reasoning, back.reasoning);
    }

    #[test]
    fn gemini_recommendation_with_provider_metadata_round_trips() {
        let rec = Recommendation::new(
            AgentId::from("classifier"),
            0.92,
            "Workload classified as Database",
            "High CPU with database process",
        )
        .with_provider(ProviderMetadata::new("gemini", "gemini-2.5-flash", false, 150));
        let json = serde_json::to_string(&rec).unwrap();
        let back: Recommendation = serde_json::from_str(&json).unwrap();
        assert_eq!(rec.id, back.id);
        let meta = back.provider.unwrap();
        assert_eq!(meta.provider_name, "gemini");
        assert_eq!(meta.model_name, "gemini-2.5-flash");
        assert!(!meta.cache_hit);
        assert_eq!(meta.generation_latency_ms, 150);
    }
}
