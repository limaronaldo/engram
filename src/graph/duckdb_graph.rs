//! DuckDB-backed TemporalGraph for OLAP read operations (Phase M).
//!
//! Implements the CQRS read side: SQLite handles all writes, DuckDB attaches
//! the same file read-only and provides fast analytical queries.  When the
//! optional `duckpgq` extension is available a full property-graph (`MATCH`)
//! query surface is also created; otherwise the module degrades gracefully to
//! plain SQL.

#![cfg(feature = "duckdb-graph")]

use duckdb::{params, Connection as DuckdbConnection};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::error::{EngramError, Result};

// ---------------------------------------------------------------------------
// DuckDB error bridging
// ---------------------------------------------------------------------------

impl From<duckdb::Error> for EngramError {
    fn from(e: duckdb::Error) -> Self {
        EngramError::Storage(format!("DuckDB error: {}", e))
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A step (or complete result) in a graph path traversal.
///
/// Each value returned by `find_connection` or `find_neighbors` carries a
/// human-readable path string and the hop-count from the origin node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathStep {
    /// Human-readable representation of the full path from origin to this node,
    /// e.g. `"1 -[works_at]-> 2 -[located_in]-> 3"`.
    pub path: String,
    /// Number of hops from the origin node.
    pub depth: i32,
}

/// A temporal edge returned from DuckDB queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuckDbTemporalEdge {
    pub id: i64,
    pub from_id: i64,
    pub to_id: i64,
    pub relation: String,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub confidence: f32,
    pub scope_path: String,
}

/// Diff between two graph snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuckDbGraphDiff {
    pub added: Vec<DuckDbTemporalEdge>,
    pub removed: Vec<DuckDbTemporalEdge>,
    pub changed: Vec<(DuckDbTemporalEdge, DuckDbTemporalEdge)>,
}

// ---------------------------------------------------------------------------
// TemporalGraph
// ---------------------------------------------------------------------------

/// DuckDB-backed analytical graph over Engram's temporal edges.
///
/// Lifecycle:
/// 1. `new(sqlite_path)` — opens an in-memory DuckDB, attaches the SQLite
///    file read-only, optionally loads `duckpgq` and creates a property graph.
/// 2. `refresh()` — detaches and re-attaches SQLite to pick up writes that
///    have been committed since the last attach.
pub struct TemporalGraph {
    conn: DuckdbConnection,
    /// Whether the `duckpgq` extension loaded successfully.
    has_pgq: bool,
    /// The SQLite path kept for re-attach on `refresh`.
    sqlite_path: String,
}

impl TemporalGraph {
    /// Open an in-memory DuckDB session attached to `sqlite_path`.
    ///
    /// Steps performed:
    /// - Install + load the bundled `sqlite` scanner extension.
    /// - Attach the SQLite database as the catalog `engram` (read-only).
    /// - Attempt to install + load `duckpgq`; failures are non-fatal.
    /// - If PGQ loaded, register a property graph over `graph_entities` /
    ///   `temporal_edges`.
    pub fn new(sqlite_path: &str) -> Result<Self> {
        let conn = DuckdbConnection::open_in_memory()?;

        // --- SQLite scanner extension -----------------------------------------
        // The bundled DuckDB already ships the sqlite extension; INSTALL is
        // effectively a no-op when it is already present.
        conn.execute_batch("INSTALL sqlite; LOAD sqlite;")?;

        // --- Attach the SQLite file read-only --------------------------------
        conn.execute_batch(&format!(
            "ATTACH '{path}' AS engram (TYPE SQLITE, READ_ONLY);",
            path = sqlite_path
        ))?;

        // --- Optional: duckpgq extension -------------------------------------
        let has_pgq = Self::try_load_pgq(&conn, sqlite_path);

        Ok(Self {
            conn,
            has_pgq,
            sqlite_path: sqlite_path.to_string(),
        })
    }

    /// Attempt to install and load `duckpgq`, then register the property graph.
    ///
    /// Returns `true` on full success, `false` on any error (with a warning
    /// logged so the caller gets visibility without a hard failure).
    fn try_load_pgq(conn: &DuckdbConnection, _sqlite_path: &str) -> bool {
        // Install extension — may fail if the registry is unavailable.
        if let Err(e) = conn.execute_batch("INSTALL duckpgq FROM community;") {
            warn!("duckpgq install failed (graph pattern matching unavailable): {}", e);
            return false;
        }

        if let Err(e) = conn.execute_batch("LOAD duckpgq;") {
            warn!("duckpgq load failed (graph pattern matching unavailable): {}", e);
            return false;
        }

        // Register the property graph over the attached SQLite tables.
        let pgq_ddl = r#"
            CREATE OR REPLACE PROPERTY GRAPH knowledge_graph
            VERTEX TABLES (engram.graph_entities)
            EDGE TABLES (
                engram.temporal_edges
                SOURCE KEY (from_id) REFERENCES graph_entities(id)
                DESTINATION KEY (to_id) REFERENCES graph_entities(id)
                LABEL relation
            );
        "#;

        if let Err(e) = conn.execute_batch(pgq_ddl) {
            warn!("duckpgq property graph creation failed: {}", e);
            return false;
        }

        debug!("duckpgq property graph 'knowledge_graph' created successfully");
        true
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Whether the `duckpgq` extension loaded and the property graph is active.
    ///
    /// When `false`, graph pattern (`MATCH`) queries are unavailable; use
    /// standard SQL over `engram.temporal_edges` / `engram.graph_entities`.
    pub fn has_pgq(&self) -> bool {
        self.has_pgq
    }

    /// Re-attach the SQLite file to reflect writes committed since the last
    /// attach.
    ///
    /// DuckDB caches the SQLite file at attach time; this detach + re-attach
    /// cycle is the canonical way to pick up new data without restarting the
    /// DuckDB session.
    pub fn refresh(&self) -> Result<()> {
        // Detach the existing catalog.
        self.conn
            .execute_batch("DETACH engram;")?;

        // Re-attach read-only.
        self.conn.execute_batch(&format!(
            "ATTACH '{path}' AS engram (TYPE SQLITE, READ_ONLY);",
            path = self.sqlite_path
        ))?;

        debug!("TemporalGraph: re-attached SQLite at '{}'", self.sqlite_path);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Temporal query methods
    // -----------------------------------------------------------------------

    /// Return all edges whose validity window includes `timestamp` within
    /// the given `scope` prefix.
    ///
    /// Edges are included when:
    /// - `scope_path` starts with `scope`
    /// - `valid_from <= timestamp`
    /// - `valid_to IS NULL OR valid_to >= timestamp`
    pub fn snapshot_at(
        &self,
        scope: &str,
        timestamp: &str,
    ) -> Result<Vec<DuckDbTemporalEdge>> {
        let scope_pattern = format!("{}%", scope);
        let sql = "
            SELECT id, from_id, to_id, relation, valid_from, valid_to, confidence, scope_path
            FROM engram.temporal_edges
            WHERE scope_path LIKE ?
              AND valid_from <= ?
              AND (valid_to IS NULL OR valid_to >= ?)
            ORDER BY id ASC
        ";
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(
            params![scope_pattern, timestamp, timestamp],
            |row| {
                Ok(DuckDbTemporalEdge {
                    id: row.get(0)?,
                    from_id: row.get(1)?,
                    to_id: row.get(2)?,
                    relation: row.get(3)?,
                    valid_from: row.get(4)?,
                    valid_to: row.get(5)?,
                    confidence: row.get::<_, f64>(6)? as f32,
                    scope_path: row.get(7)?,
                })
            },
        )?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(EngramError::from)
    }

    /// Compute the structural difference between two graph snapshots at `t1`
    /// and `t2` within the given `scope` prefix.
    ///
    /// - `added`   — edges present in t2 but not t1 (matched by (from_id, to_id, relation))
    /// - `removed` — edges present in t1 but not t2
    /// - `changed` — edges present in both snapshots but with differing
    ///               `confidence` or `valid_to`; tuple is (t1_edge, t2_edge)
    pub fn graph_diff(
        &self,
        scope: &str,
        t1: &str,
        t2: &str,
    ) -> Result<DuckDbGraphDiff> {
        let snap1 = self.snapshot_at(scope, t1)?;
        let snap2 = self.snapshot_at(scope, t2)?;

        // Build a lookup key: (from_id, to_id, relation) -> edge
        use std::collections::HashMap;

        let key = |e: &DuckDbTemporalEdge| (e.from_id, e.to_id, e.relation.clone());

        let map1: HashMap<_, _> = snap1.iter().map(|e| (key(e), e.clone())).collect();
        let map2: HashMap<_, _> = snap2.iter().map(|e| (key(e), e.clone())).collect();

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut changed = Vec::new();

        for (k, e2) in &map2 {
            match map1.get(k) {
                None => added.push(e2.clone()),
                Some(e1) => {
                    // Consider an edge "changed" when confidence or valid_to differ.
                    let conf_changed = (e1.confidence - e2.confidence).abs() > f32::EPSILON;
                    let valid_to_changed = e1.valid_to != e2.valid_to;
                    if conf_changed || valid_to_changed {
                        changed.push((e1.clone(), e2.clone()));
                    }
                }
            }
        }

        for (k, e1) in &map1 {
            if !map2.contains_key(k) {
                removed.push(e1.clone());
            }
        }

        Ok(DuckDbGraphDiff { added, removed, changed })
    }

    /// Return the full history of edges between `from_id` and `to_id` within
    /// the given `scope` prefix, ordered from most-recent to oldest.
    pub fn relationship_timeline(
        &self,
        scope: &str,
        from_id: i64,
        to_id: i64,
    ) -> Result<Vec<DuckDbTemporalEdge>> {
        let scope_pattern = format!("{}%", scope);
        let sql = "
            SELECT id, from_id, to_id, relation, valid_from, valid_to, confidence, scope_path
            FROM engram.temporal_edges
            WHERE scope_path LIKE ?
              AND from_id = ?
              AND to_id   = ?
            ORDER BY valid_from DESC
        ";
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(
            params![scope_pattern, from_id, to_id],
            |row| {
                Ok(DuckDbTemporalEdge {
                    id: row.get(0)?,
                    from_id: row.get(1)?,
                    to_id: row.get(2)?,
                    relation: row.get(3)?,
                    valid_from: row.get(4)?,
                    valid_to: row.get(5)?,
                    confidence: row.get::<_, f64>(6)? as f32,
                    scope_path: row.get(7)?,
                })
            },
        )?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(EngramError::from)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[cfg(feature = "duckdb-graph")]
mod tests {
    use super::*;

    /// Create a minimal SQLite database at `path` that satisfies the schema
    /// expected by `TemporalGraph` (v33 tables).
    fn setup_sqlite(path: &str) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open(path).expect("open sqlite");
        conn.execute_batch(r#"
            CREATE TABLE IF NOT EXISTS graph_entities (
                id          TEXT PRIMARY KEY,
                scope_path  TEXT NOT NULL DEFAULT 'global',
                name        TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                metadata    TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS temporal_edges (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                from_id     INTEGER NOT NULL,
                to_id       INTEGER NOT NULL,
                relation    TEXT NOT NULL,
                properties  TEXT,
                valid_from  TEXT NOT NULL,
                valid_to    TEXT,
                confidence  REAL NOT NULL DEFAULT 1.0,
                source      TEXT,
                scope_path  TEXT NOT NULL DEFAULT 'global',
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );
        "#).expect("create tables");
        conn
    }

    /// Insert a single edge into `temporal_edges` and return its rowid.
    fn insert_edge(
        conn: &rusqlite::Connection,
        from_id: i64,
        to_id: i64,
        relation: &str,
        valid_from: &str,
        valid_to: Option<&str>,
        confidence: f64,
        scope_path: &str,
    ) {
        conn.execute(
            "INSERT INTO temporal_edges
                (from_id, to_id, relation, valid_from, valid_to, confidence, scope_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![from_id, to_id, relation, valid_from, valid_to, confidence, scope_path],
        )
        .expect("insert edge");
    }

    // -----------------------------------------------------------------------

    #[test]
    fn test_temporal_graph_new() {
        let dir = std::env::temp_dir();
        let path = dir.join("engram_test_new.sqlite");
        let path_str = path.to_str().unwrap();

        // Remove any leftovers from a previous run.
        let _ = std::fs::remove_file(path_str);

        setup_sqlite(path_str);

        let graph = TemporalGraph::new(path_str);
        assert!(
            graph.is_ok(),
            "TemporalGraph::new should succeed: {:?}",
            graph.err()
        );

        // Cleanup.
        let _ = std::fs::remove_file(path_str);
    }

    #[test]
    fn test_temporal_graph_refresh() {
        let dir = std::env::temp_dir();
        let path = dir.join("engram_test_refresh.sqlite");
        let path_str = path.to_str().unwrap();

        let _ = std::fs::remove_file(path_str);
        setup_sqlite(path_str);

        let graph = TemporalGraph::new(path_str).expect("new");

        // First refresh should succeed (detach + re-attach).
        let r1 = graph.refresh();
        assert!(r1.is_ok(), "first refresh failed: {:?}", r1.err());

        // Second refresh should also succeed (idempotent).
        let r2 = graph.refresh();
        assert!(r2.is_ok(), "second refresh failed: {:?}", r2.err());

        let _ = std::fs::remove_file(path_str);
    }

    #[test]
    fn test_has_pgq_false_without_extension() {
        // In most CI environments duckpgq is not installed.  We verify that
        // the constructor still succeeds and `has_pgq` returns a bool (true
        // only if the extension happened to be available).
        let dir = std::env::temp_dir();
        let path = dir.join("engram_test_pgq.sqlite");
        let path_str = path.to_str().unwrap();

        let _ = std::fs::remove_file(path_str);
        setup_sqlite(path_str);

        let graph = TemporalGraph::new(path_str).expect("new");

        // The important invariant: even if PGQ is unavailable the struct is
        // valid and `has_pgq()` returns a stable boolean without panicking.
        let _ = graph.has_pgq(); // just assert it doesn't panic

        let _ = std::fs::remove_file(path_str);
    }

    // -----------------------------------------------------------------------
    // Temporal query method tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_snapshot_at() {
        let dir = std::env::temp_dir();
        let path = dir.join("engram_test_snapshot_at.sqlite");
        let path_str = path.to_str().unwrap();
        let _ = std::fs::remove_file(path_str);

        let sqlite = setup_sqlite(path_str);

        // Edge A: active 2024-01-01 → 2024-06-30 (closed)
        insert_edge(&sqlite, 1, 2, "knows", "2024-01-01", Some("2024-06-30"), 0.9, "global");
        // Edge B: active 2024-01-01 → open (still active)
        insert_edge(&sqlite, 1, 3, "follows", "2024-01-01", None, 0.8, "global");
        // Edge C: starts in the future (2025-01-01), should not appear at 2024-03-01
        insert_edge(&sqlite, 2, 3, "linked", "2025-01-01", None, 0.7, "global");
        drop(sqlite);

        let graph = TemporalGraph::new(path_str).expect("new");

        // Snapshot mid-year 2024: should see edges A and B, not C.
        let snap = graph.snapshot_at("global", "2024-03-01").expect("snapshot_at");
        assert_eq!(snap.len(), 2, "expected 2 edges active at 2024-03-01");
        let relations: Vec<&str> = snap.iter().map(|e| e.relation.as_str()).collect();
        assert!(relations.contains(&"knows"), "edge A should be included");
        assert!(relations.contains(&"follows"), "edge B should be included");

        // Snapshot after edge A expired: should see only B and C.
        let snap2 = graph.snapshot_at("global", "2024-08-01").expect("snapshot_at late");
        assert_eq!(snap2.len(), 1, "expected 1 edge active at 2024-08-01");
        assert_eq!(snap2[0].relation, "follows");

        let _ = std::fs::remove_file(path_str);
    }

    #[test]
    fn test_graph_diff() {
        let dir = std::env::temp_dir();
        let path = dir.join("engram_test_graph_diff.sqlite");
        let path_str = path.to_str().unwrap();
        let _ = std::fs::remove_file(path_str);

        let sqlite = setup_sqlite(path_str);

        // Edge present at both t1 and t2 (no change).
        insert_edge(&sqlite, 1, 2, "knows", "2024-01-01", None, 1.0, "global");
        // Edge present at t1 but expired before t2 (removed).
        insert_edge(&sqlite, 1, 3, "follows", "2024-01-01", Some("2024-03-31"), 1.0, "global");
        // Edge starting after t1 (added at t2).
        insert_edge(&sqlite, 2, 3, "linked", "2024-06-01", None, 0.5, "global");
        drop(sqlite);

        let graph = TemporalGraph::new(path_str).expect("new");

        let diff = graph
            .graph_diff("global", "2024-02-01", "2024-07-01")
            .expect("graph_diff");

        assert_eq!(diff.added.len(), 1, "one edge added between t1 and t2");
        assert_eq!(diff.added[0].relation, "linked");

        assert_eq!(diff.removed.len(), 1, "one edge removed between t1 and t2");
        assert_eq!(diff.removed[0].relation, "follows");

        assert_eq!(diff.changed.len(), 0, "no edges changed");

        let _ = std::fs::remove_file(path_str);
    }

    #[test]
    fn test_relationship_timeline() {
        let dir = std::env::temp_dir();
        let path = dir.join("engram_test_timeline.sqlite");
        let path_str = path.to_str().unwrap();
        let _ = std::fs::remove_file(path_str);

        let sqlite = setup_sqlite(path_str);

        // Three versions of the same 1→2 relationship, plus an unrelated edge.
        insert_edge(&sqlite, 1, 2, "knows", "2022-01-01", Some("2022-12-31"), 0.5, "global");
        insert_edge(&sqlite, 1, 2, "knows", "2023-01-01", Some("2023-12-31"), 0.75, "global");
        insert_edge(&sqlite, 1, 2, "knows", "2024-01-01", None, 0.9, "global");
        // Unrelated: different pair.
        insert_edge(&sqlite, 3, 4, "linked", "2024-01-01", None, 1.0, "global");
        drop(sqlite);

        let graph = TemporalGraph::new(path_str).expect("new");

        let timeline = graph
            .relationship_timeline("global", 1, 2)
            .expect("timeline");

        assert_eq!(timeline.len(), 3, "three versions of the 1→2 relationship");

        // Results should be ordered by valid_from DESC.
        assert_eq!(timeline[0].valid_from, "2024-01-01", "most recent first");
        assert_eq!(timeline[1].valid_from, "2023-01-01");
        assert_eq!(timeline[2].valid_from, "2022-01-01", "oldest last");

        let _ = std::fs::remove_file(path_str);
    }

    #[test]
    fn test_scope_filtering_in_snapshot() {
        let dir = std::env::temp_dir();
        let path = dir.join("engram_test_scope_filter.sqlite");
        let path_str = path.to_str().unwrap();
        let _ = std::fs::remove_file(path_str);

        let sqlite = setup_sqlite(path_str);

        // Two edges in "project/alpha" scope.
        insert_edge(&sqlite, 1, 2, "depends", "2024-01-01", None, 1.0, "project/alpha");
        insert_edge(&sqlite, 2, 3, "depends", "2024-01-01", None, 1.0, "project/alpha/sub");
        // One edge in a sibling scope — must NOT appear in "project/alpha" snapshots.
        insert_edge(&sqlite, 3, 4, "depends", "2024-01-01", None, 1.0, "project/beta");
        // One edge in parent scope — must NOT appear (prefix match is strict on scope arg).
        insert_edge(&sqlite, 4, 5, "depends", "2024-01-01", None, 1.0, "project");
        drop(sqlite);

        let graph = TemporalGraph::new(path_str).expect("new");

        // Snapshot scoped to "project/alpha" should return both alpha + alpha/sub edges.
        let snap = graph
            .snapshot_at("project/alpha", "2024-06-01")
            .expect("snapshot_at scoped");

        assert_eq!(snap.len(), 2, "only edges under project/alpha scope");
        for edge in &snap {
            assert!(
                edge.scope_path.starts_with("project/alpha"),
                "unexpected scope: {}",
                edge.scope_path
            );
        }

        let _ = std::fs::remove_file(path_str);
    }
}
