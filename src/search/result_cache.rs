//! Search Result Caching with Adaptive Thresholds (Phase 4 - ENG-36)
//!
//! Provides caching for search results with:
//! - Similarity-based cache lookup (not just exact query match)
//! - Adaptive threshold adjustment based on feedback
//! - TTL-based expiration
//! - Cache invalidation on memory changes

use crate::types::{MemoryType, SearchResult};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Filter parameters that affect cache key generation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CacheFilterParams {
    pub workspace: Option<String>,
    pub tier: Option<String>,
    pub memory_types: Option<Vec<MemoryType>>,
    pub include_archived: bool,
    pub include_transcripts: bool,
    pub tags: Option<Vec<String>>,
}

impl Default for CacheFilterParams {
    fn default() -> Self {
        Self {
            workspace: None,
            tier: None,
            memory_types: None,
            include_archived: false,
            include_transcripts: false,
            tags: None,
        }
    }
}

/// A cached search result entry
#[derive(Debug)]
pub struct CachedSearchResult {
    /// Hash of the original query
    pub query_hash: u64,
    /// The query embedding (for similarity matching)
    pub query_embedding: Option<Vec<f32>>,
    /// Filter parameters used for this search
    pub filter_params: CacheFilterParams,
    /// The cached results
    pub results: Vec<SearchResult>,
    /// When this entry was created
    pub created_at: Instant,
    /// Number of times this cache entry was hit
    pub hit_count: AtomicU64,
    /// Feedback score (positive = good results, negative = bad)
    pub feedback_score: AtomicI64,
}

impl CachedSearchResult {
    pub fn new(
        query_hash: u64,
        query_embedding: Option<Vec<f32>>,
        filter_params: CacheFilterParams,
        results: Vec<SearchResult>,
    ) -> Self {
        Self {
            query_hash,
            query_embedding,
            filter_params,
            results,
            created_at: Instant::now(),
            hit_count: AtomicU64::new(0),
            feedback_score: AtomicI64::new(0),
        }
    }

    /// Check if this entry is expired
    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }

    /// Record a cache hit
    pub fn record_hit(&self) {
        self.hit_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record feedback (positive or negative)
    pub fn record_feedback(&self, positive: bool) {
        if positive {
            self.feedback_score.fetch_add(1, Ordering::Relaxed);
        } else {
            self.feedback_score.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// Configuration for the adaptive cache
#[derive(Debug, Clone)]
pub struct AdaptiveCacheConfig {
    /// Base similarity threshold for cache hits (default: 0.92)
    pub similarity_threshold: f32,
    /// Minimum similarity threshold (floor: 0.85)
    pub min_threshold: f32,
    /// Maximum similarity threshold (ceiling: 0.98)
    pub max_threshold: f32,
    /// Time-to-live for cache entries (default: 5 minutes)
    pub ttl_seconds: u64,
    /// Maximum number of cache entries (default: 1000)
    pub max_entries: usize,
    /// Enable adaptive threshold adjustment
    pub adaptive_enabled: bool,
}

impl Default for AdaptiveCacheConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.92,
            min_threshold: 0.85,
            max_threshold: 0.98,
            ttl_seconds: 300, // 5 minutes
            max_entries: 1000,
            adaptive_enabled: true,
        }
    }
}

/// Search result cache with adaptive thresholds
pub struct SearchResultCache {
    /// Cached entries keyed by cache key (query_hash + filter_hash)
    entries: DashMap<String, Arc<CachedSearchResult>>,
    /// Configuration
    config: AdaptiveCacheConfig,
    /// Current adaptive threshold
    current_threshold: std::sync::atomic::AtomicU32,
    /// Cache statistics
    stats: CacheStats,
}

/// Cache statistics
#[derive(Debug, Default)]
pub struct CacheStats {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub invalidations: AtomicU64,
    pub evictions: AtomicU64,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }
}

/// Cache lookup result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStatsResponse {
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
    pub hit_rate: f64,
    pub invalidations: u64,
    pub evictions: u64,
    pub current_threshold: f32,
    pub ttl_seconds: u64,
}

impl SearchResultCache {
    pub fn new(config: AdaptiveCacheConfig) -> Self {
        let threshold_bits = config.similarity_threshold.to_bits();
        Self {
            entries: DashMap::new(),
            current_threshold: std::sync::atomic::AtomicU32::new(threshold_bits),
            config,
            stats: CacheStats::default(),
        }
    }

    /// Get current similarity threshold
    pub fn current_threshold(&self) -> f32 {
        f32::from_bits(self.current_threshold.load(Ordering::Relaxed))
    }

    /// Generate cache key from query hash and filter params
    fn cache_key(query_hash: u64, filters: &CacheFilterParams) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        query_hash.hash(&mut hasher);
        filters.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Hash a query string
    pub fn hash_query(query: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        query.to_lowercase().trim().hash(&mut hasher);
        hasher.finish()
    }

    /// Calculate cosine similarity between two embeddings
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }

        let mut dot = 0.0f32;
        let mut norm_a = 0.0f32;
        let mut norm_b = 0.0f32;

        for (x, y) in a.iter().zip(b.iter()) {
            dot += x * y;
            norm_a += x * x;
            norm_b += y * y;
        }

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot / (norm_a.sqrt() * norm_b.sqrt())
    }

    /// Try to get cached results for a query
    pub fn get(
        &self,
        query: &str,
        query_embedding: Option<&[f32]>,
        filters: &CacheFilterParams,
    ) -> Option<Vec<SearchResult>> {
        let query_hash = Self::hash_query(query);
        let cache_key = Self::cache_key(query_hash, filters);

        // First try exact match
        if let Some(entry) = self.entries.get(&cache_key) {
            if !entry.is_expired(Duration::from_secs(self.config.ttl_seconds)) {
                entry.record_hit();
                self.stats.hits.fetch_add(1, Ordering::Relaxed);
                return Some(entry.results.clone());
            } else {
                // Remove expired entry
                drop(entry);
                self.entries.remove(&cache_key);
            }
        }

        // Try similarity-based lookup if we have an embedding
        if let Some(embedding) = query_embedding {
            let threshold = self.current_threshold();

            for entry in self.entries.iter() {
                if entry.filter_params != *filters {
                    continue;
                }

                if entry.is_expired(Duration::from_secs(self.config.ttl_seconds)) {
                    continue;
                }

                if let Some(ref cached_embedding) = entry.query_embedding {
                    let similarity = Self::cosine_similarity(embedding, cached_embedding);
                    if similarity >= threshold {
                        entry.record_hit();
                        self.stats.hits.fetch_add(1, Ordering::Relaxed);
                        return Some(entry.results.clone());
                    }
                }
            }
        }

        self.stats.misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Store search results in cache
    pub fn put(
        &self,
        query: &str,
        query_embedding: Option<Vec<f32>>,
        filters: CacheFilterParams,
        results: Vec<SearchResult>,
    ) {
        let query_hash = Self::hash_query(query);
        let cache_key = Self::cache_key(query_hash, &filters);

        // Evict if at capacity
        if self.entries.len() >= self.config.max_entries {
            self.evict_oldest();
        }

        let entry = CachedSearchResult::new(query_hash, query_embedding, filters, results);
        self.entries.insert(cache_key, Arc::new(entry));
    }

    /// Evict the oldest entry
    fn evict_oldest(&self) {
        let mut oldest_key: Option<String> = None;
        let mut oldest_time = Instant::now();

        for entry in self.entries.iter() {
            if entry.created_at < oldest_time {
                oldest_time = entry.created_at;
                oldest_key = Some(entry.key().clone());
            }
        }

        if let Some(key) = oldest_key {
            self.entries.remove(&key);
            self.stats.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Remove expired entries
    pub fn remove_expired(&self) {
        let ttl = Duration::from_secs(self.config.ttl_seconds);
        self.entries.retain(|_, v| !v.is_expired(ttl));
    }

    /// Invalidate cache entries for a specific workspace
    pub fn invalidate_for_workspace(&self, workspace: Option<&str>) {
        self.entries.retain(|_, v| {
            let should_keep = v.filter_params.workspace.as_deref() != workspace;
            if !should_keep {
                self.stats.invalidations.fetch_add(1, Ordering::Relaxed);
            }
            should_keep
        });
    }

    /// Invalidate cache entries that might contain a specific memory
    pub fn invalidate_for_memory(&self, memory_id: i64) {
        // Since we don't track which memories are in which cache entries,
        // we invalidate entries that could potentially contain this memory.
        // For now, we do a simple approach: invalidate all entries older than
        // a certain threshold or just clear all.
        // A more sophisticated approach would track memory IDs in each entry.
        self.entries.retain(|_, v| {
            // Check if any result contains this memory ID
            let contains_memory = v.results.iter().any(|r| r.memory.id == memory_id);
            if contains_memory {
                self.stats.invalidations.fetch_add(1, Ordering::Relaxed);
            }
            !contains_memory
        });
    }

    /// Clear all cache entries
    pub fn clear(&self) {
        let count = self.entries.len();
        self.entries.clear();
        self.stats
            .invalidations
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Record feedback for a query (adjusts adaptive threshold)
    pub fn record_feedback(&self, query: &str, filters: &CacheFilterParams, positive: bool) {
        let query_hash = Self::hash_query(query);
        let cache_key = Self::cache_key(query_hash, filters);

        if let Some(entry) = self.entries.get(&cache_key) {
            entry.record_feedback(positive);
        }

        // Adjust threshold based on feedback
        if self.config.adaptive_enabled {
            self.adjust_threshold(positive);
        }
    }

    /// Adjust the similarity threshold based on feedback
    fn adjust_threshold(&self, positive: bool) {
        let current = self.current_threshold();
        let adjustment = 0.01; // 1% adjustment per feedback

        let new_threshold = if positive {
            // Positive feedback: can be more lenient (lower threshold)
            (current - adjustment).max(self.config.min_threshold)
        } else {
            // Negative feedback: be more strict (higher threshold)
            (current + adjustment).min(self.config.max_threshold)
        };

        self.current_threshold
            .store(new_threshold.to_bits(), Ordering::Relaxed);
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStatsResponse {
        CacheStatsResponse {
            entries: self.entries.len(),
            hits: self.stats.hits.load(Ordering::Relaxed),
            misses: self.stats.misses.load(Ordering::Relaxed),
            hit_rate: self.stats.hit_rate(),
            invalidations: self.stats.invalidations.load(Ordering::Relaxed),
            evictions: self.stats.evictions.load(Ordering::Relaxed),
            current_threshold: self.current_threshold(),
            ttl_seconds: self.config.ttl_seconds,
        }
    }

    /// Start background expiration worker (call from main thread)
    pub fn start_expiration_worker(cache: Arc<Self>, interval_secs: u64) {
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(interval_secs));
            cache.remove_expired();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MemoryType;

    fn make_test_memory(id: i64, content: &str) -> crate::types::Memory {
        crate::types::Memory {
            id,
            content: content.to_string(),
            memory_type: MemoryType::Note,
            importance: 0.5,
            tags: vec![],
            access_count: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_accessed_at: None,
            owner_id: None,
            visibility: Default::default(),
            version: 1,
            has_embedding: false,
            metadata: Default::default(),
            scope: crate::types::MemoryScope::Global,
            workspace: "default".to_string(),
            tier: crate::types::MemoryTier::Permanent,
            expires_at: None,
            content_hash: None,
            event_time: None,
            event_duration_seconds: None,
            trigger_pattern: None,
            procedure_success_count: 0,
            procedure_failure_count: 0,
            summary_of_id: None,
            lifecycle_state: crate::types::LifecycleState::Active,
        }
    }

    fn make_test_result(id: i64, content: &str, score: f32) -> SearchResult {
        SearchResult {
            memory: make_test_memory(id, content),
            score,
            match_info: crate::types::MatchInfo {
                strategy: crate::types::SearchStrategy::Hybrid,
                matched_terms: vec![],
                highlights: vec![],
                semantic_score: None,
                keyword_score: Some(score),
            },
        }
    }

    #[test]
    fn test_cache_put_get() {
        let cache = SearchResultCache::new(AdaptiveCacheConfig::default());
        let results = vec![make_test_result(1, "test content", 0.9)];

        cache.put(
            "test query",
            None,
            CacheFilterParams::default(),
            results.clone(),
        );

        let cached = cache.get("test query", None, &CacheFilterParams::default());
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().len(), 1);
    }

    #[test]
    fn test_cache_miss() {
        let cache = SearchResultCache::new(AdaptiveCacheConfig::default());

        let cached = cache.get("nonexistent", None, &CacheFilterParams::default());
        assert!(cached.is_none());
    }

    #[test]
    fn test_cache_invalidation() {
        let cache = SearchResultCache::new(AdaptiveCacheConfig::default());
        let results = vec![make_test_result(1, "test", 0.9)];

        cache.put("query", None, CacheFilterParams::default(), results);

        // Verify it's cached
        assert!(cache
            .get("query", None, &CacheFilterParams::default())
            .is_some());

        // Invalidate for memory ID 1
        cache.invalidate_for_memory(1);

        // Should be gone
        assert!(cache
            .get("query", None, &CacheFilterParams::default())
            .is_none());
    }

    #[test]
    fn test_different_filters_different_cache() {
        let cache = SearchResultCache::new(AdaptiveCacheConfig::default());
        let results1 = vec![make_test_result(1, "result 1", 0.9)];
        let results2 = vec![make_test_result(2, "result 2", 0.8)];

        let filters1 = CacheFilterParams {
            workspace: Some("ws1".to_string()),
            ..Default::default()
        };
        let filters2 = CacheFilterParams {
            workspace: Some("ws2".to_string()),
            ..Default::default()
        };

        cache.put("query", None, filters1.clone(), results1);
        cache.put("query", None, filters2.clone(), results2);

        let cached1 = cache.get("query", None, &filters1);
        let cached2 = cache.get("query", None, &filters2);

        assert!(cached1.is_some());
        assert!(cached2.is_some());
        assert_eq!(cached1.unwrap()[0].memory.id, 1);
        assert_eq!(cached2.unwrap()[0].memory.id, 2);
    }

    #[test]
    fn test_similarity_lookup() {
        let cache = SearchResultCache::new(AdaptiveCacheConfig {
            similarity_threshold: 0.9,
            ..Default::default()
        });

        let embedding = vec![1.0, 0.0, 0.0];
        let results = vec![make_test_result(1, "test", 0.9)];

        cache.put(
            "original query",
            Some(embedding.clone()),
            CacheFilterParams::default(),
            results,
        );

        // Same embedding should hit
        let cached = cache.get(
            "different query",
            Some(&embedding),
            &CacheFilterParams::default(),
        );
        assert!(cached.is_some());

        // Very similar embedding should hit
        let similar = vec![0.99, 0.1, 0.0];
        let cached = cache.get(
            "another query",
            Some(&similar),
            &CacheFilterParams::default(),
        );
        assert!(cached.is_some());

        // Different embedding should miss
        let different = vec![0.0, 1.0, 0.0];
        let cached = cache.get(
            "yet another",
            Some(&different),
            &CacheFilterParams::default(),
        );
        assert!(cached.is_none());
    }

    #[test]
    fn test_stats() {
        let cache = SearchResultCache::new(AdaptiveCacheConfig::default());
        let results = vec![make_test_result(1, "test", 0.9)];

        // Miss
        cache.get("query", None, &CacheFilterParams::default());

        // Put
        cache.put("query", None, CacheFilterParams::default(), results);

        // Hit
        cache.get("query", None, &CacheFilterParams::default());
        cache.get("query", None, &CacheFilterParams::default());

        let stats = cache.stats();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 2);
        assert!(stats.hit_rate > 0.6);
    }
}
