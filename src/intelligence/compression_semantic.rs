//! Semantic Structured Compression — RML-1208
//!
//! Compresses verbose memory content into structured summaries targeting ~30x
//! token reduction using rule-based NLP techniques. Pure computation — no
//! database access, no network I/O.
//!
//! ## Pipeline
//! 1. Split text into sentences
//! 2. Strip filler and hedging phrases
//! 3. Extract proper nouns and number/date entities
//! 4. Derive subject-verb-object cores
//! 5. Deduplicate near-identical sentences (Jaccard > 0.6)
//! 6. Reassemble structured_content and key_facts
//!
//! ## Invariants
//! - Never panics on any input (including empty strings)
//! - Token estimation uses `text.len() / 4`
//! - Short content below `min_content_length` is returned verbatim

use std::collections::HashSet;

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

// =============================================================================
// Constants — filler / hedging phrase lists
// =============================================================================

/// Filler phrases that add no information and should be stripped.
const FILLER_PHRASES: &[&str] = &[
    "i think",
    "basically",
    "you know",
    "kind of",
    "sort of",
    "i mean",
    "like",
    "actually",
    "to be honest",
    "in my opinion",
    "i believe",
    "i guess",
    "i suppose",
    "it seems like",
    "more or less",
    "pretty much",
    "at the end of the day",
    "as a matter of fact",
    "the thing is",
    "to be fair",
    "honestly",
    "literally",
    "obviously",
    "clearly",
    "just",
    "simply",
    "basically speaking",
    "needless to say",
    "as you know",
    "for what it's worth",
];

/// Hedging phrases — uncertainty markers that inflate token count.
const HEDGING_PHRASES: &[&str] = &[
    "maybe",
    "perhaps",
    "sort of",
    "kind of",
    "somewhat",
    "rather",
    "fairly",
    "quite",
    "a bit",
    "a little",
    "in a way",
    "in some ways",
    "to some extent",
    "to a degree",
    "more or less",
];

// =============================================================================
// Regex patterns (compiled once via Lazy)
// =============================================================================

/// Matches capitalized words that could be proper nouns.
/// We use a simple pattern and filter sentence-start words in post-processing.
static PROPER_NOUN_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([A-Z][a-z]{2,}(?:\s+[A-Z][a-z]{2,})*)\b").expect("valid regex"));

/// Matches numbers (integers, decimals) and common date-like patterns.
static NUMBER_DATE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(\d{1,4}[/-]\d{1,2}[/-]\d{1,4}|\d{4}|\d+\.\d+|\d{1,3}(?:,\d{3})*(?:\.\d+)?)\b")
        .expect("valid regex")
});

/// Matches sentence-terminating punctuation followed by whitespace.
/// Used to split sentences without requiring look-behind.
static SENTENCE_SPLIT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[.!?]\s+").expect("valid regex"));

/// Verb word list used to identify the predicate in an SVO triple.
static COMMON_VERBS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(is|are|was|were|has|have|had|will|can|could|should|would|does|did|do|provides|uses|returns|creates|stores|contains|supports|requires|enables|implements|defines|allows|includes|handles|manages)\b")
        .expect("valid regex")
});

// =============================================================================
// Public types
// =============================================================================

/// Configuration for the semantic compressor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    /// Target compression ratio (0.0–1.0). Default: 0.1 (keep 10% of tokens).
    pub target_ratio: f32,
    /// Minimum content length (chars) to attempt compression. Default: 100.
    pub min_content_length: usize,
    /// Preserve proper nouns and numbers in `key_entities`. Default: true.
    pub preserve_entities: bool,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            target_ratio: 0.1,
            min_content_length: 100,
            preserve_entities: true,
        }
    }
}

/// The result of compressing a single piece of text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedMemory {
    /// Estimated original token count (`original_text.len() / 4`).
    pub original_tokens: usize,
    /// Estimated compressed token count (`structured_content.len() / 4`).
    pub compressed_tokens: usize,
    /// Actual ratio: `compressed_tokens as f32 / original_tokens as f32`.
    pub ratio: f32,
    /// Stripped, deduplicated sentence cores joined by ". ".
    pub structured_content: String,
    /// Proper nouns and numbers/dates extracted from the text.
    pub key_entities: Vec<String>,
    /// Sentences that contain at least one entity and one verb.
    pub key_facts: Vec<String>,
}

// =============================================================================
// SemanticCompressor
// =============================================================================

/// Rule-based semantic compressor — no ML required.
pub struct SemanticCompressor {
    config: CompressionConfig,
}

impl SemanticCompressor {
    /// Create a new compressor with the given configuration.
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }

    /// Compress a single text string.
    ///
    /// If the text is shorter than `config.min_content_length`, returns the
    /// text verbatim with a ratio of `1.0`.
    pub fn compress(&self, text: &str) -> CompressedMemory {
        let original_tokens = estimate_tokens(text);

        if text.trim().is_empty() {
            return CompressedMemory {
                original_tokens: 0,
                compressed_tokens: 0,
                ratio: 1.0,
                structured_content: String::new(),
                key_entities: Vec::new(),
                key_facts: Vec::new(),
            };
        }

        if text.trim().len() < self.config.min_content_length {
            return CompressedMemory {
                original_tokens,
                compressed_tokens: original_tokens,
                ratio: 1.0,
                structured_content: text.trim().to_string(),
                key_entities: Vec::new(),
                key_facts: Vec::new(),
            };
        }

        // Step 1 — Split into sentences
        let sentences = split_sentences(text);

        // Step 2 — Strip filler and hedging phrases
        let cleaned: Vec<String> = sentences
            .iter()
            .map(|s| strip_filler(s))
            .filter(|s| !s.trim().is_empty())
            .collect();

        // Step 3 & 4 — Extract entities + SVO cores
        let key_entities = if self.config.preserve_entities {
            extract_entities(&sentences)
        } else {
            Vec::new()
        };

        // Step 5 — Deduplicate similar sentences (Jaccard > 0.6)
        let deduped = deduplicate_sentences(&cleaned);

        // Step 6 — Build structured content from SVO cores
        let cores: Vec<String> = deduped.iter().map(|s| extract_svo_core(s)).collect();
        let structured_content = cores.join(". ");

        // Step 7 — Extract key facts (sentence with entity + verb)
        let key_facts = extract_key_facts(&deduped, &key_entities);

        let compressed_tokens = estimate_tokens(&structured_content);
        let ratio = if original_tokens == 0 {
            1.0
        } else {
            compressed_tokens as f32 / original_tokens as f32
        };

        CompressedMemory {
            original_tokens,
            compressed_tokens,
            ratio,
            structured_content,
            key_entities,
            key_facts,
        }
    }

    /// Reconstruct an approximate text from a `CompressedMemory`.
    ///
    /// Expands `key_facts` by appending a parenthetical list of related
    /// entities. If there are no key facts falls back to `structured_content`.
    pub fn decompress(&self, compressed: &CompressedMemory) -> String {
        if compressed.structured_content.is_empty() {
            return String::new();
        }

        if compressed.key_facts.is_empty() {
            return compressed.structured_content.clone();
        }

        let entity_context = if !compressed.key_entities.is_empty() {
            format!(" (entities: {})", compressed.key_entities.join(", "))
        } else {
            String::new()
        };

        let mut parts: Vec<String> = compressed.key_facts.clone();
        // Append entity context to the last fact
        if let Some(last) = parts.last_mut() {
            last.push_str(&entity_context);
        }
        parts.join(". ")
    }

    /// Compress a batch of texts.
    pub fn compress_batch(&self, texts: &[&str]) -> Vec<CompressedMemory> {
        texts.iter().map(|t| self.compress(t)).collect()
    }
}

// =============================================================================
// Internal helpers
// =============================================================================

/// Estimate token count: `text.len() / 4` (byte-length heuristic).
fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Split text into sentences on `.`, `!`, `?` boundaries.
///
/// Because the Rust `regex` crate does not support look-behind we match the
/// terminator + whitespace, then manually re-attach the terminator to the
/// preceding fragment.
fn split_sentences(text: &str) -> Vec<String> {
    // Find all match ranges [start, end) of "<punct><whitespace>" sequences
    let terminators: Vec<(usize, usize, char)> = SENTENCE_SPLIT_RE
        .find_iter(text)
        .map(|m| {
            // The punctuation character is the first byte of the match
            let punct = text[m.start()..].chars().next().unwrap_or('.');
            (m.start(), m.end(), punct)
        })
        .collect();

    if terminators.is_empty() {
        let trimmed = text.trim().to_string();
        return if trimmed.is_empty() {
            vec![]
        } else {
            vec![trimmed]
        };
    }

    let mut sentences: Vec<String> = Vec::new();
    let mut cursor = 0usize;

    for (t_start, t_end, punct) in &terminators {
        let fragment = text[cursor..*t_start].trim().to_string();
        if !fragment.is_empty() {
            sentences.push(format!("{fragment}{punct}"));
        }
        cursor = *t_end;
    }
    // Remainder after last terminator
    let tail = text[cursor..].trim().to_string();
    if !tail.is_empty() {
        sentences.push(tail);
    }

    sentences
}

/// Remove filler and hedging phrases (case-insensitive, whole-word).
fn strip_filler(text: &str) -> String {
    let mut result = text.to_string();

    // Sort by descending length so multi-word phrases match before sub-phrases
    let mut phrases: Vec<&str> = FILLER_PHRASES
        .iter()
        .chain(HEDGING_PHRASES.iter())
        .copied()
        .collect();
    phrases.sort_by_key(|b| std::cmp::Reverse(b.len()));
    phrases.dedup();

    for phrase in phrases {
        // Build a case-insensitive whole-word regex for this phrase
        let escaped = regex::escape(phrase);
        // Match phrase at word boundary (or start/end of string), possibly
        // followed by a comma or space, and remove it.
        if let Ok(re) = Regex::new(&format!(r"(?i)\b{escaped}\b[,\s]*")) {
            result = re.replace_all(&result, " ").to_string();
        }
    }

    // Collapse multiple spaces and trim
    let collapsed = result.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
}

/// Extract proper nouns and number/date entities from the original sentences.
///
/// To approximate "not at sentence start" without look-behind we build a set of
/// the first word of every sentence and exclude those from the proper-noun list
/// unless they appear again inside a sentence.
fn extract_entities(sentences: &[String]) -> Vec<String> {
    // Collect the first (lowercased) word of every sentence so we can skip them
    // when they appear at position 0 of a sentence.
    let sentence_starters: HashSet<String> = sentences
        .iter()
        .filter_map(|s| s.split_whitespace().next())
        .map(|w| w.to_lowercase())
        .collect();

    let full_text = sentences.join(" ");
    let mut entities: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Proper nouns — skip tokens whose lowercase form is a sentence starter
    // unless they appear mid-sentence at least once.
    for cap in PROPER_NOUN_RE.captures_iter(&full_text) {
        let entity = cap[1].to_string();
        let entity_lower = entity.to_lowercase();
        // Accept if it is NOT a plain sentence starter, OR if it appears
        // more than once (mid-sentence occurrences will make count > 1).
        let count = PROPER_NOUN_RE
            .find_iter(&full_text)
            .filter(|m| full_text[m.start()..m.end()].to_lowercase() == entity_lower)
            .count();
        if (!sentence_starters.contains(&entity_lower) || count > 1) && seen.insert(entity.clone())
        {
            entities.push(entity);
        }
    }

    // Numbers and dates
    for cap in NUMBER_DATE_RE.captures_iter(&full_text) {
        let token = cap[1].to_string();
        if seen.insert(token.clone()) {
            entities.push(token);
        }
    }

    entities
}

/// Compute Jaccard similarity between two sentences (token sets).
fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let set_a: HashSet<&str> = a.split_whitespace().collect();
    let set_b: HashSet<&str> = b.split_whitespace().collect();

    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }

    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();

    if union == 0 {
        1.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Deduplicate sentences where Jaccard similarity > 0.6.
/// Keeps the first of each near-duplicate group.
fn deduplicate_sentences(sentences: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();

    'outer: for sentence in sentences {
        for existing in &kept {
            if jaccard_similarity(sentence, existing) > 0.6 {
                continue 'outer;
            }
        }
        kept.push(sentence.clone());
    }

    kept
}

/// Extract a simplified SVO core from a sentence.
///
/// Finds the first verb match and returns the text up to and including a
/// short object span. Falls back to returning the full trimmed sentence if
/// no verb is found.
fn extract_svo_core(sentence: &str) -> String {
    let words: Vec<&str> = sentence.split_whitespace().collect();
    if words.len() <= 6 {
        // Already short enough — return as-is
        return sentence.trim().to_string();
    }

    if let Some(verb_match) = COMMON_VERBS.find(sentence) {
        // Take: everything before the verb (subject), the verb, and up to
        // 5 words after the verb (object span)
        let pre = &sentence[..verb_match.start()].trim();
        let post = &sentence[verb_match.end()..].trim();
        let object_words: Vec<&str> = post.split_whitespace().take(5).collect();
        let object = object_words.join(" ");
        let verb = verb_match.as_str();

        let parts = [*pre, verb, &object]
            .iter()
            .filter(|p| !p.is_empty())
            .copied()
            .collect::<Vec<_>>();
        return parts.join(" ");
    }

    // No verb found — truncate to first 8 words
    words[..words.len().min(8)].join(" ")
}

/// Extract key facts: sentences that contain at least one entity and one verb.
fn extract_key_facts(sentences: &[String], entities: &[String]) -> Vec<String> {
    sentences
        .iter()
        .filter(|s| {
            let has_verb = COMMON_VERBS.is_match(s);
            let s_lower = s.to_lowercase();
            let has_entity = entities.iter().any(|e| s_lower.contains(&e.to_lowercase()))
                || NUMBER_DATE_RE.is_match(s);
            has_verb && has_entity
        })
        .cloned()
        .collect()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn default_compressor() -> SemanticCompressor {
        SemanticCompressor::new(CompressionConfig::default())
    }

    // -------------------------------------------------------------------------
    // Test 1: Short text below min_content_length is returned verbatim
    // -------------------------------------------------------------------------
    #[test]
    fn test_short_text_returned_verbatim() {
        let compressor = default_compressor();
        let short = "Hello world.";
        assert!(short.len() < 100);
        let result = compressor.compress(short);
        assert_eq!(result.structured_content, short.trim());
        assert!((result.ratio - 1.0).abs() < f32::EPSILON);
    }

    // -------------------------------------------------------------------------
    // Test 2: Filler removal reduces content
    // -------------------------------------------------------------------------
    #[test]
    fn test_filler_removal_reduces_content() {
        let original = "I think basically you know we should sort of consider the proposal. \
                        Actually to be honest I believe we need to look at it more carefully. \
                        Kind of like the previous plan but maybe with more flexibility and scope.";
        let stripped = strip_filler(original);
        assert!(
            stripped.len() < original.len(),
            "stripped ({}) should be shorter than original ({})",
            stripped.len(),
            original.len()
        );
        // Key content words should still be present
        assert!(
            stripped.to_lowercase().contains("proposal")
                || stripped.to_lowercase().contains("consider")
        );
    }

    // -------------------------------------------------------------------------
    // Test 3: Entity extraction finds proper nouns
    // -------------------------------------------------------------------------
    #[test]
    fn test_entity_extraction_proper_nouns() {
        let sentences = vec![
            "Alice works at Google in San Francisco.".to_string(),
            "Bob joined Microsoft last year.".to_string(),
        ];
        let entities = extract_entities(&sentences);
        // Should find some of: Alice, Google, San, Francisco, Bob, Microsoft
        assert!(
            !entities.is_empty(),
            "expected entities, got none from: {sentences:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Test 4: Number / date extraction
    // -------------------------------------------------------------------------
    #[test]
    fn test_number_date_extraction() {
        let sentences = vec![
            "The project started on 2024-01-15 and costs 1500.00 dollars.".to_string(),
            "There were 42 participants in 2023.".to_string(),
        ];
        let entities = extract_entities(&sentences);
        // Should find numeric tokens like "2024-01-15", "1500.00", "42", "2023"
        let has_number = entities
            .iter()
            .any(|e| e.chars().any(|c| c.is_ascii_digit()));
        assert!(has_number, "expected numeric entities; got {entities:?}");
    }

    // -------------------------------------------------------------------------
    // Test 5: Deduplication of similar sentences
    // -------------------------------------------------------------------------
    #[test]
    fn test_deduplication_removes_near_duplicates() {
        let sentences = vec![
            "The cat sat on the mat.".to_string(),
            "The cat sat on the mat.".to_string(), // exact duplicate
            "The cat is sitting on the mat.".to_string(), // near duplicate
            "Dogs love to play in the park every afternoon.".to_string(),
        ];
        let deduped = deduplicate_sentences(&sentences);
        // Exact duplicate must be removed; unique sentence kept
        assert!(
            deduped.len() < sentences.len(),
            "deduped len {} should be < original len {}",
            deduped.len(),
            sentences.len()
        );
        assert!(deduped.iter().any(|s| s.contains("Dogs")));
    }

    // -------------------------------------------------------------------------
    // Test 6: Compression ratio computed correctly
    // -------------------------------------------------------------------------
    #[test]
    fn test_compression_ratio_computed() {
        let compressor = default_compressor();
        let text = "I think basically we need to understand that the system, \
                    you know, is sort of designed to handle large amounts of data. \
                    Actually to be honest the architecture was I believe chosen to \
                    support scalability. At the end of the day the database stores \
                    records and provides search functionality for the application. \
                    The API layer handles authentication and rate limiting as well.";
        let result = compressor.compress(text);
        assert!(
            result.ratio > 0.0 && result.ratio <= 1.0,
            "ratio {} should be in (0, 1]",
            result.ratio
        );
        assert_eq!(
            result.ratio,
            result.compressed_tokens as f32 / result.original_tokens as f32
        );
    }

    // -------------------------------------------------------------------------
    // Test 7: Decompress produces non-empty text
    // -------------------------------------------------------------------------
    #[test]
    fn test_decompress_produces_non_empty_text() {
        let compressor = default_compressor();
        let text = "Alice joined Google in 2022 as a senior engineer. \
                    She works on distributed systems and handles large scale data pipelines. \
                    The team uses Rust and Go for backend services in the cloud infrastructure.";
        let compressed = compressor.compress(text);
        let decompressed = compressor.decompress(&compressed);
        assert!(
            !decompressed.is_empty(),
            "decompress should produce non-empty text"
        );
    }

    // -------------------------------------------------------------------------
    // Test 8: Batch compression
    // -------------------------------------------------------------------------
    #[test]
    fn test_batch_compression() {
        let compressor = default_compressor();
        let texts = &[
            "Short text.",
            "Alice works at Google as a software engineer and manages infrastructure projects in California.",
            "The system provides search and storage capabilities for large enterprise applications.",
        ];
        let results = compressor.compress_batch(texts);
        assert_eq!(results.len(), texts.len());
    }

    // -------------------------------------------------------------------------
    // Test 9: Empty input handled gracefully
    // -------------------------------------------------------------------------
    #[test]
    fn test_empty_input_handled() {
        let compressor = default_compressor();
        let result = compressor.compress("");
        assert_eq!(result.original_tokens, 0);
        assert_eq!(result.compressed_tokens, 0);
        assert!(result.structured_content.is_empty());
        assert!(result.key_entities.is_empty());
    }

    // -------------------------------------------------------------------------
    // Test 10: Whitespace-only input handled gracefully
    // -------------------------------------------------------------------------
    #[test]
    fn test_whitespace_only_input_handled() {
        let compressor = default_compressor();
        let result = compressor.compress("   \n\t   ");
        assert!(result.structured_content.is_empty());
    }

    // -------------------------------------------------------------------------
    // Test 11: Jaccard similarity
    // -------------------------------------------------------------------------
    #[test]
    fn test_jaccard_identical_sentences() {
        let a = "the cat sat on the mat";
        assert!((jaccard_similarity(a, a) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_jaccard_disjoint_sentences() {
        let a = "apple orange banana";
        let b = "car truck motorcycle";
        assert_eq!(jaccard_similarity(a, b), 0.0);
    }
}
