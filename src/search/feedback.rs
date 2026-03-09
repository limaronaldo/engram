//! Relevance Feedback Loop — RML-1243
//!
//! Persists explicit user feedback (useful / irrelevant) for search results and
//! uses that history to compute per-memory boost factors that can be applied to
//! subsequent search score vectors.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{EngramError, Result};

// ---------------------------------------------------------------------------
// DDL
// ---------------------------------------------------------------------------

/// SQL for creating the `search_feedback` table and its indexes.
/// Safe to call on an existing database — uses `CREATE TABLE IF NOT EXISTS`.
pub const CREATE_SEARCH_FEEDBACK_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS search_feedback (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    query TEXT NOT NULL,
    query_embedding_hash TEXT,
    memory_id INTEGER NOT NULL,
    signal TEXT NOT NULL CHECK(signal IN ('useful', 'irrelevant')),
    rank_position INTEGER,
    original_score REAL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    workspace TEXT DEFAULT 'default'
);
CREATE INDEX IF NOT EXISTS idx_feedback_memory ON search_feedback(memory_id);
CREATE INDEX IF NOT EXISTS idx_feedback_query ON search_feedback(query);
CREATE INDEX IF NOT EXISTS idx_feedback_workspace ON search_feedback(workspace);
"#;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Feedback signal: whether a search result was helpful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackSignal {
    Useful,
    Irrelevant,
}

impl FeedbackSignal {
    fn as_str(self) -> &'static str {
        match self {
            FeedbackSignal::Useful => "useful",
            FeedbackSignal::Irrelevant => "irrelevant",
        }
    }

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "useful" => Ok(FeedbackSignal::Useful),
            "irrelevant" => Ok(FeedbackSignal::Irrelevant),
            other => Err(EngramError::InvalidInput(format!(
                "unknown feedback signal: {other}"
            ))),
        }
    }
}

/// A single recorded feedback entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFeedback {
    pub id: i64,
    pub query: String,
    pub query_embedding_hash: Option<String>,
    pub memory_id: i64,
    pub signal: FeedbackSignal,
    pub rank_position: Option<i32>,
    pub original_score: Option<f32>,
    pub created_at: String,
    pub workspace: String,
}

/// Aggregated feedback statistics for a workspace (or all workspaces).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackStats {
    pub total_feedback: i64,
    pub useful_count: i64,
    pub irrelevant_count: i64,
    pub useful_ratio: f64,
    /// Top memories marked useful — `(memory_id, count)`, up to 10 entries.
    pub top_useful_memories: Vec<(i64, i64)>,
    /// Top memories marked irrelevant — `(memory_id, count)`, up to 10 entries.
    pub top_irrelevant_memories: Vec<(i64, i64)>,
    /// Average rank position of results marked useful.
    pub avg_useful_rank: Option<f64>,
    /// Average rank position of results marked irrelevant.
    pub avg_irrelevant_rank: Option<f64>,
}

/// Boost factor derived from feedback history for a single memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackBoost {
    pub memory_id: i64,
    /// Multiplier applied to the raw search score.
    /// `> 1.0` promotes the result, `< 1.0` demotes it, `1.0` is neutral.
    pub boost_factor: f64,
    /// Total number of feedback signals this boost is based on.
    pub signal_count: i64,
    /// Confidence in the boost estimate (0.0 – 1.0).
    /// Increases with more signals, capped at 1.0.
    pub confidence: f64,
}

// ---------------------------------------------------------------------------
// Storage functions
// ---------------------------------------------------------------------------

/// Record a feedback signal for a (query, memory) pair.
///
/// Returns the newly created [`SearchFeedback`] row.
pub fn record_feedback(
    conn: &Connection,
    query: &str,
    memory_id: i64,
    signal: FeedbackSignal,
    rank_position: Option<i32>,
    original_score: Option<f32>,
    workspace: &str,
) -> Result<SearchFeedback> {
    conn.execute(
        "INSERT INTO search_feedback (query, memory_id, signal, rank_position, original_score, workspace)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            query,
            memory_id,
            signal.as_str(),
            rank_position,
            original_score,
            workspace,
        ],
    )?;

    let id = conn.last_insert_rowid();

    let row = conn.query_row(
        "SELECT id, query, query_embedding_hash, memory_id, signal,
                rank_position, original_score, created_at, workspace
         FROM search_feedback WHERE id = ?1",
        rusqlite::params![id],
        row_to_feedback,
    )?;

    Ok(row)
}

/// Retrieve all feedback rows for a specific memory.
pub fn get_feedback_for_memory(conn: &Connection, memory_id: i64) -> Result<Vec<SearchFeedback>> {
    let mut stmt = conn.prepare(
        "SELECT id, query, query_embedding_hash, memory_id, signal,
                rank_position, original_score, created_at, workspace
         FROM search_feedback
         WHERE memory_id = ?1
         ORDER BY created_at DESC",
    )?;

    let rows = stmt
        .query_map(rusqlite::params![memory_id], row_to_feedback)?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(rows)
}

/// Retrieve all feedback rows for a specific query string.
pub fn get_feedback_for_query(conn: &Connection, query: &str) -> Result<Vec<SearchFeedback>> {
    let mut stmt = conn.prepare(
        "SELECT id, query, query_embedding_hash, memory_id, signal,
                rank_position, original_score, created_at, workspace
         FROM search_feedback
         WHERE query = ?1
         ORDER BY created_at DESC",
    )?;

    let rows = stmt
        .query_map(rusqlite::params![query], row_to_feedback)?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(rows)
}

/// Delete a single feedback entry by its ID.
pub fn delete_feedback(conn: &Connection, feedback_id: i64) -> Result<()> {
    let affected = conn.execute(
        "DELETE FROM search_feedback WHERE id = ?1",
        rusqlite::params![feedback_id],
    )?;

    if affected == 0 {
        return Err(EngramError::NotFound(feedback_id));
    }

    Ok(())
}

/// Compute aggregated feedback statistics.
///
/// When `workspace` is `Some`, only feedback rows from that workspace are
/// included.  Pass `None` to aggregate across all workspaces.
pub fn feedback_stats(conn: &Connection, workspace: Option<&str>) -> Result<FeedbackStats> {
    // Helper: execute a query that uses an optional workspace parameter.
    // When `ws` is Some the query must use `?1` as its sole parameter.
    let exec_scalar = |sql: &str| -> Result<(i64, i64, i64)> {
        if let Some(ws) = workspace {
            Ok(conn.query_row(sql, rusqlite::params![ws], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?)
        } else {
            Ok(conn.query_row(sql, [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?)
        }
    };

    let exec_pairs = |sql: &str| -> Result<Vec<(i64, i64)>> {
        if let Some(ws) = workspace {
            let mut stmt = conn.prepare(sql)?;
            let v = stmt
                .query_map(rusqlite::params![ws], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(v)
        } else {
            let mut stmt = conn.prepare(sql)?;
            let v = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(v)
        }
    };

    let exec_avg = |sql: &str| -> Result<Option<f64>> {
        let v: Option<f64> = if let Some(ws) = workspace {
            conn.query_row(sql, rusqlite::params![ws], |r| r.get(0))
                .optional()?
                .flatten()
        } else {
            conn.query_row(sql, [], |r| r.get(0))
                .optional()?
                .flatten()
        };
        Ok(v)
    };

    // Build SQL that filters by workspace when provided.
    let ws_clause = if workspace.is_some() {
        "WHERE workspace = ?1"
    } else {
        "WHERE 1=1"
    };

    // --- totals ---
    let totals_sql = format!(
        "SELECT
            COUNT(*),
            SUM(CASE WHEN signal = 'useful' THEN 1 ELSE 0 END),
            SUM(CASE WHEN signal = 'irrelevant' THEN 1 ELSE 0 END)
         FROM search_feedback {ws_clause}"
    );
    let (total_feedback, useful_count, irrelevant_count) = exec_scalar(&totals_sql)?;

    let useful_ratio = if total_feedback == 0 {
        0.0
    } else {
        useful_count as f64 / total_feedback as f64
    };

    // --- top useful memories ---
    let top_useful_sql = format!(
        "SELECT memory_id, COUNT(*) AS cnt
         FROM search_feedback
         {ws_clause} AND signal = 'useful'
         GROUP BY memory_id
         ORDER BY cnt DESC
         LIMIT 10"
    );
    let top_useful_memories = exec_pairs(&top_useful_sql)?;

    // --- top irrelevant memories ---
    let top_irrelevant_sql = format!(
        "SELECT memory_id, COUNT(*) AS cnt
         FROM search_feedback
         {ws_clause} AND signal = 'irrelevant'
         GROUP BY memory_id
         ORDER BY cnt DESC
         LIMIT 10"
    );
    let top_irrelevant_memories = exec_pairs(&top_irrelevant_sql)?;

    // --- average ranks ---
    let avg_useful_sql = format!(
        "SELECT AVG(rank_position)
         FROM search_feedback
         {ws_clause} AND signal = 'useful' AND rank_position IS NOT NULL"
    );
    let avg_useful_rank = exec_avg(&avg_useful_sql)?;

    let avg_irrelevant_sql = format!(
        "SELECT AVG(rank_position)
         FROM search_feedback
         {ws_clause} AND signal = 'irrelevant' AND rank_position IS NOT NULL"
    );
    let avg_irrelevant_rank = exec_avg(&avg_irrelevant_sql)?;

    Ok(FeedbackStats {
        total_feedback,
        useful_count,
        irrelevant_count,
        useful_ratio,
        top_useful_memories,
        top_irrelevant_memories,
        avg_useful_rank,
        avg_irrelevant_rank,
    })
}

/// Compute boost factors for a set of memory IDs.
///
/// For each memory, the boost formula is:
///
/// ```text
/// boost = 1.0 + (useful_count - irrelevant_count * 1.5) / (total_count + 5)
/// ```
///
/// The `+5` smoothing term prevents extreme boosts from very few signals.
/// Confidence is `min(1.0, total_count / 10.0)`.
///
/// If `query` is provided, feedback rows whose query text overlaps heavily
/// with the current query receive a 2× weight in the aggregation (query
/// similarity is measured as Jaccard overlap on word sets).
pub fn compute_feedback_boosts(
    conn: &Connection,
    memory_ids: &[i64],
    query: Option<&str>,
) -> Result<Vec<FeedbackBoost>> {
    if memory_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut boosts = Vec::with_capacity(memory_ids.len());

    for &memory_id in memory_ids {
        // Fetch all feedback for this memory.
        let rows = get_feedback_for_memory(conn, memory_id)?;

        if rows.is_empty() {
            boosts.push(FeedbackBoost {
                memory_id,
                boost_factor: 1.0,
                signal_count: 0,
                confidence: 0.0,
            });
            continue;
        }

        // Accumulate weighted counts.
        let mut weighted_useful = 0.0_f64;
        let mut weighted_irrelevant = 0.0_f64;
        let mut weighted_total = 0.0_f64;

        for row in &rows {
            let weight = if let Some(q) = query {
                query_similarity_weight(q, &row.query)
            } else {
                1.0
            };

            match row.signal {
                FeedbackSignal::Useful => weighted_useful += weight,
                FeedbackSignal::Irrelevant => weighted_irrelevant += weight,
            }
            weighted_total += weight;
        }

        let signal_count = rows.len() as i64;
        let boost_factor =
            1.0 + (weighted_useful - weighted_irrelevant * 1.5) / (weighted_total + 5.0);
        let confidence = (signal_count as f64 / 10.0).min(1.0);

        boosts.push(FeedbackBoost {
            memory_id,
            boost_factor,
            signal_count,
            confidence,
        });
    }

    Ok(boosts)
}

/// Apply boost factors to a slice of `(memory_id, score)` pairs in-place.
///
/// Each score is multiplied by the matching boost factor, clamped to `[0.5, 2.0]`.
/// Memory IDs with no matching boost entry are left unchanged.
pub fn apply_feedback_boosts(scores: &mut [(i64, f32)], boosts: &[FeedbackBoost]) {
    for (memory_id, score) in scores.iter_mut() {
        if let Some(boost) = boosts.iter().find(|b| b.memory_id == *memory_id) {
            *score = (*score * boost.boost_factor as f32).clamp(0.5, 2.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Map a rusqlite row to [`SearchFeedback`].
fn row_to_feedback(r: &rusqlite::Row<'_>) -> rusqlite::Result<SearchFeedback> {
    let signal_str: String = r.get(4)?;
    let signal = FeedbackSignal::from_str(&signal_str).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::new(std::fmt::Error),
        )
    })?;

    Ok(SearchFeedback {
        id: r.get(0)?,
        query: r.get(1)?,
        query_embedding_hash: r.get(2)?,
        memory_id: r.get(3)?,
        signal,
        rank_position: r.get(5)?,
        original_score: r.get(6)?,
        created_at: r.get(7)?,
        workspace: r.get(8)?,
    })
}

/// Compute a simple query similarity weight in `[1.0, 2.0]`.
///
/// Uses Jaccard overlap on word sets:
/// - identical queries → 2.0
/// - no overlap → 1.0
/// - partial overlap → interpolated
fn query_similarity_weight(current: &str, historical: &str) -> f64 {
    let current_words: std::collections::HashSet<&str> =
        current.split_whitespace().collect();
    let historical_words: std::collections::HashSet<&str> =
        historical.split_whitespace().collect();

    if current_words.is_empty() || historical_words.is_empty() {
        return 1.0;
    }

    let intersection = current_words.intersection(&historical_words).count();
    let union = current_words.union(&historical_words).count();

    let jaccard = intersection as f64 / union as f64;
    // Map [0, 1] → [1.0, 2.0]
    1.0 + jaccard
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(CREATE_SEARCH_FEEDBACK_TABLE)
            .expect("create table");
        conn
    }

    // 1. Record and retrieve feedback
    #[test]
    fn test_record_and_retrieve_feedback() {
        let conn = setup();

        let fb = record_feedback(&conn, "rust async", 42, FeedbackSignal::Useful, Some(1), Some(0.9), "default")
            .expect("record");

        assert_eq!(fb.query, "rust async");
        assert_eq!(fb.memory_id, 42);
        assert_eq!(fb.signal, FeedbackSignal::Useful);
        assert_eq!(fb.rank_position, Some(1));
        assert!((fb.original_score.unwrap() - 0.9).abs() < 1e-5);
        assert_eq!(fb.workspace, "default");
        assert!(fb.id > 0);
    }

    // 2. Record useful signal
    #[test]
    fn test_record_useful_signal() {
        let conn = setup();

        let fb = record_feedback(&conn, "search query", 10, FeedbackSignal::Useful, None, None, "ws1")
            .expect("record useful");

        assert_eq!(fb.signal, FeedbackSignal::Useful);
    }

    // 3. Record irrelevant signal
    #[test]
    fn test_record_irrelevant_signal() {
        let conn = setup();

        let fb = record_feedback(&conn, "another query", 20, FeedbackSignal::Irrelevant, Some(5), Some(0.3), "ws1")
            .expect("record irrelevant");

        assert_eq!(fb.signal, FeedbackSignal::Irrelevant);
        assert_eq!(fb.rank_position, Some(5));
    }

    // 4. Stats computation — counts and ratios
    #[test]
    fn test_stats_counts_and_ratios() {
        let conn = setup();

        record_feedback(&conn, "q", 1, FeedbackSignal::Useful, None, None, "ws").unwrap();
        record_feedback(&conn, "q", 2, FeedbackSignal::Useful, None, None, "ws").unwrap();
        record_feedback(&conn, "q", 3, FeedbackSignal::Irrelevant, None, None, "ws").unwrap();

        let stats = feedback_stats(&conn, None).expect("stats");

        assert_eq!(stats.total_feedback, 3);
        assert_eq!(stats.useful_count, 2);
        assert_eq!(stats.irrelevant_count, 1);
        assert!((stats.useful_ratio - 2.0 / 3.0).abs() < 1e-9);
    }

    // 5. Stats with workspace filter
    #[test]
    fn test_stats_workspace_filter() {
        let conn = setup();

        record_feedback(&conn, "q", 1, FeedbackSignal::Useful, None, None, "ws_a").unwrap();
        record_feedback(&conn, "q", 2, FeedbackSignal::Useful, None, None, "ws_a").unwrap();
        record_feedback(&conn, "q", 3, FeedbackSignal::Irrelevant, None, None, "ws_b").unwrap();

        let stats_a = feedback_stats(&conn, Some("ws_a")).expect("stats_a");
        assert_eq!(stats_a.total_feedback, 2);
        assert_eq!(stats_a.useful_count, 2);
        assert_eq!(stats_a.irrelevant_count, 0);

        let stats_b = feedback_stats(&conn, Some("ws_b")).expect("stats_b");
        assert_eq!(stats_b.total_feedback, 1);
        assert_eq!(stats_b.useful_count, 0);
        assert_eq!(stats_b.irrelevant_count, 1);
    }

    // 6. Boost — mostly useful signals → boost > 1.0
    #[test]
    fn test_boost_mostly_useful() {
        let conn = setup();

        for _ in 0..8 {
            record_feedback(&conn, "q", 99, FeedbackSignal::Useful, None, None, "ws").unwrap();
        }
        record_feedback(&conn, "q", 99, FeedbackSignal::Irrelevant, None, None, "ws").unwrap();

        let boosts = compute_feedback_boosts(&conn, &[99], None).expect("boosts");
        assert_eq!(boosts.len(), 1);
        assert!(boosts[0].boost_factor > 1.0, "expected boost > 1.0, got {}", boosts[0].boost_factor);
    }

    // 7. Boost — mostly irrelevant → boost < 1.0
    #[test]
    fn test_boost_mostly_irrelevant() {
        let conn = setup();

        for _ in 0..8 {
            record_feedback(&conn, "q", 77, FeedbackSignal::Irrelevant, None, None, "ws").unwrap();
        }
        record_feedback(&conn, "q", 77, FeedbackSignal::Useful, None, None, "ws").unwrap();

        let boosts = compute_feedback_boosts(&conn, &[77], None).expect("boosts");
        assert_eq!(boosts.len(), 1);
        assert!(boosts[0].boost_factor < 1.0, "expected boost < 1.0, got {}", boosts[0].boost_factor);
    }

    // 8. Boost — no feedback → boost = 1.0
    #[test]
    fn test_boost_no_feedback() {
        let conn = setup();

        let boosts = compute_feedback_boosts(&conn, &[999], None).expect("boosts");
        assert_eq!(boosts.len(), 1);
        assert_eq!(boosts[0].boost_factor, 1.0);
        assert_eq!(boosts[0].signal_count, 0);
        assert_eq!(boosts[0].confidence, 0.0);
    }

    // 9. Boost smoothing prevents extreme values with few signals
    #[test]
    fn test_boost_smoothing_prevents_extremes() {
        let conn = setup();

        // Only 1 useful signal — smoothing (+5) should keep boost moderate.
        record_feedback(&conn, "q", 55, FeedbackSignal::Useful, None, None, "ws").unwrap();

        let boosts = compute_feedback_boosts(&conn, &[55], None).expect("boosts");
        // With 1 useful: boost = 1 + (1 - 0) / (1 + 5) = 1 + 1/6 ≈ 1.167
        let expected = 1.0 + 1.0 / 6.0;
        assert!((boosts[0].boost_factor - expected).abs() < 1e-9);
        // Not extreme (e.g., not 2.0)
        assert!(boosts[0].boost_factor < 1.3);
    }

    // 10. Apply boosts modifies scores correctly
    #[test]
    fn test_apply_boosts_modifies_scores() {
        let boosts = vec![
            FeedbackBoost { memory_id: 1, boost_factor: 1.5, signal_count: 5, confidence: 0.5 },
            // 0.7 * 0.8 = 0.56 — stays above the 0.5 clamp floor
            FeedbackBoost { memory_id: 2, boost_factor: 0.8, signal_count: 3, confidence: 0.3 },
        ];

        let mut scores = vec![(1_i64, 0.6_f32), (2_i64, 0.7_f32), (3_i64, 0.4_f32)];
        apply_feedback_boosts(&mut scores, &boosts);

        // memory 1: 0.6 * 1.5 = 0.9
        assert!((scores[0].1 - 0.9_f32).abs() < 1e-5, "score[0] = {}", scores[0].1);
        // memory 2: 0.7 * 0.8 = 0.56
        assert!((scores[1].1 - 0.56_f32).abs() < 1e-4, "score[1] = {}", scores[1].1);
        // memory 3: no boost entry, unchanged
        assert!((scores[2].1 - 0.4_f32).abs() < 1e-5, "score[2] = {}", scores[2].1);
    }

    // 11. Boost clamping to [0.5, 2.0]
    #[test]
    fn test_boost_clamping() {
        // Very high boost factor → clamped to 2.0
        let boosts_high = vec![FeedbackBoost {
            memory_id: 10,
            boost_factor: 5.0,
            signal_count: 100,
            confidence: 1.0,
        }];
        let mut scores_high = vec![(10_i64, 0.9_f32)];
        apply_feedback_boosts(&mut scores_high, &boosts_high);
        assert!((scores_high[0].1 - 2.0_f32).abs() < 1e-5, "expected clamp to 2.0, got {}", scores_high[0].1);

        // Very low boost factor → clamped to 0.5
        let boosts_low = vec![FeedbackBoost {
            memory_id: 20,
            boost_factor: 0.1,
            signal_count: 100,
            confidence: 1.0,
        }];
        let mut scores_low = vec![(20_i64, 0.9_f32)];
        apply_feedback_boosts(&mut scores_low, &boosts_low);
        assert!((scores_low[0].1 - 0.5_f32).abs() < 1e-5, "expected clamp to 0.5, got {}", scores_low[0].1);
    }

    // 12. Delete feedback
    #[test]
    fn test_delete_feedback() {
        let conn = setup();

        let fb = record_feedback(&conn, "to delete", 1, FeedbackSignal::Useful, None, None, "ws")
            .expect("record");

        delete_feedback(&conn, fb.id).expect("delete");

        let remaining = get_feedback_for_memory(&conn, 1).expect("get");
        assert!(remaining.is_empty());
    }

    // 12b. Delete non-existent feedback returns NotFound
    #[test]
    fn test_delete_nonexistent_feedback() {
        let conn = setup();
        let result = delete_feedback(&conn, 9999);
        assert!(matches!(result, Err(EngramError::NotFound(_))));
    }

    // 13. Query similarity weighting
    #[test]
    fn test_query_similarity_weighting() {
        let conn = setup();

        // Record feedback for two different queries on the same memory.
        // "rust async runtime" overlaps with "rust async" but not "python web".
        record_feedback(&conn, "rust async runtime", 42, FeedbackSignal::Useful, None, None, "ws").unwrap();
        record_feedback(&conn, "python web framework", 42, FeedbackSignal::Irrelevant, None, None, "ws").unwrap();

        // Query "rust async" — should weight the useful signal higher → boost > 1.0
        let boosts_rust = compute_feedback_boosts(&conn, &[42], Some("rust async")).expect("boosts");
        assert!(boosts_rust[0].boost_factor > 1.0,
            "expected boost > 1.0 with matching query, got {}", boosts_rust[0].boost_factor);

        // Query "python web" — should weight the irrelevant signal higher → boost < 1.0
        let boosts_python = compute_feedback_boosts(&conn, &[42], Some("python web")).expect("boosts");
        assert!(boosts_python[0].boost_factor < 1.0,
            "expected boost < 1.0 with mismatched query, got {}", boosts_python[0].boost_factor);
    }

    // Extra: get_feedback_for_query
    #[test]
    fn test_get_feedback_for_query() {
        let conn = setup();

        record_feedback(&conn, "specific query", 1, FeedbackSignal::Useful, None, None, "ws").unwrap();
        record_feedback(&conn, "specific query", 2, FeedbackSignal::Irrelevant, None, None, "ws").unwrap();
        record_feedback(&conn, "other query", 3, FeedbackSignal::Useful, None, None, "ws").unwrap();

        let rows = get_feedback_for_query(&conn, "specific query").expect("get");
        assert_eq!(rows.len(), 2);
        for r in &rows {
            assert_eq!(r.query, "specific query");
        }
    }

    // Extra: top_useful and top_irrelevant_memories populated correctly
    #[test]
    fn test_stats_top_memories() {
        let conn = setup();

        // memory 1: 3 useful
        for _ in 0..3 {
            record_feedback(&conn, "q", 1, FeedbackSignal::Useful, None, None, "ws").unwrap();
        }
        // memory 2: 1 useful
        record_feedback(&conn, "q", 2, FeedbackSignal::Useful, None, None, "ws").unwrap();
        // memory 3: 2 irrelevant
        for _ in 0..2 {
            record_feedback(&conn, "q", 3, FeedbackSignal::Irrelevant, None, None, "ws").unwrap();
        }

        let stats = feedback_stats(&conn, None).unwrap();
        assert_eq!(stats.top_useful_memories[0].0, 1);
        assert_eq!(stats.top_useful_memories[0].1, 3);
        assert_eq!(stats.top_irrelevant_memories[0].0, 3);
        assert_eq!(stats.top_irrelevant_memories[0].1, 2);
    }

    // Extra: average rank computed correctly
    #[test]
    fn test_stats_avg_rank() {
        let conn = setup();

        record_feedback(&conn, "q", 1, FeedbackSignal::Useful, Some(1), None, "ws").unwrap();
        record_feedback(&conn, "q", 2, FeedbackSignal::Useful, Some(3), None, "ws").unwrap();
        record_feedback(&conn, "q", 3, FeedbackSignal::Irrelevant, Some(10), None, "ws").unwrap();

        let stats = feedback_stats(&conn, None).unwrap();
        // avg useful rank = (1 + 3) / 2 = 2.0
        assert!((stats.avg_useful_rank.unwrap() - 2.0).abs() < 1e-9);
        // avg irrelevant rank = 10.0
        assert!((stats.avg_irrelevant_rank.unwrap() - 10.0).abs() < 1e-9);
    }

    // Extra: empty memory_ids returns empty vec
    #[test]
    fn test_compute_boosts_empty_ids() {
        let conn = setup();
        let boosts = compute_feedback_boosts(&conn, &[], None).expect("boosts");
        assert!(boosts.is_empty());
    }
}
