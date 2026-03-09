//! Maximal Marginal Relevance (MMR) — diversity-aware retrieval
//!
//! Implements the MMR algorithm from Carbonell & Goldstein (1998).
//! Iteratively selects documents that maximize:
//!
//! ```text
//! MMR(d) = λ * sim(d, query) - (1-λ) * max(sim(d, selected))
//! ```
//!
//! This balances relevance to the query against redundancy with already-selected results,
//! producing a diverse yet relevant result set.

use serde_json::Value;

/// Configuration for MMR selection
#[derive(Debug, Clone)]
pub struct MmrConfig {
    /// Trade-off between relevance and diversity.
    /// 0.0 = pure diversity, 1.0 = pure relevance
    pub lambda: f32,
    /// Number of results to return
    pub top_k: usize,
    /// Size of candidate pool to consider (candidates are truncated to this before selection)
    pub candidate_pool: usize,
}

impl Default for MmrConfig {
    fn default() -> Self {
        Self {
            lambda: 0.7,
            top_k: 10,
            candidate_pool: 50,
        }
    }
}

/// A candidate document for MMR selection
#[derive(Debug, Clone)]
pub struct MmrCandidate {
    /// Unique identifier for the memory
    pub id: i64,
    /// Text content of the memory
    pub content: String,
    /// Embedding vector for similarity computation
    pub embedding: Vec<f32>,
    /// Original relevance score from upstream retrieval
    pub original_score: f32,
    /// Optional JSON metadata
    pub metadata: Option<Value>,
}

/// A result produced by MMR selection
#[derive(Debug, Clone)]
pub struct MmrResult {
    /// Unique identifier for the memory
    pub id: i64,
    /// Text content of the memory
    pub content: String,
    /// Original relevance score from upstream retrieval
    pub score: f32,
    /// Combined MMR score at time of selection
    pub mmr_score: f32,
    /// Optional JSON metadata
    pub metadata: Option<Value>,
}

/// Compute cosine similarity between two vectors.
///
/// Returns 0.0 if either vector has zero magnitude (avoids division by zero).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }

    // Clamp to [-1, 1] to handle floating-point rounding
    (dot / (mag_a * mag_b)).clamp(-1.0, 1.0)
}

/// Select a diverse and relevant subset of candidates using the MMR algorithm.
///
/// # Arguments
/// * `query_embedding` — embedding of the search query
/// * `candidates` — pool of candidate documents; sorted by `original_score` descending
///   before selection begins
/// * `config` — MMR configuration
///
/// # Returns
/// Up to `config.top_k` results in selection order (most MMR-valuable first).
pub fn mmr_select(
    query_embedding: &[f32],
    candidates: Vec<MmrCandidate>,
    config: &MmrConfig,
) -> Vec<MmrResult> {
    if candidates.is_empty() {
        return Vec::new();
    }

    // Limit to candidate pool
    let mut pool: Vec<MmrCandidate> = candidates.into_iter().take(config.candidate_pool).collect();

    let target_k = config.top_k.min(pool.len());
    let mut selected: Vec<MmrResult> = Vec::with_capacity(target_k);
    // Track embeddings of already-selected documents for redundancy computation
    let mut selected_embeddings: Vec<Vec<f32>> = Vec::with_capacity(target_k);

    while selected.len() < target_k && !pool.is_empty() {
        let mut best_idx: Option<usize> = None;
        let mut best_mmr = f32::NEG_INFINITY;

        for (i, candidate) in pool.iter().enumerate() {
            let relevance = if query_embedding.is_empty() {
                candidate.original_score
            } else {
                cosine_similarity(query_embedding, &candidate.embedding)
            };

            // Max similarity to any already-selected document
            let max_redundancy = if selected_embeddings.is_empty() {
                0.0
            } else {
                selected_embeddings
                    .iter()
                    .map(|s| cosine_similarity(&candidate.embedding, s))
                    .fold(f32::NEG_INFINITY, f32::max)
            };

            let mmr_score = config.lambda * relevance - (1.0 - config.lambda) * max_redundancy;

            if best_idx.is_none() || mmr_score > best_mmr {
                best_mmr = mmr_score;
                best_idx = Some(i);
            }
        }

        if let Some(idx) = best_idx {
            let candidate = pool.remove(idx);
            selected_embeddings.push(candidate.embedding.clone());
            selected.push(MmrResult {
                id: candidate.id,
                content: candidate.content,
                score: candidate.original_score,
                mmr_score: best_mmr,
                metadata: candidate.metadata,
            });
        } else {
            break;
        }
    }

    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn make_candidate(id: i64, embedding: Vec<f32>, score: f32) -> MmrCandidate {
        MmrCandidate {
            id,
            content: format!("content {id}"),
            embedding,
            original_score: score,
            metadata: None,
        }
    }

    // ── cosine_similarity tests ───────────────────────────────────────────────

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0_f32, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!(
            (sim - 1.0).abs() < 1e-6,
            "identical vectors must have similarity 1.0, got {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0_f32, 0.0, 0.0];
        let b = vec![0.0_f32, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(
            sim.abs() < 1e-6,
            "orthogonal vectors must have similarity 0.0, got {sim}"
        );
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let zero = vec![0.0_f32, 0.0, 0.0];
        let other = vec![1.0_f32, 2.0, 3.0];
        assert_eq!(cosine_similarity(&zero, &other), 0.0);
        assert_eq!(cosine_similarity(&other, &zero), 0.0);
        assert_eq!(cosine_similarity(&zero, &zero), 0.0);
    }

    // ── mmr_select edge-case tests ────────────────────────────────────────────

    #[test]
    fn test_mmr_empty_candidates() {
        let config = MmrConfig::default();
        let result = mmr_select(&[1.0, 0.0], vec![], &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_mmr_single_candidate() {
        let config = MmrConfig {
            top_k: 5,
            ..Default::default()
        };
        let candidates = vec![make_candidate(42, vec![1.0, 0.0], 0.9)];
        let result = mmr_select(&[1.0, 0.0], candidates, &config);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, 42);
    }

    #[test]
    fn test_mmr_top_k_limit() {
        let config = MmrConfig {
            top_k: 3,
            candidate_pool: 50,
            lambda: 0.7,
        };
        // Provide 10 candidates
        let candidates: Vec<MmrCandidate> = (0..10)
            .map(|i| {
                // Each candidate has a unique, non-zero embedding
                let emb = vec![i as f32 + 1.0, 0.0];
                make_candidate(i, emb, 1.0 - i as f32 * 0.05)
            })
            .collect();

        let result = mmr_select(&[1.0, 0.0], candidates, &config);
        assert_eq!(result.len(), 3, "must respect top_k=3");
    }

    // ── mmr_select behavioural tests ──────────────────────────────────────────

    #[test]
    fn test_mmr_pure_relevance() {
        // lambda=1.0: MMR degenerates to pure relevance → order by original_score
        let config = MmrConfig {
            lambda: 1.0,
            top_k: 3,
            candidate_pool: 50,
        };

        // Three candidates with clearly distinct scores, distinct embeddings
        let candidates = vec![
            make_candidate(1, vec![1.0, 0.0, 0.0], 0.9),
            make_candidate(2, vec![0.0, 1.0, 0.0], 0.7),
            make_candidate(3, vec![0.0, 0.0, 1.0], 0.5),
        ];

        // Use a query aligned with the first candidate so relevance = cosine similarity
        let query = vec![1.0_f32, 0.0, 0.0];
        let result = mmr_select(&query, candidates, &config);

        assert_eq!(result.len(), 3);
        // Highest cosine sim to query is candidate 1 (sim=1.0), then 2 & 3 (sim=0.0)
        assert_eq!(result[0].id, 1, "first pick must be the most relevant");
    }

    #[test]
    fn test_mmr_pure_diversity() {
        // lambda=0.0: MMR degenerates to pure diversity (maximise distance from selected set)
        let config = MmrConfig {
            lambda: 0.0,
            top_k: 3,
            candidate_pool: 50,
        };

        // Three candidates — candidates 2 and 3 are nearly identical; 4 is orthogonal
        let candidates = vec![
            make_candidate(1, vec![1.0, 0.0], 0.9), // first pick: no selected set yet
            make_candidate(2, vec![1.0, 0.0], 0.8), // very similar to 1
            make_candidate(3, vec![0.0, 1.0], 0.5), // orthogonal to 1
        ];

        let query = vec![0.0_f32, 0.0]; // query irrelevant when lambda=0
        let result = mmr_select(&query, candidates, &config);

        assert_eq!(result.len(), 3);
        // After picking candidate 1, candidate 3 should be preferred over 2
        // because it is more distant (cosine sim to 1 is 0.0 vs 1.0)
        assert_eq!(
            result[1].id, 3,
            "second pick must be the most diverse (orthogonal) candidate"
        );
    }

    #[test]
    fn test_mmr_balanced() {
        // lambda=0.5: balanced blend — verify we get the expected number of results
        // and that they are not all from one extreme
        let config = MmrConfig {
            lambda: 0.5,
            top_k: 3,
            candidate_pool: 50,
        };

        let candidates = vec![
            make_candidate(1, vec![1.0, 0.0, 0.0], 0.9),
            make_candidate(2, vec![0.9, 0.1, 0.0], 0.85), // very similar to 1
            make_candidate(3, vec![0.0, 1.0, 0.0], 0.6),  // diverse from 1
            make_candidate(4, vec![0.0, 0.0, 1.0], 0.4),  // diverse from all above
        ];

        let query = vec![1.0_f32, 0.0, 0.0];
        let result = mmr_select(&query, candidates, &config);

        assert_eq!(result.len(), 3);

        // The results should include some diversity: candidate 2 (near-duplicate of 1)
        // should be deprioritised in favour of 3 or 4
        let ids: Vec<i64> = result.iter().map(|r| r.id).collect();
        // At least one of the diverse candidates should appear
        assert!(
            ids.contains(&3) || ids.contains(&4),
            "balanced MMR must include at least one diverse candidate, got ids: {ids:?}"
        );
    }
}
