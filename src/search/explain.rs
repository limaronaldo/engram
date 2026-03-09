//! Search result explainability (RML-1242)
//!
//! Provides human-readable explanations of why a search result ranked where it
//! did, including per-signal score breakdowns and contribution percentages.
//!
//! This module is purely computational — it performs no database access.

use serde::{Deserialize, Serialize};

/// Breakdown of individual scoring signals for a search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    /// BM25 keyword relevance score (0.0–1.0)
    pub bm25_score: f32,
    /// Vector / semantic similarity score (0.0–1.0)
    pub vector_score: f32,
    /// Fuzzy match score (0.0–1.0)
    pub fuzzy_score: f32,
    /// Recency boost factor applied during reranking
    pub recency_boost: f32,
    /// Importance weight derived from `memory.importance`
    pub importance_weight: f32,
    /// Cross-encoder reranking score (`None` when the reranker is not active)
    pub rerank_score: Option<f32>,
    /// Final combined score after RRF / reranking
    pub final_score: f32,
}

/// A named signal and its contribution to the final score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalContribution {
    /// Signal name, e.g. `"semantic similarity"`.
    pub signal: String,
    /// Raw score for this signal.
    pub score: f32,
    /// Percentage contribution to the final score (0–100).
    pub contribution_pct: f32,
}

/// Human-readable explanation of why a result ranked where it did.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchExplanation {
    /// Memory ID of the explained result.
    pub memory_id: i64,
    /// 1-based rank position.
    pub rank: usize,
    /// Per-signal score breakdown.
    pub scores: ScoreBreakdown,
    /// Human-readable explanation text.
    pub explanation: String,
    /// Signals sorted by contribution percentage (descending).
    pub top_signals: Vec<SignalContribution>,
}

/// Generates [`SearchExplanation`] values for search results.
pub struct SearchExplainer {
    /// RRF *k* parameter (should match `SearchConfig::rrf_k`).
    pub rrf_k: f32,
    /// Whether a cross-encoder reranker was active during this search.
    pub reranking_active: bool,
}

impl SearchExplainer {
    /// Create a new explainer.
    pub fn new(rrf_k: f32, reranking_active: bool) -> Self {
        Self {
            rrf_k,
            reranking_active,
        }
    }

    /// Explain a single search result.
    ///
    /// # Parameters
    /// * `memory_id` — ID of the memory being explained.
    /// * `rank` — 1-based rank position in the result set.
    /// * `bm25` — BM25 keyword relevance score.
    /// * `vector` — Vector/semantic similarity score.
    /// * `fuzzy` — Fuzzy match score.
    /// * `recency` — Recency boost applied.
    /// * `importance` — Importance weight.
    /// * `rerank` — Cross-encoder score (`None` if reranker inactive).
    /// * `final_score` — Final combined score after fusion/reranking.
    #[allow(clippy::too_many_arguments)]
    pub fn explain_result(
        &self,
        memory_id: i64,
        rank: usize,
        bm25: f32,
        vector: f32,
        fuzzy: f32,
        recency: f32,
        importance: f32,
        rerank: Option<f32>,
        final_score: f32,
    ) -> SearchExplanation {
        let scores = ScoreBreakdown {
            bm25_score: bm25,
            vector_score: vector,
            fuzzy_score: fuzzy,
            recency_boost: recency,
            importance_weight: importance,
            rerank_score: rerank,
            final_score,
        };

        let top_signals = self.compute_signal_contributions(&scores);
        let explanation = self.generate_explanation(rank, &scores, &top_signals);

        SearchExplanation {
            memory_id,
            rank,
            scores,
            explanation,
            top_signals,
        }
    }

    /// Explain all results in a batch, assigning ranks 1..N in the order given.
    ///
    /// Each tuple is `(memory_id, bm25, vector, fuzzy, recency, importance,
    /// rerank, final_score)`.
    pub fn explain_batch(
        &self,
        results: Vec<(i64, f32, f32, f32, f32, f32, Option<f32>, f32)>,
    ) -> Vec<SearchExplanation> {
        results
            .into_iter()
            .enumerate()
            .map(|(i, (memory_id, bm25, vector, fuzzy, recency, importance, rerank, final_score))| {
                self.explain_result(
                    memory_id,
                    i + 1,
                    bm25,
                    vector,
                    fuzzy,
                    recency,
                    importance,
                    rerank,
                    final_score,
                )
            })
            .collect()
    }

    /// Build the human-readable explanation string.
    pub fn generate_explanation(
        &self,
        rank: usize,
        scores: &ScoreBreakdown,
        signals: &[SignalContribution],
    ) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Lead: rank + final score
        parts.push(format!(
            "Ranked #{rank} (score: {:.2}).",
            scores.final_score
        ));

        // Primary signal
        if let Some(primary) = signals.first() {
            parts.push(format!(
                "Primary signal: {} ({:.0}%).",
                primary.signal, primary.contribution_pct
            ));
        }

        // Secondary signals (up to 3 more)
        for signal in signals.iter().skip(1).take(3) {
            if signal.contribution_pct >= 1.0 {
                // Only mention signals that meaningfully contributed
                let verb = match signal.signal.as_str() {
                    "BM25 keyword match" => "BM25 keyword match contributed",
                    "recency boost" => "Recency boost added",
                    "importance weight" => "Importance weight contributed",
                    "fuzzy match" => "Fuzzy match contributed",
                    _ => "contributed",
                };
                parts.push(format!(
                    "{} {:.0}%.",
                    verb, signal.contribution_pct
                ));
            }
        }

        // Cross-encoder note
        if self.reranking_active && scores.rerank_score.is_some() {
            parts.push("Cross-encoder reranking confirmed relevance.".to_string());
        }

        parts.join(" ")
    }

    // ------------------------------------------------------------------ //
    // Private helpers                                                      //
    // ------------------------------------------------------------------ //

    /// Map raw signal scores to [`SignalContribution`] sorted by contribution.
    fn compute_signal_contributions(&self, scores: &ScoreBreakdown) -> Vec<SignalContribution> {
        let mut raw: Vec<(&str, f32)> = vec![
            ("semantic similarity", scores.vector_score),
            ("BM25 keyword match", scores.bm25_score),
            ("fuzzy match", scores.fuzzy_score),
            ("recency boost", scores.recency_boost),
            ("importance weight", scores.importance_weight),
        ];

        // Include cross-encoder only when the reranker is active
        if self.reranking_active {
            if let Some(rs) = scores.rerank_score {
                raw.push(("cross-encoder reranking", rs));
            }
        }

        let total: f32 = raw.iter().map(|(_, s)| s).sum();

        let mut contributions: Vec<SignalContribution> = raw
            .into_iter()
            .map(|(name, score)| {
                let contribution_pct = if total > 0.0 {
                    (score / total) * 100.0
                } else {
                    // Equal contribution when all scores are zero
                    0.0
                };
                SignalContribution {
                    signal: name.to_string(),
                    score,
                    contribution_pct,
                }
            })
            .collect();

        // Sort descending by contribution percentage
        contributions.sort_by(|a, b| {
            b.contribution_pct
                .partial_cmp(&a.contribution_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        contributions
    }
}

impl Default for SearchExplainer {
    fn default() -> Self {
        Self::new(60.0, false)
    }
}

// ------------------------------------------------------------------ //
// Tests                                                                //
// ------------------------------------------------------------------ //

#[cfg(test)]
mod tests {
    use super::*;

    fn make_explainer() -> SearchExplainer {
        SearchExplainer::new(60.0, true)
    }

    // Helper: produce a deterministic explanation for most tests.
    fn default_explanation(explainer: &SearchExplainer) -> SearchExplanation {
        explainer.explain_result(
            42,  // memory_id
            1,   // rank
            0.5, // bm25
            0.8, // vector
            0.3, // fuzzy
            0.1, // recency
            0.6, // importance
            Some(0.7), // rerank
            0.85, // final_score
        )
    }

    // ------------------------------------------------------------------ //
    // Test 1: Single result has all required fields populated             //
    // ------------------------------------------------------------------ //
    #[test]
    fn test_single_result_has_all_fields() {
        let explainer = make_explainer();
        let exp = default_explanation(&explainer);

        assert_eq!(exp.memory_id, 42);
        assert_eq!(exp.rank, 1);
        assert!((exp.scores.final_score - 0.85).abs() < f32::EPSILON);
        assert!(!exp.explanation.is_empty());
        assert!(!exp.top_signals.is_empty());
    }

    // ------------------------------------------------------------------ //
    // Test 2: Top signals are sorted by contribution (descending)        //
    // ------------------------------------------------------------------ //
    #[test]
    fn test_top_signals_sorted_descending() {
        let explainer = make_explainer();
        let exp = default_explanation(&explainer);

        for window in exp.top_signals.windows(2) {
            assert!(
                window[0].contribution_pct >= window[1].contribution_pct,
                "signals not sorted: {} ({:.2}%) before {} ({:.2}%)",
                window[0].signal,
                window[0].contribution_pct,
                window[1].signal,
                window[1].contribution_pct
            );
        }
    }

    // ------------------------------------------------------------------ //
    // Test 3: Contribution percentages sum to ~100 %                     //
    // ------------------------------------------------------------------ //
    #[test]
    fn test_contribution_percentages_sum_to_100() {
        let explainer = make_explainer();
        let exp = default_explanation(&explainer);

        let total: f32 = exp.top_signals.iter().map(|s| s.contribution_pct).sum();
        assert!(
            (total - 100.0).abs() < 0.1,
            "percentages sum to {total:.2}, expected ~100"
        );
    }

    // ------------------------------------------------------------------ //
    // Test 4: Rerank score included when reranker is active              //
    // ------------------------------------------------------------------ //
    #[test]
    fn test_rerank_score_included_when_active() {
        let explainer = SearchExplainer::new(60.0, true);
        let exp = explainer.explain_result(1, 1, 0.4, 0.6, 0.2, 0.05, 0.5, Some(0.9), 0.75);

        assert!(
            exp.scores.rerank_score.is_some(),
            "rerank_score should be Some when active"
        );
        // Cross-encoder signal must appear in top_signals
        assert!(
            exp.top_signals.iter().any(|s| s.signal == "cross-encoder reranking"),
            "cross-encoder signal missing from top_signals"
        );
    }

    // ------------------------------------------------------------------ //
    // Test 5: Rerank score is None when reranker is inactive             //
    // ------------------------------------------------------------------ //
    #[test]
    fn test_rerank_score_none_when_inactive() {
        let explainer = SearchExplainer::new(60.0, false);
        // Pass Some(0.9) as raw input but the explainer is inactive — the
        // stored rerank_score comes straight from the caller's value, but
        // the signal must NOT appear in top_signals.
        let exp = explainer.explain_result(1, 1, 0.4, 0.6, 0.2, 0.05, 0.5, None, 0.75);

        assert!(
            exp.scores.rerank_score.is_none(),
            "rerank_score should be None when inactive"
        );
        assert!(
            !exp.top_signals.iter().any(|s| s.signal == "cross-encoder reranking"),
            "cross-encoder signal must not appear when reranker is inactive"
        );
    }

    // ------------------------------------------------------------------ //
    // Test 6: Batch explanation assigns correct sequential ranks         //
    // ------------------------------------------------------------------ //
    #[test]
    fn test_batch_assigns_correct_ranks() {
        let explainer = SearchExplainer::new(60.0, false);
        let results = vec![
            (1_i64, 0.9_f32, 0.8_f32, 0.1_f32, 0.05_f32, 0.7_f32, None, 0.92_f32),
            (2_i64, 0.7_f32, 0.6_f32, 0.0_f32, 0.02_f32, 0.5_f32, None, 0.72_f32),
            (3_i64, 0.5_f32, 0.4_f32, 0.2_f32, 0.01_f32, 0.3_f32, None, 0.55_f32),
        ];

        let explanations = explainer.explain_batch(results);

        assert_eq!(explanations.len(), 3);
        for (i, exp) in explanations.iter().enumerate() {
            assert_eq!(exp.rank, i + 1, "rank mismatch at index {i}");
        }
        assert_eq!(explanations[0].memory_id, 1);
        assert_eq!(explanations[1].memory_id, 2);
        assert_eq!(explanations[2].memory_id, 3);
    }

    // ------------------------------------------------------------------ //
    // Test 7: Human-readable text contains rank and top signal name      //
    // ------------------------------------------------------------------ //
    #[test]
    fn test_explanation_text_contains_rank_and_top_signal() {
        let explainer = make_explainer();
        let exp = default_explanation(&explainer);

        assert!(
            exp.explanation.contains("#1"),
            "explanation should reference rank #1: {:?}",
            exp.explanation
        );

        let top_signal_name = &exp.top_signals[0].signal;
        assert!(
            exp.explanation.contains(top_signal_name.as_str()),
            "explanation should mention top signal '{top_signal_name}': {:?}",
            exp.explanation
        );
    }

    // ------------------------------------------------------------------ //
    // Test 8: Zero scores handled gracefully (no panic, 0% contributions) //
    // ------------------------------------------------------------------ //
    #[test]
    fn test_zero_scores_handled_gracefully() {
        let explainer = SearchExplainer::new(60.0, false);
        let exp = explainer.explain_result(99, 5, 0.0, 0.0, 0.0, 0.0, 0.0, None, 0.0);

        // Should not panic; all contributions are 0
        for signal in &exp.top_signals {
            assert!(
                (signal.contribution_pct - 0.0).abs() < f32::EPSILON,
                "expected 0% contribution, got {:.2}% for {}",
                signal.contribution_pct,
                signal.signal
            );
        }

        // Explanation should still be generated
        assert!(exp.explanation.contains("#5"));
    }

    // ------------------------------------------------------------------ //
    // Test 9: All signals at equal score → roughly equal contributions   //
    // ------------------------------------------------------------------ //
    #[test]
    fn test_equal_signals_have_roughly_equal_contributions() {
        let explainer = SearchExplainer::new(60.0, true);
        // Six signals each at 1.0 (5 base + 1 rerank)
        let exp = explainer.explain_result(7, 2, 1.0, 1.0, 1.0, 1.0, 1.0, Some(1.0), 1.0);

        let expected_pct = 100.0 / 6.0;
        for signal in &exp.top_signals {
            assert!(
                (signal.contribution_pct - expected_pct).abs() < 1.0,
                "signal '{}' has {:.2}%, expected ~{:.2}%",
                signal.signal,
                signal.contribution_pct,
                expected_pct
            );
        }
    }
}
