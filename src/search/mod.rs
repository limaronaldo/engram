//! Search functionality for Engram
//!
//! Implements:
//! - BM25 full-text search (RML-876)
//! - Fuzzy/typo-tolerant search (RML-877)
//! - Search result explanation (RML-878)
//! - Adaptive search strategy (RML-898)
//! - Hybrid search with RRF
//! - Aggregation queries (RML-880)
//! - Search result reranking (RML-927)

mod aggregation;
mod bm25;
mod fuzzy;
mod hybrid;
mod metadata;
mod rerank;

pub use aggregation::*;
pub use bm25::*;
pub use fuzzy::*;
pub use hybrid::*;
pub use metadata::*;
pub use rerank::*;

use crate::types::SearchStrategy;

/// Analyze query to determine optimal search strategy (RML-898)
pub fn select_search_strategy(query: &str) -> SearchStrategy {
    let query = query.trim();
    let word_count = query.split_whitespace().count();
    let has_quotes = query.contains('"');
    let has_operators = query.contains(':')
        || query.contains(" AND ")
        || query.contains(" OR ")
        || query.contains(" NOT ");
    let has_special = query.contains('*') || query.contains('?');

    // Explicit search syntax → keyword only
    if has_quotes || has_operators || has_special {
        return SearchStrategy::KeywordOnly;
    }

    // Very short queries → keyword (faster, usually precise enough)
    if word_count <= 2 {
        return SearchStrategy::KeywordOnly;
    }

    // Long conceptual queries → semantic
    if word_count >= 8 {
        return SearchStrategy::SemanticOnly;
    }

    // Default → hybrid
    SearchStrategy::Hybrid
}

/// Strategy for deduplicating search results across result sets
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DedupeStrategy {
    /// Deduplicate by memory ID (default, fastest)
    #[default]
    ById,
    /// Deduplicate by content hash (catches duplicates with different IDs)
    ByContentHash,
}

/// Configuration for search thresholds
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// Word count threshold for short queries (keyword-only)
    pub short_threshold: usize,
    /// Word count threshold for long queries (semantic-only)
    pub long_threshold: usize,
    /// Minimum score to include in results
    pub min_score: f32,
    /// Weight for keyword score in hybrid search
    pub keyword_weight: f32,
    /// Weight for semantic score in hybrid search
    pub semantic_weight: f32,
    /// RRF constant (k parameter, default: 60)
    /// Higher values favor lower-ranked results, lower values favor top results
    pub rrf_k: f32,
    /// Boost factor for project context memories when metadata.project_path matches cwd
    pub project_context_boost: f32,
    /// Current working directory for project context matching
    pub project_context_path: Option<String>,
    /// Deduplication strategy for hybrid search
    pub dedupe_strategy: DedupeStrategy,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            short_threshold: 2,
            long_threshold: 8,
            min_score: 0.1,
            keyword_weight: 0.4,
            semantic_weight: 0.6,
            rrf_k: 60.0,
            project_context_boost: 0.2,
            project_context_path: None,
            dedupe_strategy: DedupeStrategy::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_selection() {
        // Short queries → keyword
        assert_eq!(select_search_strategy("auth"), SearchStrategy::KeywordOnly);
        assert_eq!(
            select_search_strategy("jwt token"),
            SearchStrategy::KeywordOnly
        );

        // Quoted → keyword
        assert_eq!(
            select_search_strategy("\"exact phrase\""),
            SearchStrategy::KeywordOnly
        );

        // Operators → keyword
        assert_eq!(
            select_search_strategy("auth AND jwt"),
            SearchStrategy::KeywordOnly
        );

        // Medium → hybrid
        assert_eq!(
            select_search_strategy("how does authentication work"),
            SearchStrategy::Hybrid
        );

        // Long → semantic
        assert_eq!(
            select_search_strategy(
                "explain the authentication flow with jwt tokens and refresh mechanism"
            ),
            SearchStrategy::SemanticOnly
        );
    }
}
