use std::collections::HashMap;

use agenticos_domain::Intent;
use tokio::runtime::Handle;

use crate::gemini::redact_secret;

const GEMINI_ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1/models";
const INTENT_PROMPT: &str = r#"You are an intent parsing system. Extract the user's intent from the given text.
Return ONLY valid JSON with no markdown formatting. Use this exact schema:
{
  "intent_type": "launch_application" | "create_project" | "open_file" | "open_url" | "run_command" | "create_directory" | "unknown",
  "parameters": {
    "application": "",
    "framework": "",
    "project_name": "",
    "url": "",
    "file_name": "",
    "directory": "",
    "repository_url": "",
    "command": ""
  },
  "confidence": 1.0
}
Populate only the parameter keys that are relevant. Set others to empty string.
User text: {text}"#;

pub trait IntentParser: Send + Sync {
    fn parse_intent(&self, text: &str) -> Result<Intent, String>;
}

// ---------------------------------------------------------------------------
// MockIntentParser — deterministic parser for testing
// ---------------------------------------------------------------------------

pub struct MockIntentParser;

impl MockIntentParser {
    pub fn new() -> Self {
        Self
    }
}

fn has_any_scheme(word: &str) -> bool {
    // A URL scheme matches [a-zA-Z][a-zA-Z0-9+.-]*:
    // Check for :// (http://, https://, ftp://, file://, etc.)
    // OR a word:word pattern where the prefix before : is alphabetic
    // (mailto:user@x, javascript:alert(1), etc.)
    if word.contains("://") {
        return true;
    }
    if let Some(colons) = word.find(':') {
        if colons > 0 {
            let candidate = &word[..colons];
            return candidate.chars().all(|c| c.is_ascii_alphabetic() || c == '+' || c == '-' || c == '.');
        }
    }
    false
}

fn extract_url(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|w| {
        if has_any_scheme(w) {
            // Existing scheme — preserve EXACTLY (file://, ftp://, mailto:, etc.)
            Some(w.trim_end_matches(&[',', '.', '!', '?', ';', ':', ')', ']'][..]).to_string())
        } else if w.contains(".com") || w.contains(".org") || w.contains(".io") {
            // No scheme — prepend https://
            let clean = w.trim_end_matches(&[',', '.', '!', '?', ';', ':', ')', ']'][..]);
            Some(format!("https://{}", clean))
        } else {
            None
        }
    })
}

fn extract_project_name(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    for marker in &["called ", "named "] {
        if let Some(idx) = lower.find(marker) {
            let after = text[idx + marker.len()..].trim();
            let name = after.split_whitespace().next()?;
            if !name.is_empty() && !name.contains('.') && !name.contains('/') {
                return Some(name.trim_end_matches(&[',', '.', '!', '?'][..]).to_string());
            }
        }
    }
    None
}

fn extract_application(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    let known = [
        ("vscode", "vscode"),
        ("code", "vscode"),
        ("firefox", "firefox"),
        ("browser", "firefox"),
        ("chrome", "chrome"),
        ("terminal", "terminal"),
        ("calculator", "calculator"),
        ("edge", "edge"),
        ("slack", "slack"),
        ("discord", "discord"),
        ("spotify", "spotify"),
    ];
    for (keyword, app) in &known {
        if lower.contains(keyword) {
            return Some(app);
        }
    }
    None
}

fn extract_framework(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    if lower.contains("nextjs") || lower.contains("next.js") || lower.contains("next ") {
        Some("nextjs")
    } else if lower.contains("react") {
        Some("react")
    } else if lower.contains("vue") || lower.contains("vuejs") {
        Some("vue")
    } else if lower.contains("angular") {
        Some("angular")
    } else if lower.contains("svelte") {
        Some("svelte")
    } else if lower.contains("django") {
        Some("django")
    } else if lower.contains("rails") || lower.contains("ruby on rails") {
        Some("rails")
    } else {
        None
    }
}

fn has_word(text: &str, needles: &[&str]) -> bool {
    let lower = text.to_lowercase();
    lower.split_whitespace().any(|tw| {
        let clean = tw.trim_end_matches(|c: char| !c.is_alphanumeric());
        needles.contains(&clean)
    })
}

fn mock_parse(text: &str) -> (String, HashMap<String, String>, f64) {
    let lower = text.to_lowercase();
    let mut params = HashMap::new();

    // clone repository
    if lower.contains("clone ") && (lower.contains("http") || lower.contains("github") || lower.contains("git@") || lower.contains("repo")) {
        if let Some(url) = extract_url(text) {
            params.insert("repository_url".into(), url);
        }
        if let Some(name) = extract_project_name(text) {
            params.insert("project_name".into(), name);
        }
        return ("create_project".into(), params, 0.85);
    }

    // create directory/folder (check BEFORE create_project to avoid "my-project" false match)
    if lower.contains("create") && has_word(&lower, &["folder", "directory"]) {
        if let Some(name) = extract_project_name(text) {
            params.insert("directory".into(), name);
        } else {
            let name = text.split_whitespace().last().unwrap_or("new_folder").to_string();
            params.insert("directory".into(), name);
        }
        return ("create_directory".into(), params, 0.80);
    }

    // create with project/app — extract framework AND project_name
    if lower.contains("create") && has_word(&lower, &["project", "app"]) {
        if let Some(fw) = extract_framework(text) {
            params.insert("framework".into(), fw.into());
        } else {
            params.insert("framework".into(), "generic".into());
        }
        if let Some(name) = extract_project_name(text) {
            params.insert("project_name".into(), name);
        }
        if let Some(url) = extract_url(text) {
            params.insert("repository_url".into(), url);
        }
        return ("create_project".into(), params, 0.85);
    }

    // open/launch/start — extract application AND url
    if lower.starts_with("open ") || lower.starts_with("launch ") || lower.starts_with("start ") {
        let has_known_app = extract_application(text).is_some();

        if let Some(app) = extract_application(text) {
            params.insert("application".into(), app.into());
        }

        if let Some(url) = extract_url(text) {
            params.insert("url".into(), url);
        }

        if has_known_app {
            return ("launch_application".into(), params, 0.9);
        }

        if params.contains_key("url") {
            return ("open_url".into(), params, 0.85);
        }

        // fallback to open_file
        let rest = text[4..].trim();
        if !rest.is_empty() {
            params.insert("file_name".into(), rest.to_string());
            return ("open_file".into(), params, 0.85);
        }
    }

    // run command (check after structured intents, before bare URL)
    if lower.starts_with("run ") {
        let cmd_text = text[4..].trim();
        if !cmd_text.is_empty() {
            params.insert("command".into(), cmd_text.to_string());
            return ("run_command".into(), params, 0.85);
        }
    }
    if lower.starts_with("execute ") {
        let cmd_text = text[8..].trim();
        if !cmd_text.is_empty() {
            params.insert("command".into(), cmd_text.to_string());
            return ("run_command".into(), params, 0.85);
        }
    }

    // bare URL (no open/launch prefix)
    if lower.contains("://") || lower.contains(".com") {
        if let Some(url) = extract_url(text) {
            params.insert("url".into(), url);
            return ("open_url".into(), params, 0.85);
        }
    }

    ("unknown".into(), params, 0.50)
}

impl IntentParser for MockIntentParser {
    fn parse_intent(&self, text: &str) -> Result<Intent, String> {
        let (intent_type, params, confidence) = mock_parse(text);
        Ok(Intent::new(text, intent_type, params, confidence))
    }
}

// ---------------------------------------------------------------------------
// GeminiIntentParser — uses Gemini API for intent parsing
// ---------------------------------------------------------------------------

pub struct GeminiIntentParser {
    api_key: String,
    model: String,
    client: reqwest::Client,
    timeout: std::time::Duration,
}

impl GeminiIntentParser {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        timeout_seconds: u64,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_seconds))
            .build()
            .expect("GeminiIntentParser: failed to build reqwest client");
        Self {
            api_key: api_key.into(),
            model: model.into(),
            client,
            timeout: std::time::Duration::from_secs(timeout_seconds),
        }
    }

    fn build_prompt(&self, text: &str) -> String {
        INTENT_PROMPT.replace("{text}", text)
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
                "temperature": 0.0
            }
        });

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

                let status = response.status();
                let body_text = response
                    .text()
                    .await
                    .map_err(|_| "failed to read Gemini response body (secret redacted)".to_string())?;

                if !status.is_success() {
                    let redacted = redact_secret(&body_text);
                    return Err(format!("Gemini API error ({}): {}", status, redacted));
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

    fn parse_intent_response(&self, raw: &str) -> Result<(String, HashMap<String, String>, f64), String> {
        let cleaned = raw
            .trim()
            .strip_prefix("```json")
            .or_else(|| raw.trim().strip_prefix("```"))
            .map(|s| s.trim_end_matches("```").trim())
            .unwrap_or(raw.trim());

        let v: serde_json::Value =
            serde_json::from_str(cleaned).map_err(|e| format!("failed to parse intent JSON: {e}"))?;

        let intent_type = v["intent_type"]
            .as_str()
            .or_else(|| v["intent"].as_str())
            .ok_or_else(|| "missing 'intent_type' or 'intent' field in Gemini response".to_string())?
            .to_string();

        let confidence = v["confidence"].as_f64().unwrap_or(0.5);

        let mut params = HashMap::new();
        if let Some(obj) = v["parameters"].as_object() {
            for (k, val) in obj {
                if let Some(s) = val.as_str() {
                    if !s.is_empty() {
                        params.insert(k.clone(), s.to_string());
                    }
                }
            }
        }

        Ok((intent_type, params, confidence))
    }
}

impl IntentParser for GeminiIntentParser {
    fn parse_intent(&self, text: &str) -> Result<Intent, String> {
        let prompt = self.build_prompt(text);
        let raw = self.call_gemini(&prompt)?;
        let (intent_type, params, confidence) = self.parse_intent_response(&raw)?;
        Ok(Intent::new(text, intent_type, params, confidence))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MockIntentParser tests ────────────────────────────────────────

    #[test]
    fn mock_parses_launch_application() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Open VS Code").unwrap();
        assert_eq!(intent.intent_type, "launch_application");
        assert_eq!(intent.parameters.get("application").unwrap(), "vscode");
        assert!((intent.confidence - 0.9).abs() < 0.01);
    }

    #[test]
    fn mock_parses_create_project() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Create a Next.js project").unwrap();
        assert_eq!(intent.intent_type, "create_project");
        assert_eq!(intent.parameters.get("framework").unwrap(), "nextjs");
    }

    #[test]
    fn mock_parses_create_folder() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Create a folder called my-project").unwrap();
        assert_eq!(intent.intent_type, "create_directory");
        assert!(intent.parameters.contains_key("directory"));
    }

    #[test]
    fn mock_parses_unknown() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("How is the weather?").unwrap();
        assert_eq!(intent.intent_type, "unknown");
        assert!((intent.confidence - 0.50).abs() < 0.01);
    }

    #[test]
    fn mock_parses_open_url() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Open https://example.com").unwrap();
        assert_eq!(intent.intent_type, "open_url");
        assert_eq!(intent.parameters.get("url").unwrap(), "https://example.com");
    }

    #[test]
    fn mock_is_deterministic() {
        let parser = MockIntentParser::new();
        let a = parser.parse_intent("Open VS Code").unwrap();
        let b = parser.parse_intent("Open VS Code").unwrap();
        assert_eq!(a.intent_type, b.intent_type);
        assert_eq!(a.parameters, b.parameters);
    }

    // ── Multi-parameter extraction tests ──────────────────────────────

    #[test]
    fn mock_url_preserves_existing_schemes() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Open file:///etc/shadow").unwrap();
        assert_eq!(intent.intent_type, "open_url");
        assert_eq!(intent.parameters.get("url").unwrap(), "file:///etc/shadow");
    }

    #[test]
    fn mock_url_handles_bare_github() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Open github.com").unwrap();
        assert_eq!(intent.intent_type, "open_url");
        assert_eq!(intent.parameters.get("url").unwrap(), "https://github.com");
    }

    #[test]
    fn mock_url_preserves_ftp_scheme() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Open ftp://example.com/file").unwrap();
        assert_eq!(intent.intent_type, "open_url");
        assert_eq!(intent.parameters.get("url").unwrap(), "ftp://example.com/file");
    }

    #[test]
    fn mock_url_preserves_mailto() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Open mailto:user@example.com").unwrap();
        assert_eq!(intent.intent_type, "open_url");
        assert_eq!(intent.parameters.get("url").unwrap(), "mailto:user@example.com");
    }

    #[test]
    fn mock_extracts_application_and_url() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Open Firefox and go to github.com").unwrap();
        assert_eq!(intent.intent_type, "launch_application");
        assert_eq!(intent.parameters.get("application").unwrap(), "firefox");
        assert_eq!(intent.parameters.get("url").unwrap(), "https://github.com");
    }

    #[test]
    fn mock_extracts_framework_and_project_name() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Create a Next.js project called examgenius").unwrap();
        assert_eq!(intent.intent_type, "create_project");
        assert_eq!(intent.parameters.get("framework").unwrap(), "nextjs");
        assert_eq!(intent.parameters.get("project_name").unwrap(), "examgenius");
    }

    #[test]
    fn mock_extracts_repository_url() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Clone https://github.com/example/repo").unwrap();
        assert_eq!(intent.intent_type, "create_project");
        assert_eq!(
            intent.parameters.get("repository_url").unwrap(),
            "https://github.com/example/repo"
        );
    }

    #[test]
    fn mock_extracts_project_name_from_clone() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Clone the repo called my-app").unwrap();
        assert_eq!(intent.intent_type, "create_project");
        assert_eq!(intent.parameters.get("project_name").unwrap(), "my-app");
    }

    #[test]
    fn mock_extracts_directory_name() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Create a directory called work").unwrap();
        assert_eq!(intent.intent_type, "create_directory");
        assert_eq!(intent.parameters.get("directory").unwrap(), "work");
    }

    #[test]
    fn mock_parses_run_command() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Run cargo build").unwrap();
        assert_eq!(intent.intent_type, "run_command");
        assert_eq!(intent.parameters.get("command").unwrap(), "cargo build");
        assert!((intent.confidence - 0.85).abs() < 0.01);
    }

    #[test]
    fn mock_parses_run_command_execute() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Execute ls -la").unwrap();
        assert_eq!(intent.intent_type, "run_command");
        assert_eq!(intent.parameters.get("command").unwrap(), "ls -la");
    }

    #[test]
    fn mock_run_command_rejects_empty() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Run").unwrap();
        assert_eq!(intent.intent_type, "unknown");
    }

    #[test]
    fn mock_parses_open_firefox() {
        let parser = MockIntentParser::new();
        let intent = parser.parse_intent("Launch Firefox").unwrap();
        assert_eq!(intent.intent_type, "launch_application");
        assert_eq!(intent.parameters.get("application").unwrap(), "firefox");
    }

    // ── GeminiIntentParser response parsing tests ─────────────────────

    #[test]
    fn parse_valid_intent_response() {
        let parser = GeminiIntentParser::new("test-key", "gemini-2.5-flash", 10);
        let json = r#"{"intent_type":"launch_application","parameters":{"application":"vscode"},"confidence":0.95}"#;
        let (intent_type, params, confidence) = parser.parse_intent_response(json).unwrap();
        assert_eq!(intent_type, "launch_application");
        assert_eq!(params.get("application").unwrap(), "vscode");
        assert!((confidence - 0.95).abs() < 0.01);
    }

    #[test]
    fn parse_intent_response_backward_compat() {
        // Old field name "intent" must still be accepted
        let parser = GeminiIntentParser::new("test-key", "gemini-2.5-flash", 10);
        let json = r#"{"intent":"launch_application","parameters":{"application":"vscode"},"confidence":0.95}"#;
        let (intent_type, params, confidence) = parser.parse_intent_response(json).unwrap();
        assert_eq!(intent_type, "launch_application");
        assert_eq!(params.get("application").unwrap(), "vscode");
    }

    #[test]
    fn parse_intent_with_markdown_fence() {
        let parser = GeminiIntentParser::new("test-key", "gemini-2.5-flash", 10);
        let json = "```json\n{\"intent_type\":\"create_project\",\"parameters\":{\"framework\":\"nextjs\",\"project_name\":\"myapp\"},\"confidence\":0.88}\n```";
        let (intent_type, params, confidence) = parser.parse_intent_response(json).unwrap();
        assert_eq!(intent_type, "create_project");
        assert_eq!(params.get("framework").unwrap(), "nextjs");
        assert_eq!(params.get("project_name").unwrap(), "myapp");
        assert!((confidence - 0.88).abs() < 0.01);
    }

    #[test]
    fn parse_missing_intent_field() {
        let parser = GeminiIntentParser::new("test-key", "gemini-2.5-flash", 10);
        let json = r#"{"confidence":0.5}"#;
        assert!(parser.parse_intent_response(json).is_err());
    }

    #[test]
    fn parse_malformed_json() {
        let parser = GeminiIntentParser::new("test-key", "gemini-2.5-flash", 10);
        assert!(parser.parse_intent_response("not json").is_err());
    }

    #[test]
    fn parse_unknown_intent() {
        let parser = GeminiIntentParser::new("test-key", "gemini-2.5-flash", 10);
        let json = r#"{"intent_type":"unknown","parameters":{},"confidence":0.4}"#;
        let (intent_type, params, confidence) = parser.parse_intent_response(json).unwrap();
        assert_eq!(intent_type, "unknown");
        assert!(params.is_empty());
        assert!((confidence - 0.4).abs() < 0.01);
    }

    #[test]
    fn parse_multi_param_response() {
        let parser = GeminiIntentParser::new("test-key", "gemini-2.5-flash", 10);
        let json = r#"{"intent_type":"launch_application","parameters":{"application":"firefox","url":"https://github.com"},"confidence":0.9}"#;
        let (intent_type, params, confidence) = parser.parse_intent_response(json).unwrap();
        assert_eq!(intent_type, "launch_application");
        assert_eq!(params.get("application").unwrap(), "firefox");
        assert_eq!(params.get("url").unwrap(), "https://github.com");
        assert!((confidence - 0.9).abs() < 0.01);
    }
}
