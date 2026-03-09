//! Online Semantic Synthesis (RML-1209)
//!
//! Detects when newly added memories overlap with recent ones in the session
//! buffer and synthesizes them into a single richer memory, reducing redundancy
//! and token overhead for downstream consumers.
//!
//! # Design
//! - Pure in-memory: no database access, no async I/O.
//! - Session-scoped: the buffer lives only as long as the [`SynthesisEngine`].
//! - Overlap detection uses Jaccard similarity on stopword-filtered tokens.
//! - Three synthesis strategies: [`SynthesisStrategy::Merge`],
//!   [`SynthesisStrategy::Replace`], and [`SynthesisStrategy::Append`].

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// Stopwords
// ---------------------------------------------------------------------------

/// Basic English stopwords filtered out before Jaccard computation.
static STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can", "to",
    "of", "in", "for", "on", "with", "at", "by", "from",
];

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Configuration for the synthesis engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesisConfig {
    /// Jaccard threshold above which two memories are considered overlapping.
    /// Default: 0.4
    pub overlap_threshold: f32,
    /// Number of recent memories kept in the sliding buffer.
    /// Default: 50
    pub buffer_size: usize,
    /// Minimum shared token count required (in addition to threshold).
    /// Default: 5
    pub min_overlap_tokens: usize,
}

impl Default for SynthesisConfig {
    fn default() -> Self {
        Self {
            overlap_threshold: 0.4,
            buffer_size: 50,
            min_overlap_tokens: 5,
        }
    }
}

/// How the engine should combine overlapping memories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SynthesisStrategy {
    /// Combine both into richer content (unique sentences from each).
    Merge,
    /// Keep the newer content, discard the older.
    Replace,
    /// Concatenate with a separator, deduplicating identical lines.
    Append,
}

/// The result of a synthesis operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesizedMemory {
    /// The synthesized content string.
    pub content: String,
    /// IDs of the source memories that were combined.
    pub sources: Vec<i64>,
    /// Jaccard overlap score that triggered synthesis.
    pub overlap_score: f32,
    /// Which strategy was applied.
    pub strategy_used: SynthesisStrategy,
    /// Approximate token reduction vs. keeping both originals separately.
    pub tokens_saved: usize,
}

/// One overlap candidate found in the buffer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapResult {
    /// Memory ID of the overlapping buffer entry.
    pub memory_id: i64,
    /// Jaccard overlap score.
    pub overlap_score: f32,
    /// Tokens shared between the new content and this buffer entry.
    pub shared_tokens: Vec<String>,
}

// ---------------------------------------------------------------------------
// Internal buffer entry
// ---------------------------------------------------------------------------

/// A slot in the recent-memory ring buffer.
pub struct BufferEntry {
    /// Persistent memory ID (assigned by storage layer).
    pub id: i64,
    /// Raw content string.
    pub content: String,
    /// Pre-computed normalised tokens (lowercase, stopwords removed).
    pub tokens: Vec<String>,
}

// ---------------------------------------------------------------------------
// SynthesisEngine
// ---------------------------------------------------------------------------

/// Session-scoped online synthesis engine.
///
/// # Usage
/// ```rust
/// use engram::intelligence::{SynthesisConfig, SynthesisEngine, SynthesisStrategy};
///
/// let mut engine = SynthesisEngine::new(SynthesisConfig::default());
/// engine.add_to_buffer(1, "Rust ownership model uses borrow checker rules");
///
/// if let Some(synth) = engine.check_and_synthesize(
///     "Rust borrow checker enforces ownership rules at compile time",
///     SynthesisStrategy::Merge,
/// ) {
///     println!("Synthesized: {}", synth.content);
/// }
/// ```
pub struct SynthesisEngine {
    config: SynthesisConfig,
    buffer: VecDeque<BufferEntry>,
}

impl SynthesisEngine {
    /// Create a new engine with the given configuration.
    pub fn new(config: SynthesisConfig) -> Self {
        Self {
            buffer: VecDeque::with_capacity(config.buffer_size),
            config,
        }
    }

    // -----------------------------------------------------------------------
    // Tokenisation
    // -----------------------------------------------------------------------

    /// Tokenise `text`: lowercase, split on whitespace and punctuation, strip
    /// leading/trailing non-alphanumeric characters, filter empty tokens and
    /// common English stopwords.
    pub fn tokenize(text: &str) -> Vec<String> {
        let lower = text.to_lowercase();
        lower
            .split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
            .map(|tok| tok.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|tok| !tok.is_empty() && !STOPWORDS.contains(&tok.as_str()))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Similarity
    // -----------------------------------------------------------------------

    /// Jaccard similarity: |intersection| / |union| over token sets.
    ///
    /// Returns 1.0 for two empty slices and 0.0 when the union is empty
    /// (degenerate case handled gracefully).
    pub fn jaccard_similarity(a: &[String], b: &[String]) -> f32 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }

        // Build sets using sorted dedup (avoids HashSet allocation for small slices)
        let mut set_a: Vec<&str> = a.iter().map(String::as_str).collect();
        set_a.sort_unstable();
        set_a.dedup();

        let mut set_b: Vec<&str> = b.iter().map(String::as_str).collect();
        set_b.sort_unstable();
        set_b.dedup();

        let mut intersection = 0usize;
        let (mut i, mut j) = (0, 0);
        while i < set_a.len() && j < set_b.len() {
            match set_a[i].cmp(set_b[j]) {
                std::cmp::Ordering::Equal => {
                    intersection += 1;
                    i += 1;
                    j += 1;
                }
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
            }
        }

        // |union| = |A| + |B| - |intersection|
        let union = set_a.len() + set_b.len() - intersection;
        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }

    // -----------------------------------------------------------------------
    // Overlap detection
    // -----------------------------------------------------------------------

    /// Check `new_content` against every entry in the buffer.
    ///
    /// Returns all entries whose Jaccard score meets both the
    /// `overlap_threshold` and `min_overlap_tokens` criteria, sorted by score
    /// descending.
    pub fn detect_overlap(&self, new_content: &str) -> Vec<OverlapResult> {
        let new_tokens = Self::tokenize(new_content);

        let mut results: Vec<OverlapResult> = self
            .buffer
            .iter()
            .filter_map(|entry| {
                let score = Self::jaccard_similarity(&new_tokens, &entry.tokens);
                if score < self.config.overlap_threshold {
                    return None;
                }

                // Collect shared tokens (intersection)
                let mut new_sorted: Vec<&str> = new_tokens.iter().map(String::as_str).collect();
                new_sorted.sort_unstable();
                new_sorted.dedup();

                let mut buf_sorted: Vec<&str> = entry.tokens.iter().map(String::as_str).collect();
                buf_sorted.sort_unstable();
                buf_sorted.dedup();

                let shared: Vec<String> = {
                    let (mut i, mut j) = (0, 0);
                    let mut shared = Vec::new();
                    while i < new_sorted.len() && j < buf_sorted.len() {
                        match new_sorted[i].cmp(buf_sorted[j]) {
                            std::cmp::Ordering::Equal => {
                                shared.push(new_sorted[i].to_string());
                                i += 1;
                                j += 1;
                            }
                            std::cmp::Ordering::Less => i += 1,
                            std::cmp::Ordering::Greater => j += 1,
                        }
                    }
                    shared
                };

                if shared.len() < self.config.min_overlap_tokens {
                    return None;
                }

                Some(OverlapResult {
                    memory_id: entry.id,
                    overlap_score: score,
                    shared_tokens: shared,
                })
            })
            .collect();

        results.sort_by(|a, b| {
            b.overlap_score
                .partial_cmp(&a.overlap_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results
    }

    // -----------------------------------------------------------------------
    // Synthesis
    // -----------------------------------------------------------------------

    /// Synthesise a new memory from `existing_content` (id `existing_id`) and
    /// `new_content` using the chosen `strategy`.
    ///
    /// The `overlap_score` is embedded in the result for caller transparency.
    pub fn synthesize(
        &self,
        existing_content: &str,
        existing_id: i64,
        new_content: &str,
        strategy: SynthesisStrategy,
    ) -> SynthesizedMemory {
        let existing_tokens = Self::tokenize(existing_content);
        let new_tokens = Self::tokenize(new_content);
        let overlap_score = Self::jaccard_similarity(&existing_tokens, &new_tokens);

        let combined_raw_len = existing_content.len() + new_content.len();

        let content = match strategy {
            SynthesisStrategy::Merge => Self::merge_content(existing_content, new_content),
            SynthesisStrategy::Replace => new_content.to_string(),
            SynthesisStrategy::Append => Self::append_content(existing_content, new_content),
        };

        let tokens_saved = combined_raw_len.saturating_sub(content.len());

        SynthesizedMemory {
            content,
            sources: vec![existing_id],
            overlap_score,
            strategy_used: strategy,
            tokens_saved,
        }
    }

    /// Merge strategy: interleave unique sentences from both inputs.
    ///
    /// Sentences are split on `.`, `!`, `?` and then deduplicated (normalised
    /// lowercase comparison). Existing sentences appear first; unique new
    /// sentences are appended.
    fn merge_content(existing: &str, new: &str) -> String {
        let existing_sentences = Self::split_sentences(existing);
        let new_sentences = Self::split_sentences(new);

        let existing_norm: Vec<String> = existing_sentences
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        let mut merged: Vec<&str> = existing_sentences.iter().map(String::as_str).collect();

        for (raw, norm) in new_sentences.iter().zip(
            new_sentences
                .iter()
                .map(|s| s.to_lowercase())
                .collect::<Vec<_>>()
                .iter(),
        ) {
            if !existing_norm.contains(norm) {
                merged.push(raw.as_str());
            }
        }

        merged.join(" ").trim().to_string()
    }

    /// Append strategy: concatenate with `\n---\n` separator and deduplicate
    /// identical lines (case-insensitive).
    fn append_content(existing: &str, new: &str) -> String {
        let existing_lines: Vec<&str> = existing.lines().collect();
        let existing_norm: Vec<String> = existing_lines
            .iter()
            .map(|l| l.trim().to_lowercase())
            .collect();

        let mut result_lines: Vec<&str> = existing_lines;

        // Add separator only if existing is non-empty
        let separator_added = !existing.trim().is_empty();

        let mut new_lines: Vec<&str> = Vec::new();
        for line in new.lines() {
            let norm = line.trim().to_lowercase();
            if !norm.is_empty() && !existing_norm.contains(&norm) {
                new_lines.push(line);
            }
        }

        if !new_lines.is_empty() {
            if separator_added {
                result_lines.push("---");
            }
            result_lines.extend_from_slice(&new_lines);
        }

        result_lines.join("\n")
    }

    /// Split text into non-empty trimmed sentences.
    fn split_sentences(text: &str) -> Vec<String> {
        text.split(['.', '!', '?'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    // -----------------------------------------------------------------------
    // Buffer management
    // -----------------------------------------------------------------------

    /// Add a memory to the sliding buffer, evicting the oldest entry when the
    /// buffer is at capacity.
    pub fn add_to_buffer(&mut self, id: i64, content: &str) {
        if self.buffer.len() >= self.config.buffer_size {
            self.buffer.pop_front();
        }
        let tokens = Self::tokenize(content);
        self.buffer.push_back(BufferEntry {
            id,
            content: content.to_string(),
            tokens,
        });
    }

    /// Current number of entries in the buffer.
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    // -----------------------------------------------------------------------
    // Convenience
    // -----------------------------------------------------------------------

    /// Convenience method: detect overlap and, if found, synthesise using the
    /// best-scoring buffer entry.
    ///
    /// Returns `None` when no buffer entry meets the overlap criteria.
    pub fn check_and_synthesize(
        &self,
        new_content: &str,
        strategy: SynthesisStrategy,
    ) -> Option<SynthesizedMemory> {
        let overlaps = self.detect_overlap(new_content);
        let best = overlaps.first()?;

        // Find the buffer entry for the best overlap
        let entry = self.buffer.iter().find(|e| e.id == best.memory_id)?;

        Some(self.synthesize(&entry.content, entry.id, new_content, strategy))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // 1. Jaccard similarity computation
    #[test]
    fn test_jaccard_similarity_basic() {
        // Identical token sets => 1.0
        let a = vec!["rust".to_string(), "ownership".to_string()];
        let b = vec!["rust".to_string(), "ownership".to_string()];
        let sim = SynthesisEngine::jaccard_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6, "identical sets should give 1.0");

        // Disjoint => 0.0
        let c = vec!["apple".to_string(), "banana".to_string()];
        let d = vec!["car".to_string(), "truck".to_string()];
        let sim2 = SynthesisEngine::jaccard_similarity(&c, &d);
        assert!((sim2 - 0.0).abs() < 1e-6, "disjoint sets should give 0.0");

        // Partial overlap: {rust, borrow} vs {rust, ownership} => 1/3
        let e = vec!["rust".to_string(), "borrow".to_string()];
        let f = vec!["rust".to_string(), "ownership".to_string()];
        let sim3 = SynthesisEngine::jaccard_similarity(&e, &f);
        let expected = 1.0f32 / 3.0;
        assert!(
            (sim3 - expected).abs() < 1e-5,
            "partial overlap sim={sim3} expected≈{expected}"
        );
    }

    // 2. Detect overlap above threshold
    #[test]
    fn test_detect_overlap_above_threshold() {
        let mut engine = SynthesisEngine::new(SynthesisConfig {
            overlap_threshold: 0.3,
            buffer_size: 10,
            min_overlap_tokens: 2,
        });
        engine.add_to_buffer(
            1,
            "Rust ownership model uses borrow checker rules compile time safety",
        );

        let results =
            engine.detect_overlap("Rust borrow checker enforces ownership rules compile time");
        assert!(
            !results.is_empty(),
            "should find overlap for highly similar content"
        );
        assert_eq!(results[0].memory_id, 1);
        assert!(results[0].overlap_score >= 0.3);
    }

    // 3. No overlap below threshold
    #[test]
    fn test_no_overlap_below_threshold() {
        let mut engine = SynthesisEngine::new(SynthesisConfig {
            overlap_threshold: 0.6,
            buffer_size: 10,
            min_overlap_tokens: 2,
        });
        engine.add_to_buffer(1, "SQLite WAL mode improves write concurrency");

        let results = engine.detect_overlap("Python async await event loop");
        assert!(
            results.is_empty(),
            "unrelated content should not trigger overlap"
        );
    }

    // 4. Merge strategy combines content
    #[test]
    fn test_merge_strategy_combines_content() {
        let engine = SynthesisEngine::new(SynthesisConfig::default());
        let existing = "Rust uses a borrow checker. Safety is guaranteed at compile time.";
        let new = "Rust uses a borrow checker. Memory leaks are prevented automatically.";

        let result = engine.synthesize(existing, 1, new, SynthesisStrategy::Merge);

        assert_eq!(result.strategy_used, SynthesisStrategy::Merge);
        // Should contain unique sentence from the new input
        assert!(
            result
                .content
                .contains("Memory leaks are prevented automatically"),
            "merged content should include the unique new sentence"
        );
        // Should not duplicate the shared sentence
        let count = result.content.matches("Rust uses a borrow checker").count();
        assert_eq!(count, 1, "shared sentence should appear only once");
        assert_eq!(result.sources, vec![1]);
    }

    // 5. Replace strategy keeps newer content
    #[test]
    fn test_replace_strategy_keeps_newer() {
        let engine = SynthesisEngine::new(SynthesisConfig::default());
        let existing = "Old description of Rust ownership.";
        let new = "Updated and more detailed description of Rust ownership.";

        let result = engine.synthesize(existing, 2, new, SynthesisStrategy::Replace);

        assert_eq!(result.strategy_used, SynthesisStrategy::Replace);
        assert_eq!(result.content, new);
        assert_eq!(result.sources, vec![2]);
    }

    // 6. Append strategy concatenates with separator and deduplicates
    #[test]
    fn test_append_strategy_concatenates() {
        let engine = SynthesisEngine::new(SynthesisConfig::default());
        let existing = "Line one\nLine two";
        let new = "Line two\nLine three";

        let result = engine.synthesize(existing, 3, new, SynthesisStrategy::Append);

        assert_eq!(result.strategy_used, SynthesisStrategy::Append);
        // Deduplication: "Line two" should appear only once
        let count = result.content.matches("Line two").count();
        assert_eq!(count, 1, "duplicate lines should be removed");
        // New unique line should be present
        assert!(
            result.content.contains("Line three"),
            "unique new line should be present"
        );
        // Separator should be present
        assert!(
            result.content.contains("---"),
            "separator should be present"
        );
    }

    // 7. Buffer eviction when full
    #[test]
    fn test_buffer_eviction_when_full() {
        let config = SynthesisConfig {
            overlap_threshold: 0.4,
            buffer_size: 3,
            min_overlap_tokens: 1,
        };
        let mut engine = SynthesisEngine::new(config);

        engine.add_to_buffer(1, "memory one");
        engine.add_to_buffer(2, "memory two");
        engine.add_to_buffer(3, "memory three");
        assert_eq!(engine.buffer_len(), 3);

        // Adding a 4th entry should evict the oldest (id=1)
        engine.add_to_buffer(4, "memory four");
        assert_eq!(engine.buffer_len(), 3, "buffer should not exceed capacity");

        // Entry id=1 should no longer be in the buffer
        let ids: Vec<i64> = engine.buffer.iter().map(|e| e.id).collect();
        assert!(
            !ids.contains(&1),
            "oldest entry should have been evicted, got: {ids:?}"
        );
        assert!(ids.contains(&4), "newest entry should be present");
    }

    // 8. Empty / very short content handled gracefully
    #[test]
    fn test_empty_content_handled() {
        let mut engine = SynthesisEngine::new(SynthesisConfig::default());

        // Tokenising empty string should return empty vec
        let tokens = SynthesisEngine::tokenize("");
        assert!(tokens.is_empty());

        // Two empty token sets => Jaccard 1.0
        let sim = SynthesisEngine::jaccard_similarity(&[], &[]);
        assert!((sim - 1.0).abs() < 1e-6);

        // Adding empty content to buffer should not panic
        engine.add_to_buffer(99, "");
        assert_eq!(engine.buffer_len(), 1);

        // Detecting overlap on empty new content should return empty results
        // (no entry will meet min_overlap_tokens=5 with zero tokens)
        let results = engine.detect_overlap("");
        assert!(results.is_empty(), "empty new content yields no overlaps");
    }

    // 9. check_and_synthesize returns None when buffer is empty
    #[test]
    fn test_check_and_synthesize_empty_buffer() {
        let engine = SynthesisEngine::new(SynthesisConfig::default());
        let result = engine.check_and_synthesize("some new content here", SynthesisStrategy::Merge);
        assert!(result.is_none(), "empty buffer should return None");
    }

    // 10. check_and_synthesize returns Some when overlap is found
    #[test]
    fn test_check_and_synthesize_returns_some() {
        let mut engine = SynthesisEngine::new(SynthesisConfig {
            overlap_threshold: 0.3,
            buffer_size: 10,
            min_overlap_tokens: 2,
        });
        engine.add_to_buffer(42, "async rust tokio runtime executor futures polling");

        let result = engine.check_and_synthesize(
            "tokio runtime executor drives async futures rust",
            SynthesisStrategy::Replace,
        );
        assert!(
            result.is_some(),
            "should find overlap and return synthesized memory"
        );
        let synth = result.unwrap();
        assert_eq!(synth.sources, vec![42]);
    }

    // 11. tokens_saved reflects character reduction
    #[test]
    fn test_tokens_saved_replace_strategy() {
        let engine = SynthesisEngine::new(SynthesisConfig::default());
        let existing = "A".repeat(100);
        let new = "B".repeat(20);

        // Replace keeps only new (20 chars). combined_raw = 120. saved = 120-20 = 100
        let result = engine.synthesize(&existing, 5, &new, SynthesisStrategy::Replace);
        assert_eq!(result.tokens_saved, 100);
    }
}
