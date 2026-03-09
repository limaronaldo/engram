//! Active Context Compression (RML-1211)
//!
//! Provides adaptive, multi-level compression of memory content to fit within
//! LLM context windows. Pure computation — no database access.
//!
//! Compression levels:
//! - `None`   — full content, no changes
//! - `Light`  — remove stopwords and filler phrases
//! - `Medium` — extractive summary (first sentence + entity sentences)
//! - `Heavy`  — key facts only (entities, numbers, dates)

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Common English stopwords (~30 words)
// ---------------------------------------------------------------------------

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for",
    "of", "with", "by", "from", "is", "are", "was", "were", "be", "been",
    "being", "have", "has", "had", "do", "does", "did", "will", "would",
];

// Common filler phrases removed during light compression
const FILLER_PHRASES: &[&str] = &[
    "basically",
    "essentially",
    "in fact",
    "as a matter of fact",
    "it is worth noting that",
    "it should be noted that",
    "needless to say",
    "to be honest",
    "honestly",
    "actually",
    "literally",
    "obviously",
    "clearly",
    "simply",
    "just",
    "very",
    "really",
    "quite",
    "rather",
    "somewhat",
    "kind of",
    "sort of",
];

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Level of compression to apply to memory content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionLevel {
    /// Full content, no changes.
    None,
    /// Remove stopwords and common filler phrases.
    Light,
    /// Extractive summary: keep first sentence of each paragraph and any
    /// sentence that contains a capitalized (entity-like) word.
    Medium,
    /// Key facts only: "Entity: fact" patterns, numbers, and dates.
    Heavy,
}

/// A memory entry after compression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedEntry {
    /// Original memory ID.
    pub original_id: i64,
    /// Estimated token count of the original content.
    pub original_tokens: usize,
    /// The (potentially compressed) content.
    pub compressed_content: String,
    /// Estimated token count of the compressed content.
    pub tokens_used: usize,
    /// Which compression level was applied.
    pub compression_level: CompressionLevel,
}

/// A snapshot of token budget state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    /// Maximum tokens allowed.
    pub total: usize,
    /// Tokens consumed so far.
    pub used: usize,
    /// Remaining capacity (`total - used`).
    pub remaining: usize,
}

/// Simple memory representation used as compression input.
#[derive(Debug, Clone)]
pub struct MemoryInput {
    /// Unique identifier for the memory.
    pub id: i64,
    /// Raw text content.
    pub content: String,
    /// Importance score in `[0.0, 1.0]`; higher = more important.
    pub importance: f32,
}

// ---------------------------------------------------------------------------
// ContextCompressor
// ---------------------------------------------------------------------------

/// Adaptively compresses a set of memories to fit within a token budget.
pub struct ContextCompressor {
    budget_tokens: usize,
    used_tokens: usize,
}

impl ContextCompressor {
    /// Create a new compressor with the given token budget.
    pub fn new(budget_tokens: usize) -> Self {
        Self {
            budget_tokens,
            used_tokens: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Token estimation
    // -----------------------------------------------------------------------

    /// Estimate the token count for `text` using the heuristic `chars / 4`.
    pub fn estimate_tokens(text: &str) -> usize {
        text.len().div_ceil(4)
    }

    // -----------------------------------------------------------------------
    // Compression implementations
    // -----------------------------------------------------------------------

    /// Light compression: remove filler phrases (case-insensitive), then
    /// drop standalone stopwords and collapse extra whitespace.
    pub fn compress_light(text: &str) -> String {
        let mut result = text.to_string();

        // Remove filler phrases first (case-insensitive, whole-phrase match)
        for phrase in FILLER_PHRASES {
            let lower = result.to_lowercase();
            // Find and remove all occurrences (greedy from the right to avoid
            // index invalidation when the string shrinks).
            let mut positions: Vec<usize> = lower.match_indices(phrase).map(|(i, _)| i).collect();
            positions.sort_unstable_by(|a, b| b.cmp(a)); // reverse order
            for pos in positions {
                // Only remove if the match is surrounded by non-alphabetic chars
                // to avoid partial-word removal.
                let before_ok = pos == 0
                    || !result
                        .as_bytes()
                        .get(pos - 1)
                        .copied()
                        .map(|b| b.is_ascii_alphabetic())
                        .unwrap_or(false);
                let after_pos = pos + phrase.len();
                let after_ok = after_pos >= result.len()
                    || !result
                        .as_bytes()
                        .get(after_pos)
                        .copied()
                        .map(|b| b.is_ascii_alphabetic())
                        .unwrap_or(false);

                if before_ok && after_ok {
                    result.drain(pos..after_pos);
                }
            }
        }

        // Drop standalone stopwords (whole-word, case-insensitive)
        let words: Vec<&str> = result.split_whitespace().collect();
        let filtered: Vec<&str> = words
            .into_iter()
            .filter(|w| {
                let lower = w.to_lowercase();
                let stripped = lower.trim_matches(|c: char| !c.is_alphabetic());
                !STOPWORDS.contains(&stripped)
            })
            .collect();

        // Rejoin and collapse whitespace
        filtered.join(" ")
    }

    /// Medium compression: keep the first sentence of each paragraph and any
    /// sentence that contains a word starting with a capital letter (entity
    /// heuristic).
    pub fn compress_medium(text: &str) -> String {
        let paragraphs: Vec<&str> = text.split("\n\n").collect();
        let mut kept: Vec<String> = Vec::new();

        for paragraph in &paragraphs {
            let sentences = split_sentences(paragraph);
            let mut para_kept: Vec<&str> = Vec::new();

            for (idx, sentence) in sentences.iter().enumerate() {
                let trimmed = sentence.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Always keep first sentence of paragraph
                if idx == 0 {
                    para_kept.push(trimmed);
                    continue;
                }

                // Keep sentence if it contains an entity-like capitalized word
                // (a word that is not the first word and starts with uppercase)
                if has_entity_word(trimmed) {
                    para_kept.push(trimmed);
                }
            }

            if !para_kept.is_empty() {
                kept.push(para_kept.join(" "));
            }
        }

        kept.join("\n\n")
    }

    /// Heavy compression: extract lines that look like key facts —
    /// "Entity: something", lines containing numbers, or dates.
    pub fn compress_heavy(text: &str) -> String {
        let mut facts: Vec<String> = Vec::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Pattern 1: "Word: ..." — colon-delimited key-value fact
            if looks_like_fact_line(trimmed) {
                facts.push(trimmed.to_string());
                continue;
            }

            // Pattern 2: contains a number
            if trimmed.chars().any(|c| c.is_ascii_digit()) {
                facts.push(trimmed.to_string());
                continue;
            }

            // Pattern 3: contains a date-like substring (YYYY or Month-name)
            if contains_date(trimmed) {
                facts.push(trimmed.to_string());
            }
        }

        if facts.is_empty() {
            // Fallback: first sentence only
            let first = split_sentences(text).into_iter().next().unwrap_or_default();
            first.trim().to_string()
        } else {
            facts.join("\n")
        }
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Apply the specified compression level to `content`.
    pub fn compress_single(content: &str, level: CompressionLevel) -> String {
        match level {
            CompressionLevel::None => content.to_string(),
            CompressionLevel::Light => Self::compress_light(content),
            CompressionLevel::Medium => Self::compress_medium(content),
            CompressionLevel::Heavy => Self::compress_heavy(content),
        }
    }

    /// Adaptively compress a slice of memories to fit within `budget` tokens.
    ///
    /// Algorithm:
    /// 1. Sort memories by importance (descending) — most important get
    ///    processed first and receive lighter compression.
    /// 2. For each memory, try compression levels in order:
    ///    `None → Light → Medium → Heavy`.
    /// 3. The first level that fits the remaining budget is used.
    /// 4. If even `Heavy` doesn't fit, the memory is skipped.
    ///
    /// Returns the ordered list of successfully compressed entries.
    pub fn compress_for_context(memories: &[MemoryInput], budget: usize) -> Vec<CompressedEntry> {
        // Sort by importance descending (clone indices to preserve original IDs)
        let mut indexed: Vec<usize> = (0..memories.len()).collect();
        indexed.sort_unstable_by(|&a, &b| {
            memories[b]
                .importance
                .partial_cmp(&memories[a].importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut entries: Vec<CompressedEntry> = Vec::new();
        let mut used: usize = 0;

        for idx in indexed {
            let mem = &memories[idx];
            let original_tokens = Self::estimate_tokens(&mem.content);

            // Try each compression level in escalating order
            let levels = [
                CompressionLevel::None,
                CompressionLevel::Light,
                CompressionLevel::Medium,
                CompressionLevel::Heavy,
            ];

            let mut chosen: Option<(CompressionLevel, String, usize)> = None;

            for &level in &levels {
                let compressed = Self::compress_single(&mem.content, level);
                let tokens = Self::estimate_tokens(&compressed);

                if used + tokens <= budget {
                    chosen = Some((level, compressed, tokens));
                    break;
                }
            }

            if let Some((level, compressed_content, tokens)) = chosen {
                used += tokens;
                entries.push(CompressedEntry {
                    original_id: mem.id,
                    original_tokens,
                    compressed_content,
                    tokens_used: tokens,
                    compression_level: level,
                });
            }
            // else: memory skipped — does not fit even at Heavy
        }

        entries
    }

    /// Current budget state of this compressor instance.
    ///
    /// Note: `compress_for_context` is a free function that does not mutate
    /// the compressor. Call this after manually tracking usage with
    /// `estimate_tokens`, or use it to inspect the configured budget.
    pub fn budget(&self) -> TokenBudget {
        let used = self.used_tokens;
        let remaining = self.budget_tokens.saturating_sub(used);
        TokenBudget {
            total: self.budget_tokens,
            used,
            remaining,
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Split text into sentences on `.`, `!`, or `?` boundaries.
fn split_sentences(text: &str) -> Vec<&str> {
    let mut sentences: Vec<&str> = Vec::new();
    let mut start = 0;

    let bytes = text.as_bytes();
    let len = bytes.len();

    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if b == b'.' || b == b'!' || b == b'?' {
            // Include the punctuation
            let end = (i + 1).min(len);
            let s = text[start..end].trim();
            if !s.is_empty() {
                sentences.push(s);
            }
            // Skip whitespace after punctuation
            i += 1;
            while i < len && bytes[i] == b' ' {
                i += 1;
            }
            start = i;
        } else {
            i += 1;
        }
    }

    // Trailing text without terminal punctuation
    let tail = text[start..].trim();
    if !tail.is_empty() {
        sentences.push(tail);
    }

    sentences
}

/// Return `true` if the sentence contains a word (not the first word) that
/// starts with an uppercase ASCII letter — a rough entity heuristic.
fn has_entity_word(sentence: &str) -> bool {
    sentence
        .split_whitespace()
        .skip(1) // skip the first word (may be sentence-initial capital)
        .any(|w| w.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false))
}

/// Return `true` if the line matches "Word(s): rest" — a key-value fact.
fn looks_like_fact_line(line: &str) -> bool {
    if let Some(colon_pos) = line.find(':') {
        if colon_pos == 0 {
            return false;
        }
        let key = &line[..colon_pos];
        // Key should be non-empty, mostly alphabetic, and reasonably short
        let trimmed_key = key.trim();
        !trimmed_key.is_empty()
            && trimmed_key.len() <= 40
            && trimmed_key
                .chars()
                .all(|c| c.is_alphabetic() || c == ' ' || c == '_' || c == '-')
    } else {
        false
    }
}

/// Return `true` if the text contains something that looks like a date
/// (a 4-digit year, or a month name).
fn contains_date(text: &str) -> bool {
    const MONTHS: &[&str] = &[
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];

    let lower = text.to_lowercase();

    // Check for 4-digit year in range 1900-2099
    let bytes = text.as_bytes();
    for i in 0..bytes.len().saturating_sub(3) {
        if bytes[i].is_ascii_digit()
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
            && bytes[i + 3].is_ascii_digit()
        {
            let year_str = &text[i..i + 4];
            if let Ok(year) = year_str.parse::<u32>() {
                if (1900..=2099).contains(&year) {
                    return true;
                }
            }
        }
    }

    // Check for month names
    MONTHS.iter().any(|m| lower.contains(m))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // 1. Budget enforcement — total tokens_used must never exceed budget
    #[test]
    fn test_budget_enforcement() {
        let memories = vec![
            MemoryInput {
                id: 1,
                content: "A".repeat(400), // ~100 tokens
                importance: 0.9,
            },
            MemoryInput {
                id: 2,
                content: "B".repeat(400),
                importance: 0.8,
            },
            MemoryInput {
                id: 3,
                content: "C".repeat(400),
                importance: 0.7,
            },
        ];

        let budget = 120; // tight budget — should not fit all three at None
        let entries = ContextCompressor::compress_for_context(&memories, budget);

        let total_used: usize = entries.iter().map(|e| e.tokens_used).sum();
        assert!(
            total_used <= budget,
            "total_used={} exceeded budget={}",
            total_used,
            budget
        );
    }

    // 2. Adaptive escalation — less important memory gets heavier compression
    #[test]
    fn test_adaptive_escalation() {
        // Create a high-importance large memory and a low-importance one.
        // Budget is tight enough that the low-importance one must be compressed.
        let long_content = "The project launched in January 2024. Alice and Bob led the team. \
            The revenue grew by 40% year over year. Customer satisfaction reached 95%. \
            The new platform handles 10 million requests per day."
            .repeat(5);

        let memories = vec![
            MemoryInput {
                id: 1,
                content: long_content.clone(),
                importance: 1.0,
            },
            MemoryInput {
                id: 2,
                content: long_content.clone(),
                importance: 0.1,
            },
        ];

        // Budget: can fit one at None, the second must be compressed
        let one_token_count = ContextCompressor::estimate_tokens(&long_content);
        let budget = one_token_count + one_token_count / 4; // room for one full + partial

        let entries = ContextCompressor::compress_for_context(&memories, budget);

        // First entry (higher importance) should be at None or Light
        if let Some(first) = entries.first() {
            assert_eq!(first.original_id, 1, "highest importance should be first");
        }

        // If both entries exist, the second should be more compressed
        if entries.len() == 2 {
            let first_level = entries[0].compression_level as u8;
            let second_level = entries[1].compression_level as u8;
            assert!(
                second_level >= first_level,
                "less important memory should have equal or heavier compression"
            );
        }
    }

    // 3. Empty input returns empty output
    #[test]
    fn test_empty_input() {
        let entries = ContextCompressor::compress_for_context(&[], 1000);
        assert!(entries.is_empty());
    }

    // 4. Single memory that fits returns at None level
    #[test]
    fn test_single_memory_fits_at_none() {
        let content = "This is a short note.";
        let memories = vec![MemoryInput {
            id: 42,
            content: content.to_string(),
            importance: 0.5,
        }];

        let budget = 1000; // very generous
        let entries = ContextCompressor::compress_for_context(&memories, budget);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].original_id, 42);
        assert_eq!(entries[0].compression_level, CompressionLevel::None);
        assert_eq!(entries[0].compressed_content, content);
    }

    // 5. All memories exceed budget — returns only what fits
    #[test]
    fn test_all_memories_exceed_budget_returns_partial() {
        let memories = vec![
            MemoryInput {
                id: 1,
                content: "A".repeat(1000), // 250 tokens
                importance: 0.9,
            },
            MemoryInput {
                id: 2,
                content: "B".repeat(1000),
                importance: 0.8,
            },
            MemoryInput {
                id: 3,
                content: "C".repeat(1000),
                importance: 0.7,
            },
        ];

        let budget = 1; // impossibly small — nothing should fit
        let entries = ContextCompressor::compress_for_context(&memories, budget);

        assert!(
            entries.is_empty(),
            "nothing should fit in a budget of 1 token"
        );
    }

    // 6. Token estimation accuracy
    #[test]
    fn test_token_estimation() {
        // Empty string
        assert_eq!(ContextCompressor::estimate_tokens(""), 0);

        // Exactly 4 chars → 1 token
        assert_eq!(ContextCompressor::estimate_tokens("abcd"), 1);

        // 8 chars → 2 tokens
        assert_eq!(ContextCompressor::estimate_tokens("abcdefgh"), 2);

        // 100 chars → 25 tokens
        let s = "a".repeat(100);
        assert_eq!(ContextCompressor::estimate_tokens(&s), 25);

        // 5 chars → ceil(5/4) = 2 (rounds up)
        assert_eq!(ContextCompressor::estimate_tokens("abcde"), 2);
    }

    // 7. Light compression removes filler
    #[test]
    fn test_light_compression_removes_filler() {
        let text = "This is basically a very simple test. It is, honestly, quite straightforward.";
        let compressed = ContextCompressor::compress_light(text);

        // Filler words should be absent or reduced
        assert!(
            compressed.len() < text.len(),
            "light compression should shorten text"
        );
        // "basically" should be removed
        assert!(
            !compressed.to_lowercase().contains("basically"),
            "filler 'basically' should be removed"
        );
        // "honestly" should be removed
        assert!(
            !compressed.to_lowercase().contains("honestly"),
            "filler 'honestly' should be removed"
        );
    }

    // 8. Heavy compression extracts facts only
    #[test]
    fn test_heavy_compression_extracts_facts() {
        let text = "The meeting was uneventful.\n\
            Revenue: 1.5 million dollars\n\
            Founded: January 2020\n\
            The weather was nice today.\n\
            Headcount: 42 engineers";

        let compressed = ContextCompressor::compress_heavy(text);

        // Should include the fact lines and number/date lines
        assert!(
            compressed.contains("Revenue:") || compressed.contains("1.5"),
            "should include revenue fact"
        );
        assert!(
            compressed.contains("Headcount:") || compressed.contains("42"),
            "should include headcount fact"
        );
        // The non-fact lines should not dominate
        let lines: Vec<&str> = compressed.lines().collect();
        assert!(
            lines.len() <= 4,
            "heavy compression should produce few lines, got {}",
            lines.len()
        );
    }

    // Bonus: compress_single dispatches correctly
    #[test]
    fn test_compress_single_none_returns_unchanged() {
        let text = "Hello, world!";
        let result = ContextCompressor::compress_single(text, CompressionLevel::None);
        assert_eq!(result, text);
    }

    // Bonus: budget() reflects configured total
    #[test]
    fn test_budget_reflects_configured_total() {
        let compressor = ContextCompressor::new(8192);
        let b = compressor.budget();
        assert_eq!(b.total, 8192);
        assert_eq!(b.used, 0);
        assert_eq!(b.remaining, 8192);
    }

    // Bonus: medium compression keeps first sentence
    #[test]
    fn test_medium_compression_keeps_first_sentence() {
        let text = "First sentence here. Second sentence with Entity Name. Third unimportant one.";
        let compressed = ContextCompressor::compress_medium(text);

        assert!(
            compressed.contains("First sentence here"),
            "medium compression should keep first sentence"
        );
    }
}
