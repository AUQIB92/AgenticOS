use std::sync::Mutex;
use std::time::Instant;

use agenticos_domain::{ProviderMetadata, Recommendation};
use crate::RecommendationContext;

use sha2::{Digest, Sha256};

const CREATE_TABLE: &str = "
CREATE TABLE IF NOT EXISTS recommendation_cache (
    cache_key TEXT PRIMARY KEY,
    recommendation_json TEXT NOT NULL,
    created_at TEXT NOT NULL
)";

pub struct RecommendationCache {
    conn: Mutex<rusqlite::Connection>,
}

impl RecommendationCache {
    pub fn new(path: &str) -> Result<Self, String> {
        let conn = rusqlite::Connection::open(path)
            .map_err(|e| format!("failed to open cache db: {e}"))?;
        conn.execute(CREATE_TABLE, [])
            .map_err(|e| format!("failed to create cache table: {e}"))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn in_memory() -> Result<Self, String> {
        let conn = rusqlite::Connection::open_in_memory()
            .map_err(|e| format!("failed to open in-memory cache: {e}"))?;
        conn.execute(CREATE_TABLE, [])
            .map_err(|e| format!("failed to create cache table: {e}"))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn get(&self, key: &str) -> Result<Option<Recommendation>, String> {
        let conn = self.conn.lock().map_err(|e| format!("lock: {e}"))?;
        let result: Result<Option<String>, _> = conn.query_row(
            "SELECT recommendation_json FROM recommendation_cache WHERE cache_key = ?1",
            [key],
            |row| row.get(0),
        );
        match result {
            Ok(Some(json)) => {
                let rec: Recommendation =
                    serde_json::from_str(&json).map_err(|e| format!("deserialize: {e}"))?;
                Ok(Some(rec))
            }
            Ok(None) => Ok(None),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("query error: {e}")),
        }
    }

    pub fn put(&self, key: &str, rec: &Recommendation) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("lock: {e}"))?;
        let json = serde_json::to_string(rec).map_err(|e| format!("serialize: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO recommendation_cache (cache_key, recommendation_json, created_at)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![key, json, &rec.timestamp],
        )
        .map_err(|e| format!("insert: {e}"))?;
        Ok(())
    }

    pub fn len(&self) -> Result<usize, String> {
        let conn = self.conn.lock().map_err(|e| format!("lock: {e}"))?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM recommendation_cache", [], |row| {
                row.get(0)
            })
            .map_err(|e| format!("count: {e}"))?;
        Ok(count as usize)
    }

    pub fn is_empty(&self) -> Result<bool, String> {
        self.len().map(|n| n == 0)
    }

    pub fn key_from_context(ctx: &RecommendationContext) -> String {
        let input = format!(
            "obs={}|agent={}|sys={}",
            ctx.observation_summary, ctx.agent_name, ctx.system_state_summary
        );
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        hex::encode(hasher.finalize())
    }
}

pub struct CachedLlmProvider<P: crate::LlmProvider> {
    inner: P,
    cache: RecommendationCache,
    hits: Mutex<u64>,
    misses: Mutex<u64>,
    provider_name: String,
    model_name: String,
}

impl<P: crate::LlmProvider> CachedLlmProvider<P> {
    pub fn new(inner: P, cache: RecommendationCache) -> Self {
        Self {
            inner,
            cache,
            hits: Mutex::new(0),
            misses: Mutex::new(0),
            provider_name: String::new(),
            model_name: String::new(),
        }
    }

    pub fn with_metadata(
        inner: P,
        cache: RecommendationCache,
        provider_name: impl Into<String>,
        model_name: impl Into<String>,
    ) -> Self {
        Self {
            inner,
            cache,
            hits: Mutex::new(0),
            misses: Mutex::new(0),
            provider_name: provider_name.into(),
            model_name: model_name.into(),
        }
    }

    pub fn cache_hits(&self) -> u64 {
        *self.hits.lock().unwrap()
    }

    pub fn cache_misses(&self) -> u64 {
        *self.misses.lock().unwrap()
    }

    pub fn inner_cache(&self) -> &RecommendationCache {
        &self.cache
    }
}

impl<P: crate::LlmProvider> crate::LlmProvider for CachedLlmProvider<P> {
    fn generate_recommendation(&self, context: RecommendationContext) -> Recommendation {
        let key = RecommendationCache::key_from_context(&context);
        let provider_name = if self.provider_name.is_empty() {
            "cached"
        } else {
            &self.provider_name
        };
        let model_name = if self.model_name.is_empty() {
            "cached"
        } else {
            &self.model_name
        };

        if let Ok(Some(rec)) = self.cache.get(&key) {
            *self.hits.lock().unwrap() += 1;
            let inner_extra = rec.provider.as_ref().map(|p| p.extra.clone()).unwrap_or_default();
            let mut hit_meta = ProviderMetadata::new(provider_name, model_name, true, 0);
            hit_meta.extra = inner_extra;
            return rec.with_provider(hit_meta);
        }

        *self.misses.lock().unwrap() += 1;
        let start = Instant::now();
        let rec = self.inner.generate_recommendation(context);
        let latency_ms = start.elapsed().as_millis() as u64;

        // Preserve inner provider's extra fields (e.g. debug info from GeminiProvider)
        let inner_extra = rec.provider.as_ref().map(|p| p.extra.clone()).unwrap_or_default();
        let mut new_meta = ProviderMetadata::new(provider_name, model_name, false, latency_ms);
        new_meta.extra = inner_extra;
        let cached_rec = rec.with_provider(new_meta);

        let _ = self.cache.put(&key, &cached_rec);
        cached_rec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RecommendationContext;
    use agenticos_domain::AgentId;

    fn test_context() -> RecommendationContext {
        RecommendationContext::new("cpu 85% procs 14", "test-agent", "normal system state")
    }

    #[test]
    fn cache_key_is_deterministic() {
        let ctx1 = test_context();
        let ctx2 = test_context();
        assert_eq!(
            RecommendationCache::key_from_context(&ctx1),
            RecommendationCache::key_from_context(&ctx2)
        );
    }

    #[test]
    fn cache_key_differs_for_different_contexts() {
        let ctx1 = test_context();
        let ctx2 =
            RecommendationContext::new("mem 90% procs 8", "other-agent", "high memory pressure");
        assert_ne!(
            RecommendationCache::key_from_context(&ctx1),
            RecommendationCache::key_from_context(&ctx2)
        );
    }

    #[test]
    fn cache_put_and_get_round_trip() {
        let cache = RecommendationCache::in_memory().unwrap();
        let key = "test-key-1";
        let rec = agenticos_domain::Recommendation::new(
            AgentId::from("test"),
            0.85,
            "test summary",
            "test reasoning",
        );

        cache.put(key, &rec).unwrap();
        let retrieved = cache.get(key).unwrap().unwrap();
        assert_eq!(retrieved.id, rec.id);
        assert_eq!(retrieved.summary, rec.summary);
        assert_eq!(retrieved.confidence, rec.confidence);
    }

    #[test]
    fn cache_get_missing_returns_none() {
        let cache = RecommendationCache::in_memory().unwrap();
        let result = cache.get("nonexistent-key").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn cache_overwrites_existing_entry() {
        let cache = RecommendationCache::in_memory().unwrap();
        let key = "overwrite-key";
        let rec1 = agenticos_domain::Recommendation::new(
            AgentId::from("test"), 0.5, "original", "original reasoning",
        );
        let rec2 = agenticos_domain::Recommendation::new(
            AgentId::from("test"), 0.9, "updated", "updated reasoning",
        );

        cache.put(key, &rec1).unwrap();
        cache.put(key, &rec2).unwrap();
        let retrieved = cache.get(key).unwrap().unwrap();
        assert_eq!(retrieved.summary, "updated");
        assert_eq!(retrieved.confidence, 0.9);
    }

    #[test]
    fn cache_len_tracks_entries() {
        let cache = RecommendationCache::in_memory().unwrap();
        assert_eq!(cache.len().unwrap(), 0);

        let rec = agenticos_domain::Recommendation::new(
            AgentId::from("test"), 0.5, "s", "r",
        );
        cache.put("key-a", &rec).unwrap();
        assert_eq!(cache.len().unwrap(), 1);

        cache.put("key-b", &rec).unwrap();
        assert_eq!(cache.len().unwrap(), 2);
    }

    #[test]
    fn cache_key_empty_context() {
        let ctx = RecommendationContext::new("", "", "");
        let key = RecommendationCache::key_from_context(&ctx);
        assert!(!key.is_empty());
        assert_eq!(key.len(), 64);
    }

    #[test]
    fn cached_provider_metadata_marks_cache_hit() {
        let cache = RecommendationCache::in_memory().unwrap();
        let ctx = test_context();
        let key = RecommendationCache::key_from_context(&ctx);
        let rec = agenticos_domain::Recommendation::new(
            AgentId::from("test"),
            0.85,
            "cached result",
            "cached reasoning",
        );

        cache.put(&key, &rec).unwrap();
        let retrieved = cache.get(&key).unwrap().unwrap();
        assert!(
            retrieved.provider.is_none(),
            "cache-stored rec should have no provider metadata yet"
        );
    }

    #[test]
    fn cached_provider_preserves_recommendation_id() {
        let cache = RecommendationCache::in_memory().unwrap();
        let key = "id-preserve-test";
        let rec = agenticos_domain::Recommendation::new(
            AgentId::from("test"), 0.5, "s", "r",
        );
        let original_id = rec.id.clone();

        cache.put(key, &rec).unwrap();
        let retrieved = cache.get(key).unwrap().unwrap();
        assert_eq!(retrieved.id, original_id);
    }
}
