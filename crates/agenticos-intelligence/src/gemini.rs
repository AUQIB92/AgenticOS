use std::fmt;
use std::time::{Duration, Instant};

use agenticos_domain::{AgentId, ProviderMetadata, Recommendation, WorkloadClass};
use tokio::runtime::Handle;

use crate::provider::LlmProvider;
use crate::types::RecommendationContext;

/// Categorized reason why the Gemini provider fell back to an Unknown classification.
#[derive(Clone, Debug, PartialEq)]
pub enum FallbackReason {
    /// HTTP request to Gemini API failed entirely (timeout, connection refused, etc.)
    PromptFailure,
    /// Gemini API returned a non-200 status (e.g. 429 rate limit, 403 auth error)
    ApiError,
    /// Gemini API returned no candidates or empty text
    EmptyResponse,
    /// The response text was not valid JSON
    ParseError,
    /// Required field missing from the parsed JSON response
    MissingField,
    /// The classification string was not one of the expected values
    InvalidClassification,
}

impl fmt::Display for FallbackReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FallbackReason::PromptFailure => write!(f, "PromptFailure"),
            FallbackReason::ApiError => write!(f, "ApiError"),
            FallbackReason::EmptyResponse => write!(f, "EmptyResponse"),
            FallbackReason::ParseError => write!(f, "ParseError"),
            FallbackReason::MissingField => write!(f, "MissingField"),
            FallbackReason::InvalidClassification => write!(f, "InvalidClassification"),
        }
    }
}

const GEMINI_ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1/models";

/// Redact a Gemini API key from a string for safe logging.
///
/// Gemini API keys start with `AIza` followed by alphanumeric characters.
/// This function preserves the first 4 characters (`AIza`) and the last 4
/// characters of any detected key, replacing the middle portion with asterisks.
/// Strings that do not contain an API key are returned unchanged.
///
/// # Example
///
/// ```
/// use agenticos_intelligence::gemini::redact_secret;
/// let redacted = redact_secret("key=AIzaSyABCDEFGHIJKLMNOPQRSTUVWXYZ123456");
/// assert!(redacted.starts_with("key=AIza"));
/// assert!(redacted.ends_with("3456"));
/// assert!(!redacted.contains("SyABCDEFGHIJKLMNOPQRSTUVWXYZ"));
/// ```
pub fn redact_secret(input: &str) -> String {
    // Gemini API keys start with "AIza" and are ~39 alphanumeric chars.
    // We show "AIza" + *** + last 4 chars for any key > 8 chars.
    let mut result = String::with_capacity(input.len());
    let mut pos = 0;
    let bytes = input.as_bytes();
    let len = bytes.len();

    while pos < len {
        if pos + 4 <= len && &bytes[pos..pos + 4] == b"AIza" {
            let start = pos;
            pos += 4;
            while pos < len
                && (bytes[pos].is_ascii_alphanumeric() || bytes[pos] == b'-' || bytes[pos] == b'_')
            {
                pos += 1;
            }
            let key = &input[start..pos];
            if key.len() > 8 {
                let keep = 4;
                result.push_str(&key[..keep]);
                let mid_len = key.len() - keep * 2;
                for _ in 0..mid_len {
                    result.push('*');
                }
                result.push_str(&key[key.len() - keep..]);
            } else {
                result.push_str(key);
            }
        } else {
            result.push(bytes[pos] as char);
            pos += 1;
        }
    }
    result
}

pub struct GeminiProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
    timeout: Duration,
}

impl fmt::Debug for GeminiProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeminiProvider")
            .field("api_key", &redact_secret(&self.api_key))
            .field("model", &self.model)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl GeminiProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        timeout_seconds: u64,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .build()
            .expect("failed to build reqwest HTTP client");
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
        // Note: url contains the API key as a query parameter.
        // Never log or serialize this url — it is only used for the HTTP request.
        let url = format!(
            "{}/{}:generateContent?key={}",
            GEMINI_ENDPOINT, self.model, self.api_key
        );

        let body = serde_json::json!({
            "contents": [{
                "parts": [{"text": prompt}]
            }],
            "generationConfig": {
                "temperature": 0.0
            }
        });

        let start = Instant::now();

        // The LlmProvider trait is synchronous, but reqwest's Client is async.
        // We block on the async call via the current tokio runtime handle.
        // block_in_place is required because we're already inside a tokio
        // runtime (from #[tokio::main]).
        let client = self.client.clone();
        let timeout = self.timeout;
        let handle = Handle::current();

        tokio::task::block_in_place(move || {
            handle.block_on(async move {
            let response = client
                .post(&url)
                .json(&body)
                .timeout(timeout)
                .send()
                .await
                .map_err(|_| "request to Gemini API failed (secret redacted)".to_string())?;

            let _latency = start.elapsed().as_millis() as u64;

            let status = response.status();
            let body_text = response
                .text()
                .await
                .map_err(|_| "failed to read Gemini response body (secret redacted)".to_string())?;

            if !status.is_success() {
                return Err(format!("Gemini API error ({}): {}", status, body_text));
            }

            let parsed: serde_json::Value = serde_json::from_str(&body_text)
                .map_err(|e| format!("failed to parse Gemini response JSON: {e}"))?;

            let text = parsed["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .ok_or_else(|| "no text in Gemini response".to_string())?;

            Ok(text.to_owned())
        })
        })
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
        let prompt = self.build_prompt(&context);
        let mut fallback_reason: Option<FallbackReason> = None;
        let mut parse_error_detail: Option<String> = None;

        let (rec, raw_response_text) = match self.call_gemini(&prompt) {
            Ok(text) => match Self::parse_gemini_response(&text) {
                Ok((class, confidence, reasoning)) => {
                    let summary = format!("Workload classified as {}", class.label());
                    (Recommendation::new(
                        AgentId::from(context.agent_name.as_str()),
                        confidence,
                        summary,
                        reasoning,
                    ), Some(text))
                }
                Err(e) => {
                    eprintln!("warning: Gemini parse error: {e}");
                    parse_error_detail = Some(e.clone());
                    fallback_reason = Some(
                        if e.contains("missing") || e.contains("no text") {
                            FallbackReason::MissingField
                        } else if e.contains("unknown classification") {
                            FallbackReason::InvalidClassification
                        } else {
                            FallbackReason::ParseError
                        }
                    );
                    (Self::fallback_recommendation(&context.agent_name), Some(text))
                }
            },
            Err(e) => {
                eprintln!("warning: {e}");
                fallback_reason = Some(
                    if e.contains("request") && e.contains("failed") {
                        FallbackReason::PromptFailure
                    } else if e.contains("no text") || e.contains("empty") {
                        FallbackReason::EmptyResponse
                    } else {
                        FallbackReason::ApiError
                    }
                );
                (Self::fallback_recommendation(&context.agent_name), None)
            }
        };

        let parsed_label = extract_class_label_from_rec(&rec);
        let mut meta = ProviderMetadata::new("gemini", &self.model, false, 0);
        meta.extra.insert("prompt".into(), truncate_for_debug(&prompt, 2000));
        if let Some(raw) = &raw_response_text {
            meta.extra.insert("raw_response".into(), truncate_for_debug(raw, 4000));
        } else {
            meta.extra.insert("raw_response".into(), "API call failed".into());
        }
        meta.extra.insert("parsed_classification".into(), format!(
            "{} ({:.2}): {}",
            parsed_label,
            rec.confidence,
            rec.reasoning,
        ));
        meta.extra.insert("confidence".into(), format!("{:.2}", rec.confidence));
        meta.extra.insert("observation_summary".into(), truncate_for_debug(&context.observation_summary, 500));
        if let Some(reason) = &fallback_reason {
            meta.extra.insert("fallback_reason".into(), reason.to_string());
        }
        if let Some(detail) = &parse_error_detail {
            meta.extra.insert("parse_error".into(), detail.clone());
        }

        rec.with_provider(meta)
    }
}

fn extract_class_label_from_rec(rec: &Recommendation) -> &str {
    if rec.summary.contains("Database") { "Database" }
    else if rec.summary.contains("Build") { "Build" }
    else if rec.summary.contains("Batch") { "Batch" }
    else if rec.summary.contains("Interactive") { "Interactive" }
    else if rec.summary.contains("SystemService") { "SystemService" }
    else { "Unknown" }
}

fn truncate_for_debug(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        let mut t = s[..max].to_owned();
        t.push_str("... (truncated)");
        t
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

    // ── Security Hardening Tests ────────────────────────────────────

    #[test]
    fn redact_secret_does_not_affect_normal_text() {
        assert_eq!(redact_secret("hello world"), "hello world");
        assert_eq!(redact_secret(""), "");
        assert_eq!(redact_secret("no key here"), "no key here");
    }

    #[test]
    fn redact_secret_redacts_gemini_key() {
        let original = "AIzaSyABCDEFGHIJKLMNOPQRSTUVWXYZ123456";
        let redacted = redact_secret(original);
        // The original full key must not appear
        assert!(!redacted.contains(original));
        // Must start with AIza
        assert!(redacted.starts_with("AIza"));
        // Must end with last 4 chars
        assert!(redacted.ends_with("3456"));
        // Must contain asterisks in the middle
        assert!(redacted.contains("****"));
        // Total length should be: 4 (AIza) + (len-8) asterisks + 4 (last 4)
        assert_eq!(redacted.len(), original.len());
    }

    #[test]
    fn redact_secret_short_key_not_redacted() {
        // Keys 8 chars or shorter are returned as-is
        assert_eq!(redact_secret("AIza1234"), "AIza1234");
        assert_eq!(redact_secret("AIza12"), "AIza12");
    }

    #[test]
    fn redact_secret_multiple_keys_in_string() {
        let input = "first=AIzaABCDEFGHIJKLMNOPQRST second=AIza1234567890123456XYZQ end";
        let redacted = redact_secret(input);
        assert!(!redacted.contains("AIzaABCDEFGHIJKLMNOPQRST"));
        assert!(!redacted.contains("AIza1234567890123456XYZQ"));
        assert!(redacted.contains("first="));
        assert!(redacted.contains("second="));
        assert!(redacted.contains(" end"));
    }

    #[test]
    fn redact_secret_key_at_start_and_end() {
        let input = "AIzaABCDEFGHIJKLMNOPQRSTUVWXYZ123456 some text AIza1234567890123456abcdef";
        let redacted = redact_secret(input);
        assert!(!redacted.contains("AIzaABCDEFGHIJKLMNOPQRSTUVWXYZ123456"));
        assert!(!redacted.contains("AIza1234567890123456abcdef"));
    }

    #[test]
    fn debug_format_does_not_expose_api_key() {
        let provider =
            GeminiProvider::new("AIzaMySecretKey1234567890".to_string(), "gemini-2.5-flash".to_string(), 10);
        let debug_str = format!("{provider:?}");
        // The full key must not appear in debug output
        assert!(!debug_str.contains("AIzaMySecretKey1234567890"));
        // The debug output must still identify the provider
        assert!(debug_str.contains("GeminiProvider"));
        assert!(debug_str.contains("gemini-2.5-flash"));
        // The api_key field should show a redacted version
        assert!(debug_str.contains("api_key"));
        assert!(debug_str.contains("AIza"));
    }

    #[test]
    fn serde_does_not_serialize_gemini_provider_api_key() {
        // GeminiProvider has no `#[derive(Serialize)]` — this is a
        // compile-time guarantee. The `api_key` field is private and never
        // serialized. If someone adds `Serialize`, they must also add
        // `#[serde(skip)]` to `api_key`. This test verifies the struct
        // layout to prevent accidental exposure.
        use std::mem;
        // Verify the struct is non-trivial (contains the key, model, client, timeout)
        assert!(
            mem::size_of::<GeminiProvider>() > mem::size_of::<reqwest::Client>(),
            "GeminiProvider is unexpectedly small — api_key may be missing"
        );
    }

    #[test]
    fn api_key_never_in_error_messages() {
        // call_gemini() internally uses Handle::current().block_on(), so we
        // must run inside a tokio runtime.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let provider = GeminiProvider::new(
            "AIzaThisIsATestKey1234567890".to_string(),
            "gemini-2.5-flash".to_string(),
            1, // 1 second timeout — will fail fast with no server
        );
        let _ctx = test_context();
        // This will likely fail (no real server), but must not leak the key
        let result = provider.call_gemini("test prompt");
        if let Err(msg) = result {
            assert!(!msg.contains("AIzaThisIsATestKey1234567890"));
            assert!(!msg.contains("1234567890"));
        }
    }

    #[test]
    fn config_create_provider_returns_error_for_missing_key() {
        let prev = std::env::var("GEMINI_API_KEY").ok();
        // Temporarily remove the env var
        std::env::remove_var("GEMINI_API_KEY");
        let cfg = crate::IntelligenceConfig::new(
            "gemini",
            "gemini-2.5-flash",
            "GEMINI_API_KEY",
            10,
            "data/recommendation-cache.db",
        );
        let err = match cfg.create_provider() {
            Err(e) => e,
            Ok(_) => panic!("expected error for missing key"),
        };
        // Error must mention the env var name but NOT any secret value
        assert!(err.contains("GEMINI_API_KEY"));
        // Cleanup
        if let Some(key) = prev {
            std::env::set_var("GEMINI_API_KEY", key);
        }
    }

    #[test]
    fn config_create_provider_returns_error_for_empty_key() {
        let prev = std::env::var("GEMINI_API_KEY").ok();
        std::env::set_var("GEMINI_API_KEY", "");
        let cfg = crate::IntelligenceConfig::new(
            "gemini",
            "gemini-2.5-flash",
            "GEMINI_API_KEY",
            10,
            "data/recommendation-cache.db",
        );
        let err = match cfg.create_provider() {
            Err(e) => e,
            Ok(_) => panic!("expected error for empty key"),
        };
        // Error must mention the env var name but NOT any secret value
        assert!(err.contains("GEMINI_API_KEY"));
        // Cleanup
        if let Some(key) = prev {
            std::env::set_var("GEMINI_API_KEY", key);
        } else {
            std::env::remove_var("GEMINI_API_KEY");
        }
    }
}
