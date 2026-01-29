//! Embedding cache with zero-copy sharing via Arc<[f32]>
//!
//! This cache provides efficient storage and retrieval of embeddings with:
//! - LRU eviction policy
//! - Bytes-based capacity (not entry count)
//! - Zero-copy sharing via Arc<[f32]>
//! - Thread-safe access with atomic hit/miss counters
//!
//! Based on Fix 10 from the design plan:
//! > Use Arc<[f32]> for zero-copy sharing instead of cloning vectors

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Statistics for the embedding cache
#[derive(Debug, Clone)]
pub struct EmbeddingCacheStats {
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Current number of entries in cache
    pub entries: usize,
    /// Current bytes used by embeddings
    pub bytes_used: usize,
    /// Maximum bytes capacity
    pub max_bytes: usize,
    /// Hit rate as percentage (0.0 - 100.0)
    pub hit_rate: f64,
}

/// LRU node for tracking access order
struct LruNode {
    /// The embedding data (shared via Arc)
    embedding: Arc<[f32]>,
    /// Size in bytes
    size_bytes: usize,
    /// Previous key in LRU order (more recently used)
    prev: Option<String>,
    /// Next key in LRU order (less recently used)
    next: Option<String>,
}

/// Internal cache state protected by mutex
struct CacheState {
    /// Key -> LRU node mapping
    entries: HashMap<String, LruNode>,
    /// Most recently used key
    head: Option<String>,
    /// Least recently used key
    tail: Option<String>,
    /// Current bytes used
    bytes_used: usize,
}

impl CacheState {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            head: None,
            tail: None,
            bytes_used: 0,
        }
    }

    /// Move a key to the front (most recently used)
    fn move_to_front(&mut self, key: &str) {
        if self.head.as_deref() == Some(key) {
            return; // Already at front
        }

        // Remove from current position
        if let Some(node) = self.entries.get(key) {
            let prev = node.prev.clone();
            let next = node.next.clone();

            // Update neighbors
            if let Some(ref prev_key) = prev {
                if let Some(prev_node) = self.entries.get_mut(prev_key) {
                    prev_node.next = next.clone();
                }
            }
            if let Some(ref next_key) = next {
                if let Some(next_node) = self.entries.get_mut(next_key) {
                    next_node.prev = prev.clone();
                }
            }

            // Update tail if needed
            if self.tail.as_deref() == Some(key) {
                self.tail = prev;
            }
        }

        // Insert at front
        if let Some(node) = self.entries.get_mut(key) {
            node.prev = None;
            node.next = self.head.clone();
        }

        if let Some(ref old_head) = self.head {
            if let Some(head_node) = self.entries.get_mut(old_head) {
                head_node.prev = Some(key.to_string());
            }
        }

        self.head = Some(key.to_string());

        if self.tail.is_none() {
            self.tail = self.head.clone();
        }
    }

    /// Remove the least recently used entry and return its size
    fn evict_lru(&mut self) -> Option<usize> {
        let tail_key = self.tail.take()?;

        if let Some(node) = self.entries.remove(&tail_key) {
            // Update new tail
            self.tail = node.prev.clone();
            if let Some(ref new_tail_key) = self.tail {
                if let Some(new_tail) = self.entries.get_mut(new_tail_key) {
                    new_tail.next = None;
                }
            }

            // Clear head if this was the only entry
            if self.head.as_deref() == Some(&tail_key) {
                self.head = None;
            }

            self.bytes_used -= node.size_bytes;
            return Some(node.size_bytes);
        }

        None
    }
}

/// Thread-safe LRU embedding cache with bytes-based capacity
pub struct EmbeddingCache {
    /// Cache state protected by mutex
    state: Mutex<CacheState>,
    /// Maximum capacity in bytes
    max_bytes: usize,
    /// Atomic hit counter
    hits: AtomicU64,
    /// Atomic miss counter
    misses: AtomicU64,
}

impl EmbeddingCache {
    /// Create a new cache with the specified byte capacity
    ///
    /// # Arguments
    /// - `max_bytes`: Maximum bytes to use for embeddings
    ///   - Default recommendation: 100MB (~25K embeddings @ 1536 dims)
    ///   - Each 1536-dim embedding uses 6144 bytes (1536 * 4)
    pub fn new(max_bytes: usize) -> Self {
        Self {
            state: Mutex::new(CacheState::new()),
            max_bytes,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// Create a cache with default capacity (100MB)
    pub fn default_capacity() -> Self {
        Self::new(100 * 1024 * 1024) // 100MB
    }

    /// Get an embedding from the cache
    ///
    /// Returns Arc clone (cheap pointer copy, not vector copy)
    pub fn get(&self, key: &str) -> Option<Arc<[f32]>> {
        let mut state = self.state.lock().unwrap();

        if state.entries.contains_key(key) {
            state.move_to_front(key);
            self.hits.fetch_add(1, Ordering::Relaxed);
            state.entries.get(key).map(|n| n.embedding.clone())
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// Insert an embedding into the cache
    ///
    /// If the key already exists, the embedding is updated and moved to front.
    /// If capacity is exceeded, least recently used entries are evicted.
    pub fn put(&self, key: String, embedding: Vec<f32>) {
        let size_bytes = embedding.len() * std::mem::size_of::<f32>();

        // Don't cache if single entry exceeds capacity
        if size_bytes > self.max_bytes {
            return;
        }

        let arc: Arc<[f32]> = embedding.into();
        let mut state = self.state.lock().unwrap();

        // Remove existing entry if present
        if let Some(old_node) = state.entries.remove(&key) {
            state.bytes_used -= old_node.size_bytes;

            // Update LRU links for removed node
            if let Some(ref prev_key) = old_node.prev {
                if let Some(prev_node) = state.entries.get_mut(prev_key) {
                    prev_node.next = old_node.next.clone();
                }
            }
            if let Some(ref next_key) = old_node.next {
                if let Some(next_node) = state.entries.get_mut(next_key) {
                    next_node.prev = old_node.prev.clone();
                }
            }
            if state.head.as_deref() == Some(&key) {
                state.head = old_node.next.clone();
            }
            if state.tail.as_deref() == Some(&key) {
                state.tail = old_node.prev.clone();
            }
        }

        // Evict until we have room
        while state.bytes_used + size_bytes > self.max_bytes {
            if state.evict_lru().is_none() {
                break;
            }
        }

        // Insert new entry at front
        let old_head = state.head.clone();
        let node = LruNode {
            embedding: arc,
            size_bytes,
            prev: None,
            next: old_head.clone(),
        };

        // Update old head's prev pointer
        if let Some(ref old_head_key) = old_head {
            if let Some(head_node) = state.entries.get_mut(old_head_key) {
                head_node.prev = Some(key.clone());
            }
        }

        state.entries.insert(key.clone(), node);
        state.bytes_used += size_bytes;
        state.head = Some(key);

        if state.tail.is_none() {
            state.tail = state.head.clone();
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> EmbeddingCacheStats {
        let state = self.state.lock().unwrap();
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;

        EmbeddingCacheStats {
            hits,
            misses,
            entries: state.entries.len(),
            bytes_used: state.bytes_used,
            max_bytes: self.max_bytes,
            hit_rate: if total > 0 {
                (hits as f64 / total as f64) * 100.0
            } else {
                0.0
            },
        }
    }

    /// Clear all entries from the cache
    pub fn clear(&self) {
        let mut state = self.state.lock().unwrap();
        state.entries.clear();
        state.head = None;
        state.tail = None;
        state.bytes_used = 0;
        // Note: We don't reset hit/miss counters - they're cumulative stats
    }

    /// Get the number of entries in the cache
    pub fn len(&self) -> usize {
        self.state.lock().unwrap().entries.len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for EmbeddingCache {
    fn default() -> Self {
        Self::default_capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let cache = EmbeddingCache::new(1024 * 1024); // 1MB

        // Insert and retrieve
        let embedding = vec![1.0, 2.0, 3.0];
        cache.put("test-key".to_string(), embedding.clone());

        let retrieved = cache.get("test-key").unwrap();
        assert_eq!(&*retrieved, &[1.0, 2.0, 3.0]);

        // Miss
        assert!(cache.get("nonexistent").is_none());

        // Stats
        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.entries, 1);
    }

    #[test]
    fn test_lru_eviction() {
        // Small cache: 48 bytes = room for 4 f32s (16 bytes) * 3 entries max
        let cache = EmbeddingCache::new(48);

        // Insert 3 entries (each 16 bytes = 4 * 4)
        cache.put("a".to_string(), vec![1.0, 2.0, 3.0, 4.0]);
        cache.put("b".to_string(), vec![5.0, 6.0, 7.0, 8.0]);
        cache.put("c".to_string(), vec![9.0, 10.0, 11.0, 12.0]);

        assert_eq!(cache.len(), 3);

        // Insert 4th entry, should evict "a" (LRU)
        cache.put("d".to_string(), vec![13.0, 14.0, 15.0, 16.0]);

        assert_eq!(cache.len(), 3);
        assert!(cache.get("a").is_none()); // Evicted
        assert!(cache.get("b").is_some());
        assert!(cache.get("c").is_some());
        assert!(cache.get("d").is_some());
    }

    #[test]
    fn test_access_updates_lru() {
        // Room for 2 entries only
        let cache = EmbeddingCache::new(32);

        cache.put("a".to_string(), vec![1.0, 2.0, 3.0, 4.0]);
        cache.put("b".to_string(), vec![5.0, 6.0, 7.0, 8.0]);

        // Access "a" to make it recently used
        let _ = cache.get("a");

        // Insert "c", should evict "b" (now LRU) instead of "a"
        cache.put("c".to_string(), vec![9.0, 10.0, 11.0, 12.0]);

        assert!(cache.get("a").is_some()); // Still present
        assert!(cache.get("b").is_none()); // Evicted
        assert!(cache.get("c").is_some());
    }

    #[test]
    fn test_clear() {
        let cache = EmbeddingCache::new(1024 * 1024);

        cache.put("a".to_string(), vec![1.0, 2.0, 3.0]);
        cache.put("b".to_string(), vec![4.0, 5.0, 6.0]);

        assert_eq!(cache.len(), 2);

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert!(cache.get("a").is_none());
        assert!(cache.get("b").is_none());

        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.bytes_used, 0);
    }

    #[test]
    fn test_update_existing() {
        let cache = EmbeddingCache::new(1024 * 1024);

        cache.put("key".to_string(), vec![1.0, 2.0, 3.0]);
        let v1 = cache.get("key").unwrap();
        assert_eq!(&*v1, &[1.0, 2.0, 3.0]);

        // Update with new value
        cache.put("key".to_string(), vec![4.0, 5.0, 6.0, 7.0]);
        let v2 = cache.get("key").unwrap();
        assert_eq!(&*v2, &[4.0, 5.0, 6.0, 7.0]);

        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_zero_copy() {
        let cache = EmbeddingCache::new(1024 * 1024);

        cache.put("key".to_string(), vec![1.0, 2.0, 3.0]);

        // Get multiple references - should be Arc clones (cheap)
        let ref1 = cache.get("key").unwrap();
        let ref2 = cache.get("key").unwrap();

        // Both point to same data
        assert!(Arc::ptr_eq(&ref1, &ref2));
    }

    #[test]
    fn test_stats_tracking() {
        let cache = EmbeddingCache::new(1024 * 1024);

        // Initial stats
        let stats = cache.stats();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.hit_rate, 0.0);

        cache.put("a".to_string(), vec![1.0, 2.0]);

        // Hit
        cache.get("a");
        // Miss
        cache.get("nonexistent");
        // Hit
        cache.get("a");

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate - 66.666).abs() < 1.0);
    }
}
