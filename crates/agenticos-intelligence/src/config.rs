use agenticos_domain::ProviderMetadata;

use crate::{LlmProvider, MockProvider, RecommendationCache};

pub struct IntelligenceConfig {
    pub provider_name: String,
    pub model: String,
    pub api_key_env: String,
    pub timeout_seconds: u64,
    pub cache_path: String,
}

impl Default for IntelligenceConfig {
    fn default() -> Self {
        Self {
            provider_name: "mock".into(),
            model: "gemini-2.5-flash".into(),
            api_key_env: "GEMINI_API_KEY".into(),
            timeout_seconds: 10,
            cache_path: "data/recommendation-cache.db".into(),
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
        }
    }

    pub fn create_provider(&self) -> Box<dyn LlmProvider> {
        let inner: Box<dyn LlmProvider> = match self.provider_name.as_str() {
            "gemini" => {
                let api_key = std::env::var(&self.api_key_env).unwrap_or_default();
                if api_key.is_empty() {
                    eprintln!(
                        "warning: GEMINI_API_KEY not set, falling back to MockProvider"
                    );
                    Box::new(MockProvider)
                } else {
                    Box::new(crate::GeminiProvider::new(
                        api_key,
                        &self.model,
                        self.timeout_seconds,
                    ))
                }
            }
            _ => Box::new(MockProvider),
        };

        match RecommendationCache::new(&self.cache_path) {
            Ok(cache) => Box::new(crate::CachedLlmProvider::new(inner, cache)),
            Err(_) => inner,
        }
    }

    pub fn provider_metadata(&self) -> ProviderMetadata {
        ProviderMetadata::new(&self.provider_name, &self.model, false, 0)
    }
}
