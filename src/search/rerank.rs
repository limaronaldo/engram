//! Search result reranking
//!
//! Provides post-search reranking to improve result quality through:
//! - Query-document relevance scoring
//! - Recency boosting
//! - Importance weighting
//! - Entity mention boosting
//! - Context-aware scoring
//!
//! Supports pluggable reranking strategies with a default heuristic-based
//! approach and optional integration with cross-encoder models.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::types::{Memory, MemoryType, SearchResult};

/// Configuration for the reranker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankConfig {
    /// Enable reranking
    pub enabled: bool,
    /// Reranking strategy to use
    pub strategy: RerankStrategy,
    /// Weight for original search score (0.0 - 1.0)
    pub original_score_weight: f32,
    /// Weight for rerank score (0.0 - 1.0)
    pub rerank_score_weight: f32,
    /// Boost for recent memories (per day, decays exponentially)
    pub recency_boost: f32,
    /// Half-life for recency boost in days
    pub recency_half_life_days: f32,
    /// Boost per importance point
    pub importance_boost: f32,
    /// Boost for memories with matching entities
    pub entity_match_boost: f32,
    /// Boost for exact phrase matches
    pub exact_match_boost: f32,
    /// Minimum number of results to consider for reranking
    pub min_results: usize,
    /// Maximum number of results to rerank (for performance)
    pub max_rerank_candidates: usize,
}

impl Default for RerankConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: RerankStrategy::Heuristic,
            original_score_weight: 0.6,
            rerank_score_weight: 0.4,
            recency_boost: 0.05,
            recency_half_life_days: 30.0,
            importance_boost: 0.1,
            entity_match_boost: 0.15,
            exact_match_boost: 0.2,
            min_results: 3,
            max_rerank_candidates: 100,
        }
    }
}

/// Reranking strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RerankStrategy {
    /// No reranking, keep original order
    None,
    /// Heuristic-based reranking using query features
    #[default]
    Heuristic,
    /// Cross-encoder model (requires external API or local model)
    CrossEncoder,
    /// Reciprocal Rank Fusion with multiple signals
    MultiSignal,
}

/// Reranking result with explanation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankResult {
    /// Original search result
    pub result: SearchResult,
    /// Original rank (1-indexed)
    pub original_rank: usize,
    /// New rank after reranking (1-indexed)
    pub new_rank: usize,
    /// Rerank score details
    pub rerank_info: RerankInfo,
}

/// Detailed reranking information for explainability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RerankInfo {
    /// Original search score
    pub original_score: f32,
    /// Final combined score
    pub final_score: f32,
    /// Rerank score before combination
    pub rerank_score: f32,
    /// Individual score components
    pub components: RerankComponents,
}

/// Individual components of the rerank score
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RerankComponents {
    /// Score from term overlap
    pub term_overlap: f32,
    /// Score from recency
    pub recency: f32,
    /// Score from memory importance
    pub importance: f32,
    /// Score from entity matches
    pub entity_match: f32,
    /// Score from exact phrase match
    pub exact_match: f32,
    /// Score from memory type relevance
    pub type_relevance: f32,
    /// Score from tag matches
    pub tag_match: f32,
}

/// Reranker for search results
pub struct Reranker {
    config: RerankConfig,
}

impl Reranker {
    /// Create a new reranker with default config
    pub fn new() -> Self {
        Self {
            config: RerankConfig::default(),
        }
    }

    /// Create a new reranker with custom config
    pub fn with_config(config: RerankConfig) -> Self {
        Self { config }
    }

    /// Rerank search results
    pub fn rerank(
        &self,
        results: Vec<SearchResult>,
        query: &str,
        query_entities: Option<&[String]>,
    ) -> Vec<RerankResult> {
        if !self.config.enabled || results.len() < self.config.min_results {
            // Return results unchanged but with rerank info
            return results
                .into_iter()
                .enumerate()
                .map(|(i, r)| RerankResult {
                    rerank_info: RerankInfo {
                        original_score: r.score,
                        final_score: r.score,
                        rerank_score: 0.0,
                        components: RerankComponents::default(),
                    },
                    result: r,
                    original_rank: i + 1,
                    new_rank: i + 1,
                })
                .collect();
        }

        match self.config.strategy {
            RerankStrategy::None => self.no_rerank(results),
            RerankStrategy::Heuristic => self.heuristic_rerank(results, query, query_entities),
            RerankStrategy::CrossEncoder => {
                // Cross-encoder requires external model, fallback to heuristic
                self.heuristic_rerank(results, query, query_entities)
            }
            RerankStrategy::MultiSignal => self.multi_signal_rerank(results, query, query_entities),
        }
    }

    /// No reranking - just wrap results
    fn no_rerank(&self, results: Vec<SearchResult>) -> Vec<RerankResult> {
        results
            .into_iter()
            .enumerate()
            .map(|(i, r)| RerankResult {
                rerank_info: RerankInfo {
                    original_score: r.score,
                    final_score: r.score,
                    rerank_score: 0.0,
                    components: RerankComponents::default(),
                },
                result: r,
                original_rank: i + 1,
                new_rank: i + 1,
            })
            .collect()
    }

    /// Heuristic-based reranking
    fn heuristic_rerank(
        &self,
        results: Vec<SearchResult>,
        query: &str,
        query_entities: Option<&[String]>,
    ) -> Vec<RerankResult> {
        let query_terms = extract_terms(query);
        let query_lower = query.to_lowercase();

        let mut rerank_results: Vec<RerankResult> = results
            .into_iter()
            .enumerate()
            .take(self.config.max_rerank_candidates)
            .map(|(i, r)| {
                let components = self.compute_rerank_components(
                    &r.memory,
                    &query_terms,
                    &query_lower,
                    query_entities,
                );

                let rerank_score = self.combine_components(&components);
                let final_score = self.config.original_score_weight * r.score
                    + self.config.rerank_score_weight * rerank_score;

                RerankResult {
                    rerank_info: RerankInfo {
                        original_score: r.score,
                        final_score,
                        rerank_score,
                        components,
                    },
                    result: r,
                    original_rank: i + 1,
                    new_rank: 0, // Will be set after sorting
                }
            })
            .collect();

        // Sort by final score
        rerank_results.sort_by(|a, b| {
            b.rerank_info
                .final_score
                .partial_cmp(&a.rerank_info.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Update new ranks
        for (i, result) in rerank_results.iter_mut().enumerate() {
            result.new_rank = i + 1;
        }

        rerank_results
    }

    /// Multi-signal reranking using RRF across multiple signals
    fn multi_signal_rerank(
        &self,
        results: Vec<SearchResult>,
        query: &str,
        query_entities: Option<&[String]>,
    ) -> Vec<RerankResult> {
        let query_terms = extract_terms(query);
        let query_lower = query.to_lowercase();

        // Compute multiple rankings
        let mut original_ranks: Vec<(usize, f32)> = results
            .iter()
            .enumerate()
            .map(|(i, r)| (i, r.score))
            .collect();
        original_ranks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut recency_ranks: Vec<(usize, f32)> = results
            .iter()
            .enumerate()
            .map(|(i, r)| (i, self.compute_recency_score(&r.memory)))
            .collect();
        recency_ranks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut term_ranks: Vec<(usize, f32)> = results
            .iter()
            .enumerate()
            .map(|(i, r)| (i, compute_term_overlap(&r.memory.content, &query_terms)))
            .collect();
        term_ranks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // RRF fusion
        let k = 60.0;
        let mut rrf_scores: Vec<(usize, f32)> = vec![];

        for i in 0..results.len() {
            let orig_rank = original_ranks
                .iter()
                .position(|(idx, _)| *idx == i)
                .unwrap()
                + 1;
            let rec_rank = recency_ranks.iter().position(|(idx, _)| *idx == i).unwrap() + 1;
            let term_rank = term_ranks.iter().position(|(idx, _)| *idx == i).unwrap() + 1;

            let rrf_score = 1.0 / (k + orig_rank as f32)
                + 0.5 / (k + rec_rank as f32)
                + 0.5 / (k + term_rank as f32);

            rrf_scores.push((i, rrf_score));
        }

        rrf_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Build results
        let mut rerank_results: Vec<RerankResult> = results
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let components = self.compute_rerank_components(
                    &r.memory,
                    &query_terms,
                    &query_lower,
                    query_entities,
                );
                let rrf_score = rrf_scores
                    .iter()
                    .find(|(idx, _)| *idx == i)
                    .map(|(_, s)| *s)
                    .unwrap_or(0.0);
                let new_rank = rrf_scores
                    .iter()
                    .position(|(idx, _)| *idx == i)
                    .unwrap_or(i)
                    + 1;

                RerankResult {
                    rerank_info: RerankInfo {
                        original_score: r.score,
                        final_score: rrf_score,
                        rerank_score: rrf_score,
                        components,
                    },
                    result: r,
                    original_rank: i + 1,
                    new_rank,
                }
            })
            .collect();

        rerank_results.sort_by_key(|r| r.new_rank);
        rerank_results
    }

    /// Compute reranking score components for a memory
    fn compute_rerank_components(
        &self,
        memory: &Memory,
        query_terms: &HashSet<String>,
        query_lower: &str,
        query_entities: Option<&[String]>,
    ) -> RerankComponents {
        let content_lower = memory.content.to_lowercase();

        RerankComponents {
            term_overlap: compute_term_overlap(&memory.content, query_terms),
            recency: self.compute_recency_score(memory),
            importance: memory.importance * self.config.importance_boost,
            entity_match: self.compute_entity_match_score(memory, query_entities),
            exact_match: if content_lower.contains(query_lower) {
                self.config.exact_match_boost
            } else {
                0.0
            },
            type_relevance: self.compute_type_relevance(memory),
            tag_match: self.compute_tag_match_score(memory, query_terms),
        }
    }

    /// Combine component scores into a single rerank score
    fn combine_components(&self, components: &RerankComponents) -> f32 {
        // Weighted combination of components
        components.term_overlap * 0.25
            + components.recency * 0.15
            + components.importance * 0.15
            + components.entity_match * 0.15
            + components.exact_match * 0.15
            + components.type_relevance * 0.05
            + components.tag_match * 0.10
    }

    /// Compute recency score with exponential decay
    fn compute_recency_score(&self, memory: &Memory) -> f32 {
        let now = chrono::Utc::now();
        let age_days = (now - memory.created_at).num_days() as f32;

        // Exponential decay: score = boost * 0.5^(age/half_life)
        let decay = 0.5_f32.powf(age_days / self.config.recency_half_life_days);
        self.config.recency_boost * decay
    }

    /// Compute entity match score
    fn compute_entity_match_score(
        &self,
        memory: &Memory,
        query_entities: Option<&[String]>,
    ) -> f32 {
        let Some(entities) = query_entities else {
            return 0.0;
        };

        if entities.is_empty() {
            return 0.0;
        }

        let content_lower = memory.content.to_lowercase();
        let matches = entities
            .iter()
            .filter(|e| content_lower.contains(&e.to_lowercase()))
            .count();

        if matches > 0 {
            self.config.entity_match_boost * (matches as f32 / entities.len() as f32)
        } else {
            0.0
        }
    }

    /// Compute type relevance (some types are generally more relevant)
    fn compute_type_relevance(&self, memory: &Memory) -> f32 {
        match memory.memory_type {
            MemoryType::Decision => 0.1,
            MemoryType::Preference => 0.08,
            MemoryType::Learning => 0.06,
            MemoryType::Context => 0.05,
            MemoryType::Note => 0.04,
            MemoryType::Todo => 0.03,
            MemoryType::Issue => 0.03,
            MemoryType::Credential => 0.02,
            MemoryType::Custom => 0.04,
            MemoryType::TranscriptChunk => 0.02, // Lower relevance for transcript chunks
        }
    }

    /// Compute tag match score
    fn compute_tag_match_score(&self, memory: &Memory, query_terms: &HashSet<String>) -> f32 {
        if memory.tags.is_empty() || query_terms.is_empty() {
            return 0.0;
        }

        let tag_set: HashSet<String> = memory.tags.iter().map(|t| t.to_lowercase()).collect();
        let matches = query_terms.intersection(&tag_set).count();

        if matches > 0 {
            0.1 * (matches as f32 / query_terms.len().min(memory.tags.len()) as f32)
        } else {
            0.0
        }
    }
}

impl Default for Reranker {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract normalized terms from text
fn extract_terms(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 2)
        .map(|s| s.to_string())
        .collect()
}

/// Compute term overlap score between content and query terms
fn compute_term_overlap(content: &str, query_terms: &HashSet<String>) -> f32 {
    if query_terms.is_empty() {
        return 0.0;
    }

    let content_terms = extract_terms(content);
    let matches = query_terms.intersection(&content_terms).count();

    matches as f32 / query_terms.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MatchInfo, MemoryScope, SearchStrategy, Visibility};
    use chrono::Utc;
    use std::collections::HashMap;

    fn create_test_memory(content: &str, importance: f32) -> Memory {
        Memory {
            id: 1,
            content: content.to_string(),
            memory_type: MemoryType::Note,
            importance,
            tags: vec![],
            access_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_accessed_at: None,
            owner_id: None,
            visibility: Visibility::Private,
            version: 1,
            has_embedding: false,
            metadata: HashMap::new(),
            scope: MemoryScope::Global,
            workspace: "default".to_string(),
            tier: crate::types::MemoryTier::Permanent,
            expires_at: None,
            content_hash: None,
        }
    }

    fn create_test_result(memory: Memory, score: f32) -> SearchResult {
        SearchResult {
            memory,
            score,
            match_info: MatchInfo {
                strategy: SearchStrategy::Hybrid,
                matched_terms: vec![],
                highlights: vec![],
                semantic_score: None,
                keyword_score: Some(score),
            },
        }
    }

    #[test]
    fn test_reranker_preserves_order_when_disabled() {
        let config = RerankConfig {
            enabled: false,
            ..Default::default()
        };
        let reranker = Reranker::with_config(config);

        let results = vec![
            create_test_result(create_test_memory("First result", 0.5), 0.9),
            create_test_result(create_test_memory("Second result", 0.5), 0.8),
            create_test_result(create_test_memory("Third result", 0.5), 0.7),
        ];

        let reranked = reranker.rerank(results, "test query", None);

        assert_eq!(reranked[0].new_rank, 1);
        assert_eq!(reranked[1].new_rank, 2);
        assert_eq!(reranked[2].new_rank, 3);
    }

    #[test]
    fn test_exact_match_boost() {
        let reranker = Reranker::new();

        let results = vec![
            create_test_result(create_test_memory("Some unrelated content", 0.5), 0.9),
            create_test_result(
                create_test_memory("This contains test query exactly", 0.5),
                0.7,
            ),
            create_test_result(create_test_memory("Another unrelated text", 0.5), 0.8),
        ];

        let reranked = reranker.rerank(results, "test query", None);

        // The result with exact match should be boosted
        let exact_match_result = reranked
            .iter()
            .find(|r| r.result.memory.content.contains("test query"))
            .unwrap();
        assert!(exact_match_result.rerank_info.components.exact_match > 0.0);
    }

    #[test]
    fn test_importance_boost() {
        let config = RerankConfig {
            min_results: 2, // Allow testing with 2 results
            ..Default::default()
        };
        let reranker = Reranker::with_config(config);

        let mut low_importance = create_test_memory("Test content low", 0.2);
        let mut high_importance = create_test_memory("Test content high", 0.9);

        low_importance.id = 1;
        high_importance.id = 2;

        let results = vec![
            create_test_result(low_importance, 0.8),
            create_test_result(high_importance, 0.75),
        ];

        let reranked = reranker.rerank(results, "test", None);

        // High importance memory should have higher importance component
        let high_result = reranked.iter().find(|r| r.result.memory.id == 2).unwrap();
        let low_result = reranked.iter().find(|r| r.result.memory.id == 1).unwrap();

        assert!(
            high_result.rerank_info.components.importance
                > low_result.rerank_info.components.importance
        );
    }

    #[test]
    fn test_entity_match_boost() {
        let config = RerankConfig {
            min_results: 2, // Allow testing with 2 results
            ..Default::default()
        };
        let reranker = Reranker::with_config(config);

        let results = vec![
            create_test_result(
                create_test_memory("Content about Python programming", 0.5),
                0.8,
            ),
            create_test_result(
                create_test_memory("Content about Rust and systems", 0.5),
                0.75,
            ),
        ];

        let entities = vec!["Rust".to_string(), "systems".to_string()];
        let reranked = reranker.rerank(results, "programming language", Some(&entities));

        // Result mentioning entities should have entity_match boost
        let rust_result = reranked
            .iter()
            .find(|r| r.result.memory.content.contains("Rust"))
            .unwrap();
        assert!(rust_result.rerank_info.components.entity_match > 0.0);
    }

    #[test]
    fn test_term_overlap() {
        let terms: HashSet<String> = ["rust", "programming", "memory"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let high_overlap = compute_term_overlap("Rust programming with memory management", &terms);
        let low_overlap = compute_term_overlap("Python web development", &terms);

        assert!(high_overlap > low_overlap);
        assert!(high_overlap > 0.5); // At least 2 of 3 terms match
    }

    #[test]
    fn test_multi_signal_rerank() {
        let config = RerankConfig {
            strategy: RerankStrategy::MultiSignal,
            ..Default::default()
        };
        let reranker = Reranker::with_config(config);

        let results = vec![
            create_test_result(create_test_memory("First memory", 0.5), 0.9),
            create_test_result(create_test_memory("Second memory", 0.5), 0.8),
            create_test_result(
                create_test_memory("Third memory with exact query", 0.5),
                0.7,
            ),
        ];

        let reranked = reranker.rerank(results, "exact query", None);

        // Results should be reranked
        assert_eq!(reranked.len(), 3);
        // All should have rerank info
        for r in &reranked {
            assert!(r.rerank_info.final_score > 0.0);
        }
    }
}
