//! Reciprocal Rank Fusion (RRF) — extracted from engram-core.
//!
//! RRF is a simple, parameter-light algorithm for combining multiple ranked
//! lists into a single merged ranking. It was proposed by Cormack et al. (2009)
//! and is used by engram-core to merge BM25 + semantic search results.
//!
//! ## Formula
//!
//! For document `d` in ranked list `r`, its RRF contribution is:
//!
//! ```text
//! rrf(d, r) = 1 / (k + rank(d, r))
//! ```
//!
//! The final score for document `d` is the sum across all ranked lists:
//!
//! ```text
//! score(d) = Σ_r  weight(r) / (k + rank(d, r))
//! ```
//!
//! where `k = 60` is the standard constant that reduces the impact of
//! high-rank differences.
//!
//! ## Invariants
//!
//! - Document IDs in the output are deduplicated (each appears once).
//! - Results are sorted by score descending.
//! - Documents not appearing in a list receive no contribution from that list.
//! - `k` must be > 0 (callers using the default `k = 60` are safe).

/// Default RRF constant. Typical value from the original paper.
pub const DEFAULT_K: f64 = 60.0;

/// A single ranked list entry: `(doc_id, rank)`.
/// Rank is 1-indexed (first place = rank 1).
#[derive(Debug, Clone, Copy)]
pub struct RankedItem {
    pub doc_id: u64,
    /// 1-indexed rank within this list
    pub rank: usize,
}

impl RankedItem {
    pub fn new(doc_id: u64, rank: usize) -> Self {
        Self { doc_id, rank }
    }
}

/// A single ranked list with an optional weight.
#[derive(Debug, Clone)]
pub struct RankedList {
    pub items: Vec<RankedItem>,
    /// Weight applied to contributions from this list. Default: 1.0.
    pub weight: f64,
}

impl RankedList {
    /// Create a ranked list from a vec of `(doc_id, rank)` pairs.
    pub fn new(items: Vec<RankedItem>) -> Self {
        Self { items, weight: 1.0 }
    }

    /// Create a ranked list with a custom weight.
    pub fn with_weight(items: Vec<RankedItem>, weight: f64) -> Self {
        Self { items, weight }
    }

    /// Build a ranked list from a slice of doc IDs in rank order (index 0 = rank 1).
    pub fn from_ordered(doc_ids: &[u64]) -> Self {
        let items = doc_ids
            .iter()
            .enumerate()
            .map(|(i, &id)| RankedItem::new(id, i + 1))
            .collect();
        Self::new(items)
    }
}

/// Merge multiple ranked lists using Reciprocal Rank Fusion.
///
/// # Arguments
///
/// * `lists` — One or more ranked lists to merge.
/// * `k`     — RRF constant (use `DEFAULT_K = 60.0`).
///
/// # Returns
///
/// Vec of `(doc_id, score)` sorted by score descending.
pub fn rrf_merge(lists: &[RankedList], k: f64) -> Vec<(u64, f64)> {
    let k = if k <= 0.0 { DEFAULT_K } else { k };

    let mut scores: std::collections::HashMap<u64, f64> = std::collections::HashMap::new();

    for list in lists {
        for item in &list.items {
            let contribution = list.weight / (k + item.rank as f64);
            *scores.entry(item.doc_id).or_insert(0.0) += contribution;
        }
    }

    let mut result: Vec<(u64, f64)> = scores.into_iter().collect();
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    result
}

/// Merge two ranked lists (keyword + semantic) with weights, the standard
/// engram-core hybrid-search pattern.
///
/// # Arguments
///
/// * `keyword_ids`     — Doc IDs ranked by BM25 score (index 0 = best).
/// * `semantic_ids`    — Doc IDs ranked by vector similarity (index 0 = best).
/// * `keyword_weight`  — Weight for keyword list. Default: 1.0.
/// * `semantic_weight` — Weight for semantic list. Default: 1.0.
/// * `k`               — RRF constant. Default: `DEFAULT_K`.
///
/// # Returns
///
/// Vec of `(doc_id, score)` sorted by score descending.
pub fn rrf_hybrid(
    keyword_ids: &[u64],
    semantic_ids: &[u64],
    keyword_weight: f64,
    semantic_weight: f64,
    k: f64,
) -> Vec<(u64, f64)> {
    let lists = vec![
        RankedList::with_weight(
            keyword_ids
                .iter()
                .enumerate()
                .map(|(i, &id)| RankedItem::new(id, i + 1))
                .collect(),
            keyword_weight,
        ),
        RankedList::with_weight(
            semantic_ids
                .iter()
                .enumerate()
                .map(|(i, &id)| RankedItem::new(id, i + 1))
                .collect(),
            semantic_weight,
        ),
    ];

    rrf_merge(&lists, k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrf_merge_single_list() {
        let list = RankedList::from_ordered(&[10, 20, 30]);
        let result = rrf_merge(&[list], DEFAULT_K);
        assert_eq!(result.len(), 3);
        // Rank 1 should score highest
        assert!(result[0].1 > result[1].1);
        assert!(result[1].1 > result[2].1);
        assert_eq!(result[0].0, 10);
    }

    #[test]
    fn test_rrf_merge_two_lists_boost_overlap() {
        // Doc 1 appears first in both lists → should win
        let list1 = RankedList::from_ordered(&[1, 2, 3]);
        let list2 = RankedList::from_ordered(&[1, 3, 2]);
        let result = rrf_merge(&[list1, list2], DEFAULT_K);
        assert_eq!(result[0].0, 1, "Top-ranked in both lists should be first");
    }

    #[test]
    fn test_rrf_merge_empty_lists() {
        let result = rrf_merge(&[], DEFAULT_K);
        assert!(result.is_empty());
    }

    #[test]
    fn test_rrf_merge_disjoint_lists() {
        let list1 = RankedList::from_ordered(&[1, 2]);
        let list2 = RankedList::from_ordered(&[3, 4]);
        let result = rrf_merge(&[list1, list2], DEFAULT_K);
        assert_eq!(result.len(), 4);
        // Rank-1 in either list should come first
        assert!(result[0].0 == 1 || result[0].0 == 3);
    }

    #[test]
    fn test_rrf_hybrid_symmetric() {
        let keyword = vec![1u64, 2, 3];
        let semantic = vec![3u64, 2, 1];
        let result = rrf_hybrid(&keyword, &semantic, 1.0, 1.0, DEFAULT_K);
        // Doc 2 appears at rank 2 in both → same score as doc 1 (rank 1 keyword, rank 3 semantic)
        // All three docs should be present
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_rrf_formula_values() {
        // Verify the formula: score = 1 / (k + rank)
        let k = 60.0;
        let expected_rank1 = 1.0 / (k + 1.0);
        let expected_rank5 = 1.0 / (k + 5.0);

        let list = RankedList::from_ordered(&[100, 200, 300, 400, 500]);
        let result = rrf_merge(&[list], k);

        let score_rank1 = result.iter().find(|(id, _)| *id == 100).unwrap().1;
        let score_rank5 = result.iter().find(|(id, _)| *id == 500).unwrap().1;

        assert!((score_rank1 - expected_rank1).abs() < 1e-9);
        assert!((score_rank5 - expected_rank5).abs() < 1e-9);
    }

    #[test]
    fn test_rrf_weighted_lists() {
        // List with weight 2.0 should dominate
        let list_low = RankedList::with_weight(
            vec![RankedItem::new(1, 1), RankedItem::new(2, 2)],
            1.0,
        );
        let list_high = RankedList::with_weight(
            vec![RankedItem::new(2, 1), RankedItem::new(1, 2)],
            2.0,
        );
        let result = rrf_merge(&[list_low, list_high], DEFAULT_K);
        // Doc 2 is rank 1 in the high-weight list → should win overall
        assert_eq!(result[0].0, 2);
    }
}
