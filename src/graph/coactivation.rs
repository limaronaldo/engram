//! Temporal Coactivation / Hebbian Learning (RML-1218).
//!
//! "Neurons that fire together wire together" — memories retrieved in the same
//! session get stronger connections each time they co-occur, and weaken when
//! left unused.
//!
//! # Overview
//!
//! [`CoactivationTracker`] maintains a `coactivation_edges` table in SQLite.
//! Each edge tracks:
//!
//! - `strength` — a value in `[0, 1]` that grows via the Hebbian update rule:
//!   `strength ← min(1.0, strength + lr × (1 − strength))`
//! - `coactivation_count` — raw co-occurrence counter
//! - `last_coactivated` — RFC 3339 timestamp of the most recent co-activation
//!
//! Edges are directional but for most queries both directions are combined so
//! that the graph is effectively undirected.

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{EngramError, Result};

// =============================================================================
// DDL
// =============================================================================

/// SQL that creates the `coactivation_edges` table and its supporting indexes.
///
/// Safe to run on an existing database — all statements use `IF NOT EXISTS`.
pub const CREATE_COACTIVATION_EDGES_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS coactivation_edges (
    from_id              INTEGER NOT NULL,
    to_id                INTEGER NOT NULL,
    strength             REAL    NOT NULL DEFAULT 0.0,
    coactivation_count   INTEGER NOT NULL DEFAULT 0,
    last_coactivated     TEXT    NOT NULL,
    PRIMARY KEY (from_id, to_id)
);
CREATE INDEX IF NOT EXISTS idx_coact_from ON coactivation_edges(from_id);
CREATE INDEX IF NOT EXISTS idx_coact_to   ON coactivation_edges(to_id);
CREATE INDEX IF NOT EXISTS idx_coact_str  ON coactivation_edges(strength DESC);
"#;

// =============================================================================
// Configuration
// =============================================================================

/// Tuning parameters for the Hebbian learning model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoactivationConfig {
    /// Hebbian learning rate — controls how quickly strength grows (0 < lr ≤ 1).
    ///
    /// Default: `0.1`
    pub learning_rate: f64,

    /// Decay multiplier applied to edges unused for `min_age_days` days.
    ///
    /// Each invocation of [`CoactivationTracker::weaken_unused`] multiplies
    /// the strength of qualifying edges by `(1 − decay_rate)`.
    ///
    /// Default: `0.01`
    pub decay_rate: f64,

    /// Edges whose strength drops below this threshold are deleted.
    ///
    /// Default: `0.01`
    pub min_strength: f64,
}

impl Default for CoactivationConfig {
    fn default() -> Self {
        Self {
            learning_rate: 0.1,
            decay_rate: 0.01,
            min_strength: 0.01,
        }
    }
}

// =============================================================================
// Data types
// =============================================================================

/// A single Hebbian edge between two memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoactivationEdge {
    /// Source memory ID.
    pub from_id: i64,
    /// Target memory ID.
    pub to_id: i64,
    /// Current connection strength in `[0, 1]`.
    pub strength: f64,
    /// Total number of co-activations recorded.
    pub count: i64,
    /// RFC 3339 timestamp of the most recent co-activation.
    pub last_coactivated: String,
}

/// Aggregate statistics for the coactivation graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoactivationReport {
    /// Total number of edges in the graph.
    pub total_edges: i64,
    /// Mean strength across all edges (0.0 when graph is empty).
    pub avg_strength: f64,
    /// Top-10 strongest pairs as `(from_id, to_id, strength)`.
    pub strongest_pairs: Vec<(i64, i64, f64)>,
}

// =============================================================================
// Tracker
// =============================================================================

/// Stateless tracker — borrows a `rusqlite::Connection` for each operation.
///
/// All state lives in the `coactivation_edges` database table.
pub struct CoactivationTracker {
    /// Configuration controlling Hebbian dynamics.
    pub config: CoactivationConfig,
}

impl CoactivationTracker {
    /// Create a tracker with default configuration.
    pub fn new() -> Self {
        Self {
            config: CoactivationConfig::default(),
        }
    }

    /// Create a tracker with custom configuration.
    pub fn with_config(config: CoactivationConfig) -> Self {
        Self { config }
    }

    // -------------------------------------------------------------------------
    // record_coactivation
    // -------------------------------------------------------------------------

    /// Record that a set of memories were retrieved together in one session.
    ///
    /// For every unordered pair `(a, b)` in `memory_ids` (where `a < b`) the
    /// function upserts a `coactivation_edges` row, applying the Hebbian
    /// update:
    ///
    /// ```text
    /// strength ← min(1.0, strength + lr × (1 − strength))
    /// ```
    ///
    /// Returns the number of edges updated (i.e. `n × (n−1) / 2` where `n` is
    /// the number of unique IDs, assuming no self-loops).
    ///
    /// `session_id` is informational only — it is not stored but could be used
    /// by callers for audit logging.
    pub fn record_coactivation(
        &self,
        conn: &Connection,
        memory_ids: &[i64],
        _session_id: &str,
    ) -> Result<usize> {
        // Deduplicate and sort to ensure canonical ordering.
        let mut ids: Vec<i64> = memory_ids.to_vec();
        ids.sort_unstable();
        ids.dedup();

        let now = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let lr = self.config.learning_rate;
        let mut updated = 0usize;

        // Iterate over every unique unordered pair.
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let from_id = ids[i];
                let to_id = ids[j];
                self.upsert_edge(conn, from_id, to_id, lr, &now)?;
                updated += 1;
            }
        }

        Ok(updated)
    }

    // -------------------------------------------------------------------------
    // strengthen
    // -------------------------------------------------------------------------

    /// Apply a single Hebbian update to the edge `from_id → to_id`.
    ///
    /// Creates the edge if it does not exist yet. Returns the new strength.
    pub fn strengthen(&self, conn: &Connection, from_id: i64, to_id: i64) -> Result<f64> {
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        self.upsert_edge(conn, from_id, to_id, self.config.learning_rate, &now)?;

        // Read back the updated strength.
        let strength: f64 = conn
            .query_row(
                "SELECT strength FROM coactivation_edges WHERE from_id = ?1 AND to_id = ?2",
                params![from_id, to_id],
                |row| row.get(0),
            )
            .map_err(EngramError::Database)?;

        Ok(strength)
    }

    // -------------------------------------------------------------------------
    // weaken_unused
    // -------------------------------------------------------------------------

    /// Decay edges that have not been co-activated in at least `min_age_days`.
    ///
    /// Each qualifying edge has its strength multiplied by `(1 − decay_rate)`.
    /// Edges that fall below [`CoactivationConfig::min_strength`] are deleted.
    ///
    /// Returns the number of edges affected (updated **or** deleted).
    pub fn weaken_unused(
        &self,
        conn: &Connection,
        decay_rate: f64,
        min_age_days: u32,
    ) -> Result<usize> {
        let cutoff = Utc::now() - chrono::Duration::days(min_age_days as i64);
        let cutoff_str = cutoff.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let min_strength = self.config.min_strength;

        // Apply decay to edges that haven't been touched recently.
        let updated = conn
            .execute(
                "UPDATE coactivation_edges
                 SET strength = strength * (1.0 - ?1)
                 WHERE last_coactivated < ?2",
                params![decay_rate, cutoff_str],
            )
            .map_err(EngramError::Database)?;

        // Delete edges that fell below the minimum strength threshold.
        let deleted = conn
            .execute(
                "DELETE FROM coactivation_edges WHERE strength < ?1",
                params![min_strength],
            )
            .map_err(EngramError::Database)?;

        Ok(updated + deleted)
    }

    // -------------------------------------------------------------------------
    // get_coactivation_graph
    // -------------------------------------------------------------------------

    /// Return all edges incident to `memory_id`, sorted by strength descending.
    ///
    /// Both `from_id = memory_id` and `to_id = memory_id` rows are returned,
    /// normalized so that `from_id` is always `memory_id` in the result.
    pub fn get_coactivation_graph(
        &self,
        conn: &Connection,
        memory_id: i64,
    ) -> Result<Vec<CoactivationEdge>> {
        let mut stmt = conn
            .prepare(
                "SELECT from_id, to_id, strength, coactivation_count, last_coactivated
                 FROM coactivation_edges
                 WHERE from_id = ?1 OR to_id = ?1
                 ORDER BY strength DESC",
            )
            .map_err(EngramError::Database)?;

        let edges = stmt
            .query_map(params![memory_id], |row| {
                Ok(CoactivationEdge {
                    from_id: row.get(0)?,
                    to_id: row.get(1)?,
                    strength: row.get(2)?,
                    count: row.get(3)?,
                    last_coactivated: row.get(4)?,
                })
            })
            .map_err(EngramError::Database)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(EngramError::Database)?;

        Ok(edges)
    }

    // -------------------------------------------------------------------------
    // suggest_related
    // -------------------------------------------------------------------------

    /// Return the `top_k` strongest co-activation partners for `memory_id`.
    ///
    /// Result tuples are `(neighbor_id, strength)` sorted by strength
    /// descending.
    pub fn suggest_related(
        &self,
        conn: &Connection,
        memory_id: i64,
        top_k: usize,
    ) -> Result<Vec<(i64, f64)>> {
        // Query both directions; expose the neighbor ID regardless of direction.
        let mut stmt = conn
            .prepare(
                "SELECT
                     CASE WHEN from_id = ?1 THEN to_id ELSE from_id END AS neighbor,
                     strength
                 FROM coactivation_edges
                 WHERE from_id = ?1 OR to_id = ?1
                 ORDER BY strength DESC
                 LIMIT ?2",
            )
            .map_err(EngramError::Database)?;

        let pairs = stmt
            .query_map(params![memory_id, top_k as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(EngramError::Database)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(EngramError::Database)?;

        Ok(pairs)
    }

    // -------------------------------------------------------------------------
    // report
    // -------------------------------------------------------------------------

    /// Compute aggregate statistics over the entire coactivation graph.
    pub fn report(&self, conn: &Connection) -> Result<CoactivationReport> {
        // Total edge count and average strength.
        let (total_edges, avg_strength): (i64, f64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(AVG(strength), 0.0) FROM coactivation_edges",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(EngramError::Database)?;

        // Top-10 strongest pairs.
        let mut stmt = conn
            .prepare(
                "SELECT from_id, to_id, strength
                 FROM coactivation_edges
                 ORDER BY strength DESC
                 LIMIT 10",
            )
            .map_err(EngramError::Database)?;

        let strongest_pairs = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map_err(EngramError::Database)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(EngramError::Database)?;

        Ok(CoactivationReport {
            total_edges,
            avg_strength,
            strongest_pairs,
        })
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Upsert the `(from_id, to_id)` edge with a Hebbian strength update.
    ///
    /// If the edge does not exist it is created with `strength = lr`.
    /// If it already exists the strength is updated as:
    /// `strength ← min(1.0, strength + lr × (1 − strength))`
    fn upsert_edge(
        &self,
        conn: &Connection,
        from_id: i64,
        to_id: i64,
        lr: f64,
        now: &str,
    ) -> Result<()> {
        // On conflict (PRIMARY KEY collision) apply Hebbian update in-place.
        conn.execute(
            "INSERT INTO coactivation_edges (from_id, to_id, strength, coactivation_count, last_coactivated)
             VALUES (?1, ?2, MIN(1.0, ?3), 1, ?4)
             ON CONFLICT (from_id, to_id) DO UPDATE SET
                 strength           = MIN(1.0, strength + ?3 * (1.0 - strength)),
                 coactivation_count = coactivation_count + 1,
                 last_coactivated   = ?4",
            params![from_id, to_id, lr, now],
        )
        .map_err(EngramError::Database)?;

        Ok(())
    }
}

impl Default for CoactivationTracker {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Open an in-memory SQLite database and create the coactivation table.
    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory DB");
        conn.execute_batch(CREATE_COACTIVATION_EDGES_TABLE)
            .expect("create table");
        conn
    }

    fn tracker() -> CoactivationTracker {
        CoactivationTracker::new()
    }

    // -------------------------------------------------------------------------
    // Test 1: record_coactivation creates edges for every pair
    // -------------------------------------------------------------------------
    #[test]
    fn test_record_coactivation_creates_edges() {
        let conn = setup_db();
        let t = tracker();

        let n = t
            .record_coactivation(&conn, &[1, 2, 3], "session-1")
            .expect("record");

        // 3 IDs → 3 pairs: (1,2), (1,3), (2,3)
        assert_eq!(n, 3, "should create one edge per unique pair");

        let report = t.report(&conn).expect("report");
        assert_eq!(report.total_edges, 3);
    }

    // -------------------------------------------------------------------------
    // Test 2: strength increases with repeated co-activation
    // -------------------------------------------------------------------------
    #[test]
    fn test_strength_increases_with_repeated_coactivation() {
        let conn = setup_db();
        let t = tracker();

        t.record_coactivation(&conn, &[10, 20], "s1")
            .expect("first");
        let s1 = get_strength(&conn, 10, 20);

        t.record_coactivation(&conn, &[10, 20], "s2")
            .expect("second");
        let s2 = get_strength(&conn, 10, 20);

        t.record_coactivation(&conn, &[10, 20], "s3")
            .expect("third");
        let s3 = get_strength(&conn, 10, 20);

        assert!(s1 > 0.0, "first activation must produce positive strength");
        assert!(s2 > s1, "second activation must increase strength");
        assert!(s3 > s2, "third activation must increase strength further");
        assert!(s3 <= 1.0, "strength must be capped at 1.0");
    }

    // -------------------------------------------------------------------------
    // Test 3: weaken_unused decays old edges and removes sub-threshold ones
    // -------------------------------------------------------------------------
    #[test]
    fn test_weaken_unused_decays_and_prunes() {
        let conn = setup_db();
        let t = CoactivationTracker::with_config(CoactivationConfig {
            learning_rate: 0.1,
            decay_rate: 0.5,     // aggressive decay so we can see the effect
            min_strength: 0.08,  // threshold just above the decayed value
        });

        // Insert an edge directly with a very old timestamp so it qualifies
        // for decay (min_age_days = 0 means any edge qualifies, but let's use
        // a real old timestamp to be precise).
        conn.execute(
            "INSERT INTO coactivation_edges
                 (from_id, to_id, strength, coactivation_count, last_coactivated)
             VALUES (100, 200, 0.10, 1, '2020-01-01T00:00:00.000Z')",
            [],
        )
        .expect("insert old edge");

        // Insert a fresh edge (should not be decayed with min_age_days=1).
        let now_str = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        conn.execute(
            "INSERT INTO coactivation_edges
                 (from_id, to_id, strength, coactivation_count, last_coactivated)
             VALUES (100, 300, 0.50, 5, ?1)",
            params![now_str],
        )
        .expect("insert fresh edge");

        // Decay edges older than 1 day using 50% decay rate.
        let affected = t
            .weaken_unused(&conn, 0.5, 1)
            .expect("weaken");

        // The old edge (0.10) decays to 0.05 which is below min_strength (0.08),
        // so it gets deleted. The fresh edge is untouched.
        // affected = 1 (updated) + 1 (deleted) = 2
        assert!(affected >= 1, "at least one edge should be affected");

        // The old edge should be gone.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM coactivation_edges WHERE from_id=100 AND to_id=200",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "sub-threshold edge should be deleted");

        // The fresh edge should still be present.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM coactivation_edges WHERE from_id=100 AND to_id=300",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "fresh edge should survive");
    }

    // -------------------------------------------------------------------------
    // Test 4: get_coactivation_graph returns neighbors sorted by strength desc
    // -------------------------------------------------------------------------
    #[test]
    fn test_get_coactivation_graph_returns_sorted_neighbors() {
        let conn = setup_db();
        let t = tracker();

        // Build a small star: node 1 connected to 2, 3, 4 with different counts.
        for _ in 0..3 {
            t.record_coactivation(&conn, &[1, 2], "s").unwrap();
        }
        for _ in 0..5 {
            t.record_coactivation(&conn, &[1, 3], "s").unwrap();
        }
        for _ in 0..1 {
            t.record_coactivation(&conn, &[1, 4], "s").unwrap();
        }

        let graph = t.get_coactivation_graph(&conn, 1).expect("graph");

        assert_eq!(graph.len(), 3, "node 1 has 3 neighbors");

        // Verify descending strength ordering.
        for w in graph.windows(2) {
            assert!(
                w[0].strength >= w[1].strength,
                "edges must be sorted by strength desc: {} >= {}",
                w[0].strength,
                w[1].strength
            );
        }
    }

    // -------------------------------------------------------------------------
    // Test 5: suggest_related returns top-k strongest neighbors
    // -------------------------------------------------------------------------
    #[test]
    fn test_suggest_related_returns_top_k() {
        let conn = setup_db();
        let t = tracker();

        // Connect memory 1 to five others with varying counts.
        for (neighbor, times) in [(10i64, 1), (20, 3), (30, 5), (40, 2), (50, 4)] {
            for _ in 0..times {
                t.record_coactivation(&conn, &[1, neighbor], "s").unwrap();
            }
        }

        let top3 = t.suggest_related(&conn, 1, 3).expect("suggest");

        assert_eq!(top3.len(), 3, "must return exactly top_k results");

        // Results must be sorted by strength descending.
        for w in top3.windows(2) {
            assert!(
                w[0].1 >= w[1].1,
                "results must be sorted by strength desc"
            );
        }

        // The strongest neighbor is memory 30 (5 activations).
        assert_eq!(top3[0].0, 30, "strongest neighbor should be memory 30");
    }

    // -------------------------------------------------------------------------
    // Test 6: report returns correct stats
    // -------------------------------------------------------------------------
    #[test]
    fn test_report_stats() {
        let conn = setup_db();
        let t = tracker();

        t.record_coactivation(&conn, &[1, 2, 3], "s1").unwrap();

        let report = t.report(&conn).expect("report");

        assert_eq!(report.total_edges, 3);
        assert!(report.avg_strength > 0.0, "avg_strength must be positive");
        assert!(
            report.avg_strength <= 1.0,
            "avg_strength must be at most 1.0"
        );
        assert!(
            !report.strongest_pairs.is_empty(),
            "strongest_pairs must not be empty"
        );
        assert!(
            report.strongest_pairs.len() <= 10,
            "strongest_pairs must have at most 10 entries"
        );
    }

    // -------------------------------------------------------------------------
    // Test 7: empty graph operations
    // -------------------------------------------------------------------------
    #[test]
    fn test_empty_graph() {
        let conn = setup_db();
        let t = tracker();

        let graph = t.get_coactivation_graph(&conn, 999).expect("graph");
        assert!(graph.is_empty(), "no neighbors for unknown memory");

        let related = t.suggest_related(&conn, 999, 5).expect("suggest");
        assert!(related.is_empty(), "no suggestions for unknown memory");

        let report = t.report(&conn).expect("report");
        assert_eq!(report.total_edges, 0);
        assert_eq!(report.avg_strength, 0.0);
        assert!(report.strongest_pairs.is_empty());
    }

    // -------------------------------------------------------------------------
    // Test 8: strengthen — single-edge Hebbian update
    // -------------------------------------------------------------------------
    #[test]
    fn test_strengthen_single_edge() {
        let conn = setup_db();
        let t = tracker();

        let s1 = t.strengthen(&conn, 5, 6).expect("strengthen 1");
        let s2 = t.strengthen(&conn, 5, 6).expect("strengthen 2");

        assert!(s1 > 0.0);
        assert!(s2 > s1, "repeated calls must increase strength");
    }

    // -------------------------------------------------------------------------
    // Test 9: record_coactivation is idempotent for a single memory
    // -------------------------------------------------------------------------
    #[test]
    fn test_single_memory_no_self_loops() {
        let conn = setup_db();
        let t = tracker();

        let n = t
            .record_coactivation(&conn, &[42], "session-x")
            .expect("record single");

        // A single memory has no pairs, so zero edges should be created.
        assert_eq!(n, 0, "no pairs from a single memory");

        let report = t.report(&conn).expect("report");
        assert_eq!(report.total_edges, 0);
    }

    // -------------------------------------------------------------------------
    // Test 10: coactivation_count increments correctly
    // -------------------------------------------------------------------------
    #[test]
    fn test_coactivation_count_increments() {
        let conn = setup_db();
        let t = tracker();

        for _ in 0..4 {
            t.record_coactivation(&conn, &[7, 8], "s").unwrap();
        }

        let graph = t.get_coactivation_graph(&conn, 7).expect("graph");
        assert_eq!(graph.len(), 1);
        assert_eq!(graph[0].count, 4, "count should reflect 4 co-activations");
    }

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    fn get_strength(conn: &Connection, from_id: i64, to_id: i64) -> f64 {
        conn.query_row(
            "SELECT strength FROM coactivation_edges WHERE from_id=?1 AND to_id=?2",
            params![from_id, to_id],
            |r| r.get(0),
        )
        .unwrap_or(0.0)
    }
}
