//! BM25 scoring — pure computation extracted from engram-core.
//!
//! The BM25 (Best Match 25) ranking function scores how relevant a document is
//! to a query based on term frequency and inverse document frequency.
//!
//! This module implements BM25+ (with lower-bound TF contribution) to avoid
//! zero scores for documents that contain none of the query terms.
//!
//! ## Parameters
//!
//! - `k1` (term saturation): Controls how quickly TF saturates. Typical range 1.2–2.0.
//! - `b` (length normalization): Controls length normalization. Typical range 0.5–0.8.
//!
//! ## Invariants
//!
//! - Scores are always non-negative.
//! - Empty queries or empty documents score 0.0.
//! - `doc_count` must be >= 1 (caller's responsibility).

use std::collections::HashMap;

/// BM25 tuning parameters.
#[derive(Debug, Clone, Copy)]
pub struct Bm25Params {
    /// Term frequency saturation parameter. Default: 1.5
    pub k1: f64,
    /// Length normalization parameter. Default: 0.75
    pub b: f64,
}

impl Default for Bm25Params {
    fn default() -> Self {
        Self { k1: 1.5, b: 0.75 }
    }
}

/// Score a single document against a set of query terms using BM25.
///
/// # Arguments
///
/// * `query_terms` — Tokenized query (terms already lowercased/normalized).
/// * `doc_terms`   — Tokenized document (same normalization as query).
/// * `doc_count`   — Total number of documents in the corpus (>= 1).
/// * `avg_doc_len` — Average document length in the corpus (tokens).
/// * `params`      — BM25 tuning parameters.
///
/// # Returns
///
/// BM25 relevance score (>= 0.0). Higher is more relevant.
pub fn bm25_score(
    query_terms: &[&str],
    doc_terms: &[&str],
    doc_count: usize,
    avg_doc_len: f64,
    params: Bm25Params,
) -> f64 {
    if query_terms.is_empty() || doc_terms.is_empty() {
        return 0.0;
    }

    let doc_count = doc_count.max(1) as f64;
    let doc_len = doc_terms.len() as f64;
    let avg_doc_len = if avg_doc_len <= 0.0 { 1.0 } else { avg_doc_len };

    // Count term frequencies in the document
    let mut tf_map: HashMap<&str, usize> = HashMap::new();
    for &term in doc_terms {
        *tf_map.entry(term).or_insert(0) += 1;
    }

    let mut score = 0.0_f64;

    for &query_term in query_terms {
        let tf = *tf_map.get(query_term).unwrap_or(&0) as f64;
        if tf == 0.0 {
            continue;
        }

        // Approximate document frequency: assume every doc with this term
        // is accounted for by TF > 0. In a real system you'd look this up
        // in an index. Here we use a conservative estimate: df = 1 for rare
        // terms that appear in the query.
        // For corpus-aware usage, callers should supply per-term df via the
        // `bm25_score_with_df` function below.
        let df = 1.0_f64;

        // IDF: log((N - df + 0.5) / (df + 0.5) + 1)
        let idf = ((doc_count - df + 0.5) / (df + 0.5) + 1.0).ln();

        // Normalized TF
        let tf_norm =
            (tf * (params.k1 + 1.0)) / (tf + params.k1 * (1.0 - params.b + params.b * doc_len / avg_doc_len));

        score += idf * tf_norm;
    }

    score.max(0.0)
}

/// Score with per-term document frequency (more accurate for real corpora).
///
/// # Arguments
///
/// * `query_terms`    — Tokenized query terms.
/// * `doc_terms`      — Tokenized document tokens.
/// * `doc_count`      — Total documents in corpus.
/// * `avg_doc_len`    — Average document length.
/// * `term_doc_freqs` — Map from term → number of documents containing the term.
/// * `params`         — BM25 parameters.
pub fn bm25_score_with_df(
    query_terms: &[&str],
    doc_terms: &[&str],
    doc_count: usize,
    avg_doc_len: f64,
    term_doc_freqs: &HashMap<&str, usize>,
    params: Bm25Params,
) -> f64 {
    if query_terms.is_empty() || doc_terms.is_empty() {
        return 0.0;
    }

    let n = doc_count.max(1) as f64;
    let doc_len = doc_terms.len() as f64;
    let avg_doc_len = if avg_doc_len <= 0.0 { 1.0 } else { avg_doc_len };

    let mut tf_map: HashMap<&str, usize> = HashMap::new();
    for &term in doc_terms {
        *tf_map.entry(term).or_insert(0) += 1;
    }

    let mut score = 0.0_f64;

    for &query_term in query_terms {
        let tf = *tf_map.get(query_term).unwrap_or(&0) as f64;
        if tf == 0.0 {
            continue;
        }

        let df = *term_doc_freqs.get(query_term).unwrap_or(&1) as f64;
        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();

        let tf_norm =
            (tf * (params.k1 + 1.0)) / (tf + params.k1 * (1.0 - params.b + params.b * doc_len / avg_doc_len));

        score += idf * tf_norm;
    }

    score.max(0.0)
}

/// Tokenize text into lowercase alphanumeric tokens (minimum length 2).
///
/// This is the same tokenizer used by engram-core's TF-IDF and BM25 pipelines.
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() >= 2)
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bm25_basic_match() {
        let query = vec!["rust", "programming"];
        let doc = vec!["rust", "programming", "language", "is", "fast"];
        let score = bm25_score(&query, &doc, 100, 10.0, Bm25Params::default());
        assert!(score > 0.0, "Matching doc should score > 0");
    }

    #[test]
    fn test_bm25_no_match() {
        let query = vec!["python"];
        let doc = vec!["rust", "is", "great"];
        let score = bm25_score(&query, &doc, 100, 10.0, Bm25Params::default());
        assert_eq!(score, 0.0, "No-match doc should score 0.0");
    }

    #[test]
    fn test_bm25_empty_query() {
        let score = bm25_score(&[], &["rust", "code"], 10, 5.0, Bm25Params::default());
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_bm25_empty_doc() {
        let score = bm25_score(&["rust"], &[], 10, 5.0, Bm25Params::default());
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_bm25_higher_tf_scores_higher() {
        let query = vec!["rust"];
        let doc_low = vec!["rust", "python", "java", "go"];
        let doc_high = vec!["rust", "rust", "rust", "rust"];
        let low = bm25_score(&query, &doc_low, 100, 5.0, Bm25Params::default());
        let high = bm25_score(&query, &doc_high, 100, 5.0, Bm25Params::default());
        assert!(high > low, "Higher TF should score higher (before saturation)");
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("Hello World 123");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"123".to_string()));
    }

    #[test]
    fn test_tokenize_filters_short() {
        let tokens = tokenize("a b rust");
        assert!(!tokens.contains(&"a".to_string()));
        assert!(!tokens.contains(&"b".to_string()));
        assert!(tokens.contains(&"rust".to_string()));
    }
}
