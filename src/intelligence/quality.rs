//! Memory Quality Scoring (RML-892)
//!
//! Automatically scores memory quality based on multiple factors.

use crate::types::Memory;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Quality metrics for a memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityMetrics {
    /// Content completeness (0.0 - 1.0)
    pub completeness: f32,
    /// Content clarity (0.0 - 1.0)
    pub clarity: f32,
    /// Relevance based on access patterns (0.0 - 1.0)
    pub relevance: f32,
    /// Freshness based on age and updates (0.0 - 1.0)
    pub freshness: f32,
    /// Connectivity in the knowledge graph (0.0 - 1.0)
    pub connectivity: f32,
    /// Consistency with other memories (0.0 - 1.0)
    pub consistency: f32,
}

impl Default for QualityMetrics {
    fn default() -> Self {
        Self {
            completeness: 0.5,
            clarity: 0.5,
            relevance: 0.5,
            freshness: 0.5,
            connectivity: 0.0,
            consistency: 0.5,
        }
    }
}

/// Overall quality score with breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityScore {
    /// Overall score (0.0 - 1.0)
    pub overall: f32,
    /// Letter grade (A, B, C, D, F)
    pub grade: char,
    /// Detailed metrics
    pub metrics: QualityMetrics,
    /// Suggestions for improvement
    pub suggestions: Vec<String>,
    /// When the score was calculated
    pub calculated_at: DateTime<Utc>,
}

impl QualityScore {
    /// Get grade from overall score
    fn grade_from_score(score: f32) -> char {
        match score {
            s if s >= 0.9 => 'A',
            s if s >= 0.8 => 'B',
            s if s >= 0.7 => 'C',
            s if s >= 0.6 => 'D',
            _ => 'F',
        }
    }
}

/// Configuration for quality scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityScorerConfig {
    /// Weight for completeness
    pub completeness_weight: f32,
    /// Weight for clarity
    pub clarity_weight: f32,
    /// Weight for relevance
    pub relevance_weight: f32,
    /// Weight for freshness
    pub freshness_weight: f32,
    /// Weight for connectivity
    pub connectivity_weight: f32,
    /// Weight for consistency
    pub consistency_weight: f32,
    /// Minimum content length for good completeness
    pub min_content_length: usize,
    /// Ideal content length
    pub ideal_content_length: usize,
    /// Days until memory is considered stale
    pub staleness_days: i64,
}

impl Default for QualityScorerConfig {
    fn default() -> Self {
        Self {
            completeness_weight: 0.2,
            clarity_weight: 0.2,
            relevance_weight: 0.2,
            freshness_weight: 0.15,
            connectivity_weight: 0.15,
            consistency_weight: 0.1,
            min_content_length: 20,
            ideal_content_length: 200,
            staleness_days: 90,
        }
    }
}

/// Engine for scoring memory quality
pub struct QualityScorer {
    config: QualityScorerConfig,
}

impl Default for QualityScorer {
    fn default() -> Self {
        Self::new(QualityScorerConfig::default())
    }
}

impl QualityScorer {
    /// Create a new quality scorer
    pub fn new(config: QualityScorerConfig) -> Self {
        Self { config }
    }

    /// Score a memory's quality
    pub fn score(&self, memory: &Memory, connection_count: usize) -> QualityScore {
        let metrics = QualityMetrics {
            completeness: self.score_completeness(memory),
            clarity: self.score_clarity(memory),
            relevance: self.score_relevance(memory),
            freshness: self.score_freshness(memory),
            connectivity: self.score_connectivity(connection_count),
            consistency: 0.5, // Would need cross-memory analysis
        };

        let overall = self.calculate_overall(&metrics);
        let suggestions = self.generate_suggestions(memory, &metrics);

        QualityScore {
            overall,
            grade: QualityScore::grade_from_score(overall),
            metrics,
            suggestions,
            calculated_at: Utc::now(),
        }
    }

    /// Score content completeness
    fn score_completeness(&self, memory: &Memory) -> f32 {
        let len = memory.content.len();

        // Too short is bad
        if len < self.config.min_content_length {
            return 0.3;
        }

        // Ideal length gets full score
        if len >= self.config.ideal_content_length {
            return 1.0;
        }

        // Linear interpolation between min and ideal
        let range = (self.config.ideal_content_length - self.config.min_content_length) as f32;
        let progress = (len - self.config.min_content_length) as f32;
        0.3 + 0.7 * (progress / range)
    }

    /// Score content clarity
    fn score_clarity(&self, memory: &Memory) -> f32 {
        let content = &memory.content;
        let mut score: f32 = 0.5;

        // Has structure (sentences)
        let sentence_count = content.matches('.').count()
            + content.matches('!').count()
            + content.matches('?').count();
        if sentence_count > 0 {
            score += 0.15;
        }

        // Not too many abbreviations or unclear terms
        let word_count = content.split_whitespace().count();
        if word_count > 0 {
            let avg_word_len: f32 = content
                .split_whitespace()
                .map(|w| w.len() as f32)
                .sum::<f32>()
                / word_count as f32;

            // Words between 3-10 chars are typically clear
            if (3.0..=10.0).contains(&avg_word_len) {
                score += 0.2;
            }
        }

        // Has tags (organization)
        if !memory.tags.is_empty() {
            score += 0.15;
        }

        score.min(1.0_f32)
    }

    /// Score relevance based on access patterns
    fn score_relevance(&self, memory: &Memory) -> f32 {
        // Base on access count and recency of access
        let access_score = (memory.access_count as f32 / 50.0).min(1.0);

        let recency_score = memory
            .last_accessed_at
            .map(|dt| {
                let days_ago = (Utc::now() - dt).num_days() as f32;
                (1.0 - days_ago / 30.0).max(0.0)
            })
            .unwrap_or(0.3);

        (access_score * 0.6 + recency_score * 0.4).min(1.0)
    }

    /// Score freshness
    fn score_freshness(&self, memory: &Memory) -> f32 {
        let age_days = (Utc::now() - memory.updated_at).num_days() as f32;
        let staleness = self.config.staleness_days as f32;

        if age_days <= 0.0 {
            1.0
        } else if age_days >= staleness {
            0.2
        } else {
            1.0 - 0.8 * (age_days / staleness)
        }
    }

    /// Score connectivity
    fn score_connectivity(&self, connection_count: usize) -> f32 {
        // Having connections is good, but diminishing returns
        match connection_count {
            0 => 0.2,
            1..=2 => 0.5,
            3..=5 => 0.8,
            _ => 1.0,
        }
    }

    /// Calculate overall score from metrics
    fn calculate_overall(&self, metrics: &QualityMetrics) -> f32 {
        let c = &self.config;
        metrics.completeness * c.completeness_weight
            + metrics.clarity * c.clarity_weight
            + metrics.relevance * c.relevance_weight
            + metrics.freshness * c.freshness_weight
            + metrics.connectivity * c.connectivity_weight
            + metrics.consistency * c.consistency_weight
    }

    /// Generate improvement suggestions
    fn generate_suggestions(&self, memory: &Memory, metrics: &QualityMetrics) -> Vec<String> {
        let mut suggestions = Vec::new();

        if metrics.completeness < 0.5 {
            suggestions.push("Add more detail to make this memory more useful".to_string());
        }

        if metrics.clarity < 0.5 {
            suggestions.push("Consider adding structure with clear sentences".to_string());
        }

        if memory.tags.is_empty() {
            suggestions.push("Add tags to improve organization and searchability".to_string());
        }

        if metrics.freshness < 0.3 {
            suggestions.push("This memory may be outdated - consider reviewing".to_string());
        }

        if metrics.connectivity < 0.3 {
            suggestions.push("Link this to related memories to build connections".to_string());
        }

        if metrics.relevance < 0.3 && memory.access_count == 0 {
            suggestions
                .push("This memory has never been accessed - is it still relevant?".to_string());
        }

        suggestions
    }

    /// Score multiple memories and return sorted by quality
    pub fn score_batch(&self, memories: &[Memory]) -> Vec<(Memory, QualityScore)> {
        let mut scored: Vec<_> = memories
            .iter()
            .map(|m| (m.clone(), self.score(m, 0)))
            .collect();

        scored.sort_by(|a, b| {
            b.1.overall
                .partial_cmp(&a.1.overall)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MemoryType, Visibility};
    use std::collections::HashMap;

    fn create_test_memory(content: &str, tags: Vec<&str>, access_count: i32) -> Memory {
        Memory {
            id: 1,
            content: content.to_string(),
            memory_type: MemoryType::Note,
            tags: tags.into_iter().map(String::from).collect(),
            metadata: HashMap::new(),
            importance: 0.5,
            access_count,
            created_at: Utc::now() - chrono::Duration::days(10),
            updated_at: Utc::now() - chrono::Duration::days(5),
            last_accessed_at: Some(Utc::now() - chrono::Duration::days(1)),
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
    fn test_score_completeness() {
        let scorer = QualityScorer::default();

        let short = create_test_memory("Hi", vec![], 0);
        let medium = create_test_memory(
            "This is a medium length note with some useful content.",
            vec![],
            0,
        );
        let long = create_test_memory(&"This is a detailed note. ".repeat(20), vec![], 0);

        let short_score = scorer.score_completeness(&short);
        let medium_score = scorer.score_completeness(&medium);
        let long_score = scorer.score_completeness(&long);

        assert!(short_score < medium_score);
        assert!(medium_score < long_score);
    }

    #[test]
    fn test_quality_grade() {
        assert_eq!(QualityScore::grade_from_score(0.95), 'A');
        assert_eq!(QualityScore::grade_from_score(0.85), 'B');
        assert_eq!(QualityScore::grade_from_score(0.75), 'C');
        assert_eq!(QualityScore::grade_from_score(0.65), 'D');
        assert_eq!(QualityScore::grade_from_score(0.5), 'F');
    }

    #[test]
    fn test_suggestions_generation() {
        let scorer = QualityScorer::default();

        let poor_memory = create_test_memory("X", vec![], 0);
        let score = scorer.score(&poor_memory, 0);

        assert!(!score.suggestions.is_empty());
        assert!(score.suggestions.iter().any(|s| s.contains("detail")));
        assert!(score.suggestions.iter().any(|s| s.contains("tags")));
    }

    #[test]
    fn test_overall_score() {
        let scorer = QualityScorer::default();

        let good_memory = create_test_memory(
            "This is a well-written note about an important topic. It has good structure and clear sentences. The content is detailed enough to be useful.",
            vec!["important", "well-written"],
            20,
        );

        let score = scorer.score(&good_memory, 3);
        assert!(score.overall > 0.6);
        assert!(score.grade == 'A' || score.grade == 'B' || score.grade == 'C');
    }
}
