use agenticos_domain::ProviderMetadata;
use serde::Deserialize;

use crate::{LlmProvider, MockProvider, RecommendationCache};

#[derive(Clone, Debug, Deserialize)]
pub struct IntelligenceConfig {
    #[serde(rename = "provider", default = "default_provider")]
    pub provider_name: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(rename = "api_key_env", default = "default_api_key_env")]
    pub api_key_env: String,
    #[serde(rename = "timeout_seconds", default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_cache_path")]
    pub cache_path: String,
    #[serde(rename = "classification_cooldown_seconds", default = "default_cooldown")]
    pub classification_cooldown_seconds: u64,
}

fn default_provider() -> String { "mock".into() }
fn default_model() -> String { "gemini-2.5-flash".into() }
fn default_api_key_env() -> String { "GEMINI_API_KEY".into() }
fn default_timeout() -> u64 { 10 }
fn default_cache_path() -> String { "data/recommendation-cache.db".into() }
fn default_cooldown() -> u64 { 60 }

impl Default for IntelligenceConfig {
    fn default() -> Self {
        Self {
            provider_name: "mock".into(),
            model: "gemini-2.5-flash".into(),
            api_key_env: "GEMINI_API_KEY".into(),
            timeout_seconds: 10,
            cache_path: "data/recommendation-cache.db".into(),
            classification_cooldown_seconds: 60,
        }
    }
}

impl IntelligenceConfig {
    pub fn new(
        provider_name: impl Into<String>,
        model: impl Into<String>,
        api_key_env: impl Into<String>,
        timeout_seconds: u64,
        cache_path: impl Into<String>,
    ) -> Self {
        Self {
            provider_name: provider_name.into(),
            model: model.into(),
            api_key_env: api_key_env.into(),
            timeout_seconds,
            cache_path: cache_path.into(),
            classification_cooldown_seconds: 60,
        }
    }

    pub fn cooldown_seconds(&self) -> u64 {
        self.classification_cooldown_seconds
    }

    /// Create a provider based on configuration.
    ///
    /// Returns an error if the provider is "gemini" but the API key environment
    /// variable is missing or empty. The error message does not include the
    /// secret value.
    pub fn create_provider(&self) -> Result<Box<dyn LlmProvider>, String> {
        let inner: Box<dyn LlmProvider> = match self.provider_name.as_str() {
            "gemini" => {
                let api_key = std::env::var(&self.api_key_env).map_err(|_| {
                    format!(
                        "Gemini API key not found in environment variable {}",
                        self.api_key_env
                    )
                })?;
                if api_key.is_empty() {
                    return Err(format!(
                        "Gemini API key environment variable {} is empty",
                        self.api_key_env
                    ));
                }
                Box::new(crate::GeminiProvider::new(
                    api_key,
                    &self.model,
                    self.timeout_seconds,
                ))
            }
            _ => Box::new(MockProvider::with_model(&self.model)),
        };

        match RecommendationCache::new(&self.cache_path) {
            Ok(cache) => Ok(Box::new(crate::CachedLlmProvider::with_metadata(
                inner,
                cache,
                &self.provider_name,
                &self.model,
            ))),
            Err(_) => Ok(inner),
        }
    }

    /// Returns `true` if the API key environment variable is set and non-empty.
    /// Does NOT expose the key value — only its presence is checked.
    pub fn api_key_present(&self) -> bool {
        std::env::var(&self.api_key_env)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    pub fn provider_metadata(&self) -> ProviderMetadata {
        ProviderMetadata::new(&self.provider_name, &self.model, false, 0)
    }
}
