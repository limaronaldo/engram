//! Retrieval Utility Scoring — RML-1214
//!
//! MemRL-inspired Q-value / utility scoring. Memories accumulate utility
//! scores based on retrieval feedback. The Q-value update rule is:
//!
//! ```text
//! Q(m) = Q(m) + α * (reward - Q(m))
//!   where reward = 1.0  (was_useful = true)
//!               = -0.5  (was_useful = false)
//! ```
//!
//! Scores are temporally decayed between retrievals:
//!
//! ```text
//! Q_decayed(m) = Q(m) * decay_factor ^ days_since_last_retrieval
//! ```

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;

// ---------------------------------------------------------------------------
// DDL
// ---------------------------------------------------------------------------

/// SQL for creating the `utility_feedback` table and its index.
/// Safe to call on an existing database — uses `CREATE TABLE IF NOT EXISTS`.
pub const CREATE_UTILITY_FEEDBACK_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS utility_feedback (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id INTEGER NOT NULL,
    was_useful BOOLEAN NOT NULL,
    query     TEXT NOT NULL DEFAULT '',
    timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_utility_memory ON utility_feedback(memory_id);
CREATE INDEX IF NOT EXISTS idx_utility_timestamp ON utility_feedback(timestamp);
"#;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Hyper-parameters for the utility tracker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityConfig {
    /// Q-learning rate α (0 < α ≤ 1). Default: 0.1
    pub learning_rate: f64,
    /// Per-day temporal decay factor (0 < γ ≤ 1). Default: 0.95
    pub decay_factor: f64,
    /// Initial utility score for memories with no history. Default: 0.5
    pub initial_score: f64,
}

impl Default for UtilityConfig {
    fn default() -> Self {
        Self {
            learning_rate: 0.1,
            decay_factor: 0.95,
            initial_score: 0.5,
        }
    }
}

/// Computed utility summary for a single memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityScore {
    pub memory_id: i64,
    /// Current utility score (after temporal decay), in range `[0.0, 1.0]`.
    pub score: f64,
    /// Total number of retrieval feedback events recorded.
    pub retrievals: i64,
    /// Number of events where `was_useful = true`.
    pub useful_count: i64,
    /// RFC-3339 timestamp of the most recent retrieval event (empty if none).
    pub last_retrieved: String,
}

/// Aggregated utility statistics across all (or a filtered subset of) memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityStats {
    /// Total number of feedback rows recorded.
    pub total_feedback: i64,
    /// Average utility score across memories that have at least one feedback row.
    pub avg_score: f64,
    /// Top 10 most-useful memories: `(memory_id, useful_count)`.
    pub top_useful: Vec<(i64, i64)>,
    /// Bottom 10 least-useful memories: `(memory_id, useful_count)`.
    pub bottom_useful: Vec<(i64, i64)>,
}

// ---------------------------------------------------------------------------
// UtilityTracker
// ---------------------------------------------------------------------------

/// Tracks and updates Q-value utility scores for memories.
pub struct UtilityTracker {
    pub config: UtilityConfig,
}

impl UtilityTracker {
    /// Create a tracker with default configuration.
    pub fn new() -> Self {
        Self {
            config: UtilityConfig::default(),
        }
    }

    /// Create a tracker with custom configuration.
    pub fn with_config(config: UtilityConfig) -> Self {
        Self { config }
    }

    // -----------------------------------------------------------------------
    // Mutations
    // -----------------------------------------------------------------------

    /// Record a retrieval feedback event for `memory_id` and update its utility
    /// score in-place using the Q-learning update rule.
    ///
    /// This is a combined insert + score update: a new row is appended to
    /// `utility_feedback` and the running Q-value is recomputed from the full
    /// feedback history so that the score is always consistent with the table.
    pub fn record_retrieval(
        &self,
        conn: &Connection,
        memory_id: i64,
        was_useful: bool,
        query: &str,
    ) -> Result<()> {
        // Insert feedback row.
        conn.execute(
            "INSERT INTO utility_feedback (memory_id, was_useful, query) VALUES (?1, ?2, ?3)",
            rusqlite::params![memory_id, was_useful, query],
        )?;

        // Score is recomputed lazily from feedback history on get_utility(); no
        // separate score table is needed — the Q-value is derived from the log.
        // This keeps the schema minimal and the data consistent.
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Compute the current utility score for `memory_id` from its feedback history.
    ///
    /// The Q-value is replayed from the oldest event to the newest, applying
    /// the learning-rate update rule at each step. Temporal decay is then
    /// applied based on days elapsed since the most-recent retrieval.
    ///
    /// Returns the `initial_score` when there is no feedback history.
    pub fn get_utility(&self, conn: &Connection, memory_id: i64) -> Result<UtilityScore> {
        // Fetch all feedback rows in chronological order.
        let mut stmt = conn.prepare(
            "SELECT was_useful, timestamp FROM utility_feedback
             WHERE memory_id = ?1
             ORDER BY timestamp ASC, id ASC",
        )?;

        struct Row {
            was_useful: bool,
            timestamp: String,
        }

        let rows: Vec<Row> = stmt
            .query_map(rusqlite::params![memory_id], |r| {
                Ok(Row {
                    was_useful: r.get::<_, bool>(0)?,
                    timestamp: r.get::<_, String>(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        if rows.is_empty() {
            return Ok(UtilityScore {
                memory_id,
                score: self.config.initial_score,
                retrievals: 0,
                useful_count: 0,
                last_retrieved: String::new(),
            });
        }

        // Replay Q-value updates.
        let mut q = self.config.initial_score;
        let mut useful_count = 0_i64;

        for row in &rows {
            let reward = if row.was_useful { 1.0 } else { -0.5 };
            q += self.config.learning_rate * (reward - q);
            if row.was_useful {
                useful_count += 1;
            }
        }

        // Apply temporal decay.
        let last_retrieved = rows.last().map(|r| r.timestamp.clone()).unwrap_or_default();
        q = self.apply_decay(q, &last_retrieved);
        // Clamp to [0.0, 1.0] — reward can push it slightly above 1 or below 0.
        q = q.clamp(0.0, 1.0);

        Ok(UtilityScore {
            memory_id,
            score: q,
            retrievals: rows.len() as i64,
            useful_count,
            last_retrieved,
        })
    }

    // -----------------------------------------------------------------------
    // Boost application
    // -----------------------------------------------------------------------

    /// Multiply the search scores for each `(memory_id, score)` pair by that
    /// memory's utility score, clamped to `[0.5, 2.0]`.
    ///
    /// Memories with no feedback history receive a neutral multiplier of 1.0
    /// (derived from `initial_score = 0.5`, mapped to 1.0 in the boost formula).
    pub fn apply_utility_boost(&self, scores: &mut [(i64, f32)], conn: &Connection) -> Result<()> {
        for (memory_id, score) in scores.iter_mut() {
            let utility = self.get_utility(conn, *memory_id)?;
            // Map utility score [0, 1] → boost [0.5, 2.0] linearly.
            // utility = 0.5 (initial/neutral) → boost = 1.0
            // utility = 1.0 → boost = 2.0
            // utility = 0.0 → boost = 0.5
            let boost = (0.5 + utility.score * 1.5).clamp(0.5, 2.0);
            *score = (*score * boost as f32).clamp(0.5, 2.0);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Batch operations
    // -----------------------------------------------------------------------

    /// Apply temporal decay to all recorded utility scores.
    ///
    /// For every memory with at least one feedback event, the Q-value is
    /// recomputed (which includes decay). This function returns the count of
    /// memories whose effective score changed (decayed by at least 0.001).
    ///
    /// Because scores are always computed from the feedback log, this function
    /// does not need to write anything to the database. It is provided as a
    /// hook for callers that want to verify how many scores have drifted.
    pub fn batch_decay(&self, conn: &Connection, _config: &UtilityConfig) -> Result<usize> {
        // Collect distinct memory IDs that have feedback.
        let mut stmt =
            conn.prepare("SELECT DISTINCT memory_id FROM utility_feedback")?;
        let memory_ids: Vec<i64> = stmt
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut affected = 0_usize;
        for memory_id in memory_ids {
            let scored = self.get_utility(conn, memory_id)?;
            // Consider "affected" when decay moved the score from initial by
            // more than the threshold (≥ 0.001 change).
            if (scored.score - self.config.initial_score).abs() >= 0.001 {
                affected += 1;
            }
        }
        Ok(affected)
    }

    // -----------------------------------------------------------------------
    // Statistics
    // -----------------------------------------------------------------------

    /// Compute aggregated utility statistics.
    ///
    /// When `workspace` is `Some`, only memories that appear in the given
    /// workspace are considered (requires a `memories` table with a `workspace`
    /// column and an `id` column). Pass `None` to aggregate across all memories
    /// that have feedback.
    pub fn utility_stats(
        &self,
        conn: &Connection,
        workspace: Option<&str>,
    ) -> Result<UtilityStats> {
        // Determine the set of memory IDs to include.
        let memory_ids: Vec<i64> = if let Some(ws) = workspace {
            // Filter to memories belonging to this workspace.
            let mut stmt = conn.prepare(
                "SELECT DISTINCT uf.memory_id
                 FROM utility_feedback uf
                 INNER JOIN memories m ON m.id = uf.memory_id
                 WHERE m.workspace = ?1",
            )?;
            let ids = stmt
                .query_map(rusqlite::params![ws], |r| r.get::<_, i64>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids
        } else {
            let mut stmt =
                conn.prepare("SELECT DISTINCT memory_id FROM utility_feedback")?;
            let ids = stmt
                .query_map([], |r| r.get::<_, i64>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids
        };

        // Total feedback count.
        let total_feedback: i64 = if let Some(ws) = workspace {
            conn.query_row(
                "SELECT COUNT(*) FROM utility_feedback uf
                 INNER JOIN memories m ON m.id = uf.memory_id
                 WHERE m.workspace = ?1",
                rusqlite::params![ws],
                |r| r.get(0),
            )?
        } else {
            conn.query_row("SELECT COUNT(*) FROM utility_feedback", [], |r| r.get(0))?
        };

        if memory_ids.is_empty() {
            return Ok(UtilityStats {
                total_feedback,
                avg_score: self.config.initial_score,
                top_useful: Vec::new(),
                bottom_useful: Vec::new(),
            });
        }

        // Compute per-memory scores.
        let mut scores: Vec<(i64, f64)> = Vec::with_capacity(memory_ids.len());
        for mid in &memory_ids {
            let us = self.get_utility(conn, *mid)?;
            scores.push((*mid, us.score));
        }

        let avg_score = scores.iter().map(|(_, s)| s).sum::<f64>() / scores.len() as f64;

        // Sort descending for top_useful.
        let mut sorted_desc = scores.clone();
        sorted_desc.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let top_useful: Vec<(i64, i64)> = sorted_desc
            .iter()
            .take(10)
            .map(|(mid, _)| {
                // Count useful retrievals for this memory.
                let cnt: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM utility_feedback WHERE memory_id = ?1 AND was_useful = 1",
                        rusqlite::params![mid],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                (*mid, cnt)
            })
            .collect();

        // Sort ascending for bottom_useful.
        let mut sorted_asc = scores.clone();
        sorted_asc.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let bottom_useful: Vec<(i64, i64)> = sorted_asc
            .iter()
            .take(10)
            .map(|(mid, _)| {
                let cnt: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM utility_feedback WHERE memory_id = ?1 AND was_useful = 1",
                        rusqlite::params![mid],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                (*mid, cnt)
            })
            .collect();

        Ok(UtilityStats {
            total_feedback,
            avg_score,
            top_useful,
            bottom_useful,
        })
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Apply temporal decay based on days elapsed since `last_retrieved_ts`.
    ///
    /// `score *= decay_factor ^ days_elapsed`
    ///
    /// Returns the original score unchanged when the timestamp cannot be parsed
    /// or when the elapsed time is negative (clock skew).
    fn apply_decay(&self, score: f64, last_retrieved_ts: &str) -> f64 {
        if last_retrieved_ts.is_empty() {
            return score;
        }

        let parsed = chrono::DateTime::parse_from_rfc3339(last_retrieved_ts)
            .ok()
            .map(|dt| dt.with_timezone(&chrono::Utc));

        let Some(last) = parsed else {
            return score;
        };

        let now = chrono::Utc::now();
        let days_elapsed = (now - last).num_seconds() as f64 / 86_400.0;

        if days_elapsed <= 0.0 {
            return score;
        }

        score * self.config.decay_factor.powf(days_elapsed)
    }
}

impl Default for UtilityTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(CREATE_UTILITY_FEEDBACK_TABLE)
            .expect("create table");
        conn
    }

    // 1. Record a retrieval event and then retrieve the utility score.
    #[test]
    fn test_record_and_retrieve_utility() {
        let conn = setup();
        let tracker = UtilityTracker::new();

        tracker
            .record_retrieval(&conn, 1, true, "rust async")
            .expect("record");

        let us = tracker.get_utility(&conn, 1).expect("get_utility");

        assert_eq!(us.memory_id, 1);
        assert_eq!(us.retrievals, 1);
        assert_eq!(us.useful_count, 1);
        assert!(!us.last_retrieved.is_empty());
        // Score should be above the initial (0.5) after a useful retrieval.
        assert!(
            us.score > tracker.config.initial_score,
            "score {} should be > initial {}",
            us.score,
            tracker.config.initial_score
        );
    }

    // 2. A series of useful retrievals should push the score toward 1.0.
    #[test]
    fn test_useful_retrievals_boost_score() {
        let conn = setup();
        let tracker = UtilityTracker::new();

        for _ in 0..20 {
            tracker
                .record_retrieval(&conn, 42, true, "query")
                .expect("record");
        }

        let us = tracker.get_utility(&conn, 42).expect("get_utility");

        // After many useful hits the score should be significantly above initial.
        assert!(
            us.score > 0.7,
            "expected score > 0.7 after 20 useful retrievals, got {}",
            us.score
        );
    }

    // 3. A series of irrelevant retrievals should lower the score below initial.
    #[test]
    fn test_irrelevant_retrievals_lower_score() {
        let conn = setup();
        let tracker = UtilityTracker::new();

        for _ in 0..20 {
            tracker
                .record_retrieval(&conn, 7, false, "query")
                .expect("record");
        }

        let us = tracker.get_utility(&conn, 7).expect("get_utility");

        // After many irrelevant hits the score should be below initial.
        assert!(
            us.score < tracker.config.initial_score,
            "expected score < initial ({}) after 20 irrelevant retrievals, got {}",
            tracker.config.initial_score,
            us.score
        );
    }

    // 4. A memory with no feedback should return the configured initial score.
    #[test]
    fn test_initial_score_default_when_no_feedback() {
        let conn = setup();
        let tracker = UtilityTracker::new();

        let us = tracker.get_utility(&conn, 999).expect("get_utility");

        assert_eq!(us.retrievals, 0);
        assert_eq!(us.useful_count, 0);
        assert!(
            (us.score - tracker.config.initial_score).abs() < 1e-9,
            "expected initial score {}, got {}",
            tracker.config.initial_score,
            us.score
        );
        assert!(us.last_retrieved.is_empty());
    }

    // 5. Temporal decay: a custom config with high decay should reduce the score.
    #[test]
    fn test_temporal_decay_reduces_score() {
        let conn = setup();

        // Use high decay (0.5) so even a small elapsed time has a noticeable effect.
        let config = UtilityConfig {
            learning_rate: 0.5,
            decay_factor: 0.5,
            initial_score: 0.5,
        };
        let tracker = UtilityTracker::with_config(config);

        // Insert a feedback row with a timestamp far in the past (100 days ago).
        let past = (chrono::Utc::now() - chrono::Duration::days(100))
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        conn.execute(
            "INSERT INTO utility_feedback (memory_id, was_useful, query, timestamp) VALUES (1, 1, 'q', ?1)",
            rusqlite::params![past],
        )
        .expect("insert");

        let us = tracker.get_utility(&conn, 1).expect("get_utility");

        // After 100 days with decay_factor=0.5, the score approaches 0.
        assert!(
            us.score < 0.1,
            "expected heavily decayed score < 0.1, got {}",
            us.score
        );
    }

    // 6. apply_utility_boost multiplies search scores by the memory's utility.
    #[test]
    fn test_apply_utility_boost() {
        let conn = setup();
        let tracker = UtilityTracker::new();

        // memory 10: many useful → high utility
        for _ in 0..15 {
            tracker
                .record_retrieval(&conn, 10, true, "q")
                .expect("record");
        }
        // memory 20: many useless → low utility
        for _ in 0..15 {
            tracker
                .record_retrieval(&conn, 20, false, "q")
                .expect("record");
        }

        let mut scores = vec![(10_i64, 0.6_f32), (20_i64, 0.6_f32)];
        tracker
            .apply_utility_boost(&mut scores, &conn)
            .expect("boost");

        let boosted = scores[0].1;
        let demoted = scores[1].1;

        assert!(
            boosted > demoted,
            "useful memory ({boosted}) should score higher than useless one ({demoted})"
        );
    }

    // 7. batch_decay returns the count of memories with non-trivial scores.
    #[test]
    fn test_batch_decay_returns_affected_count() {
        let conn = setup();
        let tracker = UtilityTracker::new();

        // Record feedback for 3 distinct memories.
        for mid in [1_i64, 2, 3] {
            tracker
                .record_retrieval(&conn, mid, true, "q")
                .expect("record");
        }

        let config = UtilityConfig::default();
        let count = tracker.batch_decay(&conn, &config).expect("batch_decay");

        // All 3 memories had a useful feedback → score above initial → affected.
        assert_eq!(count, 3, "expected 3 affected memories, got {count}");
    }

    // 8. utility_stats returns correct total_feedback, avg_score, top/bottom.
    #[test]
    fn test_utility_stats() {
        let conn = setup();
        let tracker = UtilityTracker::new();

        // memory 1: 5 useful
        for _ in 0..5 {
            tracker
                .record_retrieval(&conn, 1, true, "q")
                .expect("record");
        }
        // memory 2: 5 useless
        for _ in 0..5 {
            tracker
                .record_retrieval(&conn, 2, false, "q")
                .expect("record");
        }

        let stats = tracker.utility_stats(&conn, None).expect("stats");

        assert_eq!(stats.total_feedback, 10);
        // avg should be between the two scores (one above 0.5, one below).
        assert!(
            stats.avg_score > 0.0 && stats.avg_score < 1.0,
            "avg_score out of range: {}",
            stats.avg_score
        );
        // top_useful should list the useful memory first.
        assert!(!stats.top_useful.is_empty());
        let top_mid = stats.top_useful[0].0;
        assert_eq!(
            top_mid, 1,
            "expected memory 1 on top, got memory {top_mid}"
        );
        // bottom_useful: memory with 0 useful count comes first.
        assert!(!stats.bottom_useful.is_empty());
        let bottom_mid = stats.bottom_useful[0].0;
        assert_eq!(
            bottom_mid, 2,
            "expected memory 2 at bottom, got memory {bottom_mid}"
        );
    }

    // 9. Q-value update formula is applied correctly for a single useful event.
    #[test]
    fn test_q_value_formula_single_useful() {
        let conn = setup();
        let config = UtilityConfig {
            learning_rate: 0.1,
            decay_factor: 1.0, // no decay for determinism
            initial_score: 0.5,
        };
        let tracker = UtilityTracker::with_config(config);

        tracker
            .record_retrieval(&conn, 1, true, "q")
            .expect("record");

        // Q = 0.5 + 0.1 * (1.0 - 0.5) = 0.5 + 0.05 = 0.55
        let us = tracker.get_utility(&conn, 1).expect("get_utility");
        let expected = 0.55;
        assert!(
            (us.score - expected).abs() < 1e-9,
            "expected score {expected}, got {}",
            us.score
        );
    }

    // 10. Q-value update formula is applied correctly for a single non-useful event.
    #[test]
    fn test_q_value_formula_single_not_useful() {
        let conn = setup();
        let config = UtilityConfig {
            learning_rate: 0.1,
            decay_factor: 1.0, // no decay
            initial_score: 0.5,
        };
        let tracker = UtilityTracker::with_config(config);

        tracker
            .record_retrieval(&conn, 2, false, "q")
            .expect("record");

        // Q = 0.5 + 0.1 * (-0.5 - 0.5) = 0.5 + 0.1 * (-1.0) = 0.5 - 0.1 = 0.4
        let us = tracker.get_utility(&conn, 2).expect("get_utility");
        let expected = 0.4;
        assert!(
            (us.score - expected).abs() < 1e-9,
            "expected score {expected}, got {}",
            us.score
        );
    }

    // 11. Boost clamp: score stays within [0.5, 2.0] for extreme utilities.
    #[test]
    fn test_boost_clamp_bounds() {
        let conn = setup();
        let tracker = UtilityTracker::new();

        // memory 100: single useful (moderate boost).
        tracker
            .record_retrieval(&conn, 100, true, "q")
            .expect("record");

        let mut scores = vec![(100_i64, 0.1_f32)];
        tracker
            .apply_utility_boost(&mut scores, &conn)
            .expect("boost");

        // Result must stay within the [0.5, 2.0] clamp.
        assert!(
            scores[0].1 >= 0.5 && scores[0].1 <= 2.0,
            "boosted score {} is outside [0.5, 2.0]",
            scores[0].1
        );
    }
}
