//! TF-IDF based embedding fallback
//!
//! Simple, fast, no external dependencies. Good for testing and
//! environments where API calls aren't possible.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::embedding::Embedder;
use crate::error::Result;

/// TF-IDF based embedder using hashing trick
pub struct TfIdfEmbedder {
    dimensions: usize,
}

impl TfIdfEmbedder {
    pub fn new(dimensions: usize) -> Self {
        Self { dimensions }
    }

    /// Tokenize text into lowercase words
    fn tokenize(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() > 1)
            .map(String::from)
            .collect()
    }

    /// Hash a token to a dimension index
    fn hash_token(token: &str, dimensions: usize) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        token.hash(&mut hasher);
        (hasher.finish() as usize) % dimensions
    }

    /// Hash a bigram (token1 + "_" + token2) directly to a dimension index
    fn hash_bigram(token1: &str, token2: &str, dimensions: usize) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        token1.hash(&mut hasher);
        "_".hash(&mut hasher);
        token2.hash(&mut hasher);
        (hasher.finish() as usize) % dimensions
    }

    /// Get sign for feature hashing (reduces collision impact)
    fn hash_sign(token: &str) -> f32 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        token.hash(&mut hasher);
        "_sign".hash(&mut hasher);
        if hasher.finish().is_multiple_of(2) {
            1.0
        } else {
            -1.0
        }
    }

    /// Get sign for bigram feature hashing
    fn hash_bigram_sign(token1: &str, token2: &str) -> f32 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        token1.hash(&mut hasher);
        "_".hash(&mut hasher);
        token2.hash(&mut hasher);
        "_sign".hash(&mut hasher);
        if hasher.finish().is_multiple_of(2) {
            1.0
        } else {
            -1.0
        }
    }
}

impl Embedder for TfIdfEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let tokens = Self::tokenize(text);
        let mut embedding = vec![0.0_f32; self.dimensions];

        if tokens.is_empty() {
            return Ok(embedding);
        }

        let doc_len = tokens.len() as f32;

        // Also add bigrams for better semantic capture
        // Process bigrams first using references to avoid cloning/moving strings yet
        for window in tokens.windows(2) {
            let idx = Self::hash_bigram(&window[0], &window[1], self.dimensions);
            let sign = Self::hash_bigram_sign(&window[0], &window[1]);
            embedding[idx] += 0.5 * sign; // Bigrams weighted less
        }

        // Count term frequencies
        // Consume tokens to avoid cloning strings when inserting into HashMap
        let mut tf: HashMap<String, f32> = HashMap::new();
        for token in tokens {
            *tf.entry(token).or_insert(0.0) += 1.0;
        }

        // Apply TF-IDF-like weighting with feature hashing
        for (token, count) in tf {
            // TF: log(1 + count/doc_len)
            let tf_score = (1.0 + count / doc_len).ln();

            // IDF approximation based on token length (longer = rarer)
            let idf_score = 1.0 + (token.len() as f32 * 0.1);

            let weight = tf_score * idf_score;
            let idx = Self::hash_token(&token, self.dimensions);
            let sign = Self::hash_sign(&token);

            embedding[idx] += weight * sign;
        }

        // L2 normalize
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut embedding {
                *x /= norm;
            }
        }

        Ok(embedding)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_name(&self) -> &str {
        "tfidf"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::cosine_similarity;

    #[test]
    fn test_tfidf_basic() {
        let embedder = TfIdfEmbedder::new(384);

        let e1 = embedder.embed("hello world").unwrap();
        let e2 = embedder.embed("hello world").unwrap();

        // Same text should produce identical embeddings
        assert_eq!(e1, e2);
    }

    #[test]
    fn test_tfidf_similarity() {
        let embedder = TfIdfEmbedder::new(384);

        let e1 = embedder
            .embed("the quick brown fox jumps over the lazy dog")
            .unwrap();
        let e2 = embedder
            .embed("a fast brown fox leaps over a sleepy dog")
            .unwrap();
        let e3 = embedder
            .embed("quantum physics and thermodynamics")
            .unwrap();

        // Similar sentences should have higher similarity
        let sim_similar = cosine_similarity(&e1, &e2);
        let sim_different = cosine_similarity(&e1, &e3);

        assert!(
            sim_similar > sim_different,
            "Similar sentences should have higher similarity"
        );
    }

    #[test]
    fn test_tfidf_empty() {
        let embedder = TfIdfEmbedder::new(384);
        let e = embedder.embed("").unwrap();
        assert_eq!(e.len(), 384);
        assert!(e.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn test_tfidf_normalized() {
        let embedder = TfIdfEmbedder::new(384);
        let e = embedder
            .embed("this is a test sentence with multiple words")
            .unwrap();

        let norm: f32 = e.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 0.001,
            "Embedding should be L2 normalized"
        );
    }
}
