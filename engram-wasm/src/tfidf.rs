//! TF-IDF vectorization and cosine similarity — extracted from engram-core.
//!
//! Uses the feature-hashing trick to produce fixed-size embedding vectors
//! without requiring a pre-built vocabulary. This is the same algorithm
//! used by `TfIdfEmbedder` in `src/embedding/tfidf.rs`.
//!
//! ## Algorithm
//!
//! 1. Tokenize text (lowercase alphanumeric, min length 2).
//! 2. Compute TF: `log(1 + count / doc_len)` for each token.
//! 3. Approximate IDF: `1 + token_len * 0.1` (longer = rarer heuristic).
//! 4. Include bigrams weighted at 0.5x for better semantic capture.
//! 5. Apply feature hashing with sign to reduce collision impact.
//! 6. L2-normalize the resulting vector.
//!
//! ## Invariants
//!
//! - Output vector always has exactly `dimensions` elements.
//! - Empty input returns an all-zero vector of the specified dimensions.
//! - Non-empty input is L2-normalized (‖v‖ ≈ 1.0).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Default embedding dimension used by engram-core.
pub const DEFAULT_DIMENSIONS: usize = 384;

/// Produce a TF-IDF embedding of `text` with `dimensions` output components.
///
/// Uses the feature-hashing trick: no vocabulary required, fully deterministic.
pub fn tfidf_embed(text: &str, dimensions: usize) -> Vec<f32> {
    let dimensions = dimensions.max(1);
    let tokens = tokenize(text);
    let mut embedding = vec![0.0_f32; dimensions];

    if tokens.is_empty() {
        return embedding;
    }

    let doc_len = tokens.len() as f32;

    // Bigrams (weighted 0.5x) — process before consuming tokens
    for window in tokens.windows(2) {
        let idx = hash_bigram(&window[0], &window[1], dimensions);
        let sign = hash_bigram_sign(&window[0], &window[1]);
        embedding[idx] += 0.5 * sign;
    }

    // Count term frequencies
    let mut tf: HashMap<String, f32> = HashMap::new();
    for token in tokens {
        *tf.entry(token).or_insert(0.0) += 1.0;
    }

    // Apply TF-IDF weighting with feature hashing
    for (token, count) in tf {
        // TF: log(1 + count/doc_len)
        let tf_score = (1.0 + count / doc_len).ln();
        // IDF approximation: longer tokens are rarer
        let idf_score = 1.0 + (token.len() as f32 * 0.1);
        let weight = tf_score * idf_score;

        let idx = hash_token(&token, dimensions);
        let sign = hash_sign(&token);
        embedding[idx] += weight * sign;
    }

    // L2 normalize
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut embedding {
            *x /= norm;
        }
    }

    embedding
}

/// Compute cosine similarity between two vectors.
///
/// Returns a value in [-1.0, 1.0]. Returns 0.0 if either vector is zero.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}

/// Tokenize text into lowercase alphanumeric tokens (minimum length 2).
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() >= 2)
        .map(String::from)
        .collect()
}

/// Hash a unigram token to a dimension index.
fn hash_token(token: &str, dimensions: usize) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    token.hash(&mut hasher);
    (hasher.finish() as usize) % dimensions
}

/// Hash a bigram to a dimension index.
fn hash_bigram(t1: &str, t2: &str, dimensions: usize) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    t1.hash(&mut hasher);
    "_".hash(&mut hasher);
    t2.hash(&mut hasher);
    (hasher.finish() as usize) % dimensions
}

/// Sign (+1 or -1) for a unigram, used to reduce hash collision impact.
fn hash_sign(token: &str) -> f32 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    token.hash(&mut hasher);
    "_sign".hash(&mut hasher);
    if hasher.finish() % 2 == 0 { 1.0 } else { -1.0 }
}

/// Sign (+1 or -1) for a bigram.
fn hash_bigram_sign(t1: &str, t2: &str) -> f32 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    t1.hash(&mut hasher);
    "_".hash(&mut hasher);
    t2.hash(&mut hasher);
    "_sign".hash(&mut hasher);
    if hasher.finish() % 2 == 0 { 1.0 } else { -1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embed_deterministic() {
        let a = tfidf_embed("hello world", 384);
        let b = tfidf_embed("hello world", 384);
        assert_eq!(a, b, "Same input must produce identical embeddings");
    }

    #[test]
    fn test_embed_correct_dimensions() {
        let v = tfidf_embed("some text", 128);
        assert_eq!(v.len(), 128);
    }

    #[test]
    fn test_embed_empty_is_zero() {
        let v = tfidf_embed("", 384);
        assert!(v.iter().all(|&x| x == 0.0), "Empty input should give zero vector");
    }

    #[test]
    fn test_embed_non_empty_is_normalized() {
        let v = tfidf_embed("rust programming language", 384);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "Non-empty embedding should be L2-normalized");
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let v = tfidf_embed("rust is great", 384);
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-5, "Identical vectors should have similarity ~1.0");
    }

    #[test]
    fn test_cosine_similarity_similar_higher_than_different() {
        let v1 = tfidf_embed("the quick brown fox", 384);
        let v2 = tfidf_embed("a fast brown fox", 384);
        let v3 = tfidf_embed("quantum thermodynamics equations", 384);

        let sim_close = cosine_similarity(&v1, &v2);
        let sim_far = cosine_similarity(&v1, &v3);

        assert!(sim_close > sim_far, "Similar texts should score higher");
    }

    #[test]
    fn test_cosine_similarity_empty_vectors() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0_f32; 4];
        let b = vec![1.0, 0.0, 0.0, 0.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }
}
