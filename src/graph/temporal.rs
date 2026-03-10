//! Temporal knowledge graph — edges with validity periods.
//!
//! Provides bi-temporal edge tracking: each edge carries a `valid_from` /
//! `valid_to` validity interval. Adding a new edge for the same
//! `(from_id, to_id, relation)` triple automatically closes the previous open
//! interval so the graph stays consistent.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{EngramError, Result};

// =============================================================================
// DDL
// =============================================================================

/// SQL that creates the `temporal_edges` table and its supporting indexes.
///
/// Safe to run on an existing database — all statements use `IF NOT EXISTS`.
///
/// Note: the `scope_path` column was added in migration v33. This constant
/// reflects the canonical schema; production databases gain the column via
/// the migration runner.
pub const CREATE_TEMPORAL_EDGES_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS temporal_edges (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    from_id     INTEGER NOT NULL,
    to_id       INTEGER NOT NULL,
    relation    TEXT    NOT NULL,
    properties  TEXT    NOT NULL DEFAULT '{}',
    valid_from  TEXT    NOT NULL,
    valid_to    TEXT,
    confidence  REAL    NOT NULL DEFAULT 1.0,
    source      TEXT    NOT NULL DEFAULT '',
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    scope_path  TEXT    NOT NULL DEFAULT 'global'
);
CREATE INDEX IF NOT EXISTS idx_temporal_edges_from       ON temporal_edges(from_id);
CREATE INDEX IF NOT EXISTS idx_temporal_edges_to         ON temporal_edges(to_id);
CREATE INDEX IF NOT EXISTS idx_temporal_edges_valid      ON temporal_edges(valid_from, valid_to);
CREATE INDEX IF NOT EXISTS idx_temporal_edges_scope_path ON temporal_edges(scope_path);
"#;

// =============================================================================
// Types
// =============================================================================

/// A directed edge in the temporal knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalEdge {
    /// Row identifier.
    pub id: i64,
    /// Source memory / node.
    pub from_id: i64,
    /// Target memory / node.
    pub to_id: i64,
    /// Semantic label for the relationship (e.g. `"works_at"`, `"reports_to"`).
    pub relation: String,
    /// Arbitrary key-value metadata stored as JSON.
    pub properties: Value,
    /// Start of validity period (RFC3339 UTC).
    pub valid_from: String,
    /// End of validity period (RFC3339 UTC), `None` means still valid.
    pub valid_to: Option<String>,
    /// Confidence in this edge (0.0–1.0).
    pub confidence: f32,
    /// Provenance string (e.g. document name, agent ID).
    pub source: String,
    /// Wall-clock creation time (RFC3339 UTC).
    pub created_at: String,
    /// Hierarchical scope path (e.g. `"global"`, `"global/org:acme/user:alice"`).
    /// Added in schema v33. Defaults to `"global"` for backward compatibility.
    pub scope_path: String,
}

/// Summary of how the graph changed between two timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphDiff {
    /// Edges present at `t2` but not at `t1`.
    pub added: Vec<TemporalEdge>,
    /// Edges present at `t1` but not at `t2`.
    pub removed: Vec<TemporalEdge>,
    /// Edges whose properties or confidence changed between `t1` and `t2`.
    ///
    /// Each tuple is `(old_edge_at_t1, new_edge_at_t2)`.
    pub changed: Vec<(TemporalEdge, TemporalEdge)>,
}

// =============================================================================
// Row mapper helpers
// =============================================================================

/// Build a `TemporalEdge` from a rusqlite row.
///
/// Expected column order:
/// 0: id, 1: from_id, 2: to_id, 3: properties, 4: valid_from, 5: valid_to,
/// 6: confidence, 7: source, 8: relation, 9: created_at, 10: scope_path
fn row_to_edge(row: &rusqlite::Row<'_>) -> rusqlite::Result<TemporalEdge> {
    let props_str: String = row.get(3)?;
    let properties: Value =
        serde_json::from_str(&props_str).unwrap_or(Value::Object(Default::default()));

    Ok(TemporalEdge {
        id: row.get(0)?,
        from_id: row.get(1)?,
        to_id: row.get(2)?,
        relation: row.get(8)?,
        properties,
        valid_from: row.get(4)?,
        valid_to: row.get(5)?,
        confidence: row.get(6)?,
        source: row.get(7)?,
        created_at: row.get(9)?,
        scope_path: row.get(10)?,
    })
}

// =============================================================================
// Public API
// =============================================================================

/// Add a new temporal edge.
///
/// If an open edge (`valid_to IS NULL`) already exists for the same
/// `(from_id, to_id, relation)` triple **within the same scope**, it is
/// automatically closed by setting its `valid_to` to the `valid_from` of the
/// new edge before inserting.
///
/// `scope_path` defaults to `"global"` when `None`.
///
/// Returns the newly inserted edge with its generated `id` and `created_at`.
pub fn add_edge(
    conn: &Connection,
    from_id: i64,
    to_id: i64,
    relation: &str,
    properties: &Value,
    valid_from: &str,
    confidence: f32,
    source: &str,
    scope_path: Option<&str>,
) -> Result<TemporalEdge> {
    let scope = scope_path.unwrap_or("global");
    let props_str = serde_json::to_string(properties)?;

    // Auto-invalidate any currently-open edges for the same triple within
    // the same scope.
    conn.execute(
        "UPDATE temporal_edges
         SET valid_to = ?1
         WHERE from_id    = ?2
           AND to_id      = ?3
           AND relation   = ?4
           AND scope_path = ?5
           AND valid_to IS NULL",
        params![valid_from, from_id, to_id, relation, scope],
    )
    .map_err(EngramError::Database)?;

    // Insert the new edge.
    conn.execute(
        "INSERT INTO temporal_edges
             (from_id, to_id, relation, properties, valid_from, confidence, source, scope_path)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![from_id, to_id, relation, props_str, valid_from, confidence, source, scope],
    )
    .map_err(EngramError::Database)?;

    let id = conn.last_insert_rowid();
    get_edge_by_id(conn, id)?
        .ok_or_else(|| EngramError::Internal(format!("Edge {} disappeared after insert", id)))
}

/// Set the `valid_to` timestamp on an existing edge, effectively closing it.
pub fn invalidate_edge(conn: &Connection, edge_id: i64, valid_to: &str) -> Result<()> {
    let affected = conn
        .execute(
            "UPDATE temporal_edges SET valid_to = ?1 WHERE id = ?2",
            params![valid_to, edge_id],
        )
        .map_err(EngramError::Database)?;

    if affected == 0 {
        return Err(EngramError::NotFound(edge_id));
    }
    Ok(())
}

/// Return all edges that were valid at `timestamp`.
///
/// An edge is valid at `t` when `valid_from <= t` AND (`valid_to IS NULL` OR
/// `valid_to > t`).
///
/// When `scope_path` is `Some(prefix)`, only edges whose `scope_path` equals
/// `prefix` or starts with `prefix/` are returned (hierarchical prefix
/// matching). When `None`, edges from all scopes are returned (backward
/// compatible).
pub fn snapshot_at(
    conn: &Connection,
    timestamp: &str,
    scope_path: Option<&str>,
) -> Result<Vec<TemporalEdge>> {
    match scope_path {
        None => {
            let mut stmt = conn
                .prepare(
                    "SELECT id, from_id, to_id, properties, valid_from, valid_to,
                            confidence, source, relation, created_at, scope_path
                     FROM   temporal_edges
                     WHERE  valid_from <= ?1
                       AND  (valid_to IS NULL OR valid_to > ?1)
                     ORDER  BY from_id, to_id, relation",
                )
                .map_err(EngramError::Database)?;

            let edges = stmt
                .query_map(params![timestamp], row_to_edge)
                .map_err(EngramError::Database)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(EngramError::Database)?;

            Ok(edges)
        }
        Some(scope) => {
            let pattern = format!("{}/%", scope);
            let mut stmt = conn
                .prepare(
                    "SELECT id, from_id, to_id, properties, valid_from, valid_to,
                            confidence, source, relation, created_at, scope_path
                     FROM   temporal_edges
                     WHERE  valid_from <= ?1
                       AND  (valid_to IS NULL OR valid_to > ?1)
                       AND  (scope_path = ?2 OR scope_path LIKE ?3)
                     ORDER  BY from_id, to_id, relation",
                )
                .map_err(EngramError::Database)?;

            let edges = stmt
                .query_map(params![timestamp, scope, pattern], row_to_edge)
                .map_err(EngramError::Database)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(EngramError::Database)?;

            Ok(edges)
        }
    }
}

/// Return the complete edit history for a `(from_id, to_id)` pair, ordered
/// chronologically (`valid_from ASC`, then `created_at ASC`).
///
/// When `scope_path` is `Some(prefix)`, only edges whose `scope_path` equals
/// `prefix` or starts with `prefix/` are returned. When `None`, all scopes
/// are included (backward compatible).
pub fn relationship_timeline(
    conn: &Connection,
    from_id: i64,
    to_id: i64,
    scope_path: Option<&str>,
) -> Result<Vec<TemporalEdge>> {
    match scope_path {
        None => {
            let mut stmt = conn
                .prepare(
                    "SELECT id, from_id, to_id, properties, valid_from, valid_to,
                            confidence, source, relation, created_at, scope_path
                     FROM   temporal_edges
                     WHERE  from_id = ?1 AND to_id = ?2
                     ORDER  BY valid_from ASC, created_at ASC",
                )
                .map_err(EngramError::Database)?;

            let edges = stmt
                .query_map(params![from_id, to_id], row_to_edge)
                .map_err(EngramError::Database)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(EngramError::Database)?;

            Ok(edges)
        }
        Some(scope) => {
            let pattern = format!("{}/%", scope);
            let mut stmt = conn
                .prepare(
                    "SELECT id, from_id, to_id, properties, valid_from, valid_to,
                            confidence, source, relation, created_at, scope_path
                     FROM   temporal_edges
                     WHERE  from_id    = ?1
                       AND  to_id      = ?2
                       AND  (scope_path = ?3 OR scope_path LIKE ?4)
                     ORDER  BY valid_from ASC, created_at ASC",
                )
                .map_err(EngramError::Database)?;

            let edges = stmt
                .query_map(params![from_id, to_id, scope, pattern], row_to_edge)
                .map_err(EngramError::Database)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(EngramError::Database)?;

            Ok(edges)
        }
    }
}

/// Detect edges that share the same `(from_id, to_id, relation)` triple and
/// have **overlapping** validity periods — which should not exist under normal
/// operation.
///
/// Returns pairs `(edge_a, edge_b)` where `edge_a.id < edge_b.id`.
pub fn detect_contradictions(conn: &Connection) -> Result<Vec<(TemporalEdge, TemporalEdge)>> {
    // Self-join: find pairs that share the triple and overlap.
    // Overlap condition: a.valid_from < b.valid_to_or_max AND b.valid_from < a.valid_to_or_max
    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.from_id, a.to_id, a.properties, a.valid_from, a.valid_to,
                    a.confidence, a.source, a.relation, a.created_at, a.scope_path,
                    b.id, b.from_id, b.to_id, b.properties, b.valid_from, b.valid_to,
                    b.confidence, b.source, b.relation, b.created_at, b.scope_path
             FROM   temporal_edges a
             JOIN   temporal_edges b
               ON   a.from_id  = b.from_id
              AND   a.to_id    = b.to_id
              AND   a.relation = b.relation
              AND   a.id < b.id
             WHERE  a.valid_from < COALESCE(b.valid_to, '9999-12-31T23:59:59Z')
               AND  b.valid_from < COALESCE(a.valid_to, '9999-12-31T23:59:59Z')",
        )
        .map_err(EngramError::Database)?;

    let pairs = stmt
        .query_map([], |row| {
            // First edge columns: 0..10
            let props_a: String = row.get(3)?;
            // Second edge columns: 11..21
            let props_b: String = row.get(14)?;

            let edge_a = TemporalEdge {
                id: row.get(0)?,
                from_id: row.get(1)?,
                to_id: row.get(2)?,
                properties: serde_json::from_str(&props_a)
                    .unwrap_or(Value::Object(Default::default())),
                valid_from: row.get(4)?,
                valid_to: row.get(5)?,
                confidence: row.get(6)?,
                source: row.get(7)?,
                relation: row.get(8)?,
                created_at: row.get(9)?,
                scope_path: row.get(10)?,
            };

            let edge_b = TemporalEdge {
                id: row.get(11)?,
                from_id: row.get(12)?,
                to_id: row.get(13)?,
                properties: serde_json::from_str(&props_b)
                    .unwrap_or(Value::Object(Default::default())),
                valid_from: row.get(15)?,
                valid_to: row.get(16)?,
                confidence: row.get(17)?,
                source: row.get(18)?,
                relation: row.get(19)?,
                created_at: row.get(20)?,
                scope_path: row.get(21)?,
            };

            Ok((edge_a, edge_b))
        })
        .map_err(EngramError::Database)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(EngramError::Database)?;

    Ok(pairs)
}

/// Compare the graph state at two different timestamps.
///
/// - `added`   — edges valid at `t2` whose `(from_id, to_id, relation)` triple
///   was not present at `t1`.
/// - `removed` — edges valid at `t1` whose triple was not present at `t2`.
/// - `changed` — triples present at both `t1` and `t2` but with a different
///   `id` (i.e. the edge was superseded), implying the properties
///   or confidence changed.
///
/// When `scope_path` is `Some(prefix)`, the diff is limited to edges within
/// that scope hierarchy. When `None`, all scopes are compared (backward
/// compatible).
pub fn diff(
    conn: &Connection,
    t1: &str,
    t2: &str,
    scope_path: Option<&str>,
) -> Result<GraphDiff> {
    let snap1 = snapshot_at(conn, t1, scope_path)?;
    let snap2 = snapshot_at(conn, t2, scope_path)?;

    // Key: (from_id, to_id, relation)
    type Key = (i64, i64, String);

    let map1: std::collections::HashMap<Key, TemporalEdge> = snap1
        .into_iter()
        .map(|e| ((e.from_id, e.to_id, e.relation.clone()), e))
        .collect();

    let map2: std::collections::HashMap<Key, TemporalEdge> = snap2
        .into_iter()
        .map(|e| ((e.from_id, e.to_id, e.relation.clone()), e))
        .collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for (key, edge2) in &map2 {
        match map1.get(key) {
            None => added.push(edge2.clone()),
            Some(edge1) if edge1.id != edge2.id => {
                changed.push((edge1.clone(), edge2.clone()));
            }
            _ => {} // same edge, no change
        }
    }

    for (key, edge1) in &map1 {
        if !map2.contains_key(key) {
            removed.push(edge1.clone());
        }
    }

    Ok(GraphDiff {
        added,
        removed,
        changed,
    })
}

// =============================================================================
// Private helpers
// =============================================================================

fn get_edge_by_id(conn: &Connection, id: i64) -> Result<Option<TemporalEdge>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, from_id, to_id, properties, valid_from, valid_to,
                    confidence, source, relation, created_at, scope_path
             FROM   temporal_edges
             WHERE  id = ?1",
        )
        .map_err(EngramError::Database)?;

    let mut rows = stmt
        .query_map(params![id], row_to_edge)
        .map_err(EngramError::Database)?;

    match rows.next() {
        Some(row) => Ok(Some(row.map_err(EngramError::Database)?)),
        None => Ok(None),
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::json;

    /// Open an in-memory SQLite database and create the temporal_edges table.
    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory DB");
        conn.execute_batch(CREATE_TEMPORAL_EDGES_TABLE)
            .expect("create table");
        conn
    }

    // -------------------------------------------------------------------------
    // Test 1: Add edge and retrieve it
    // -------------------------------------------------------------------------
    #[test]
    fn test_add_edge_and_retrieve() {
        let conn = setup_db();

        let edge = add_edge(
            &conn,
            1,
            2,
            "works_at",
            &json!({}),
            "2024-01-01T00:00:00Z",
            0.9,
            "test",
            None,
        )
        .expect("add_edge");

        assert_eq!(edge.from_id, 1);
        assert_eq!(edge.to_id, 2);
        assert_eq!(edge.relation, "works_at");
        assert!(edge.valid_to.is_none());
        assert_eq!(edge.confidence, 0.9);
        assert_eq!(edge.source, "test");
        assert_eq!(edge.scope_path, "global");
    }

    // -------------------------------------------------------------------------
    // Test 2: Auto-invalidation of conflicting edges
    // -------------------------------------------------------------------------
    #[test]
    fn test_auto_invalidation_on_new_edge() {
        let conn = setup_db();

        let first = add_edge(
            &conn,
            1,
            2,
            "works_at",
            &json!({"role": "engineer"}),
            "2023-01-01T00:00:00Z",
            1.0,
            "hr",
            None,
        )
        .expect("first edge");

        assert!(first.valid_to.is_none(), "first edge should be open");

        // Adding a new edge for the same triple must close the first one.
        let _second = add_edge(
            &conn,
            1,
            2,
            "works_at",
            &json!({"role": "manager"}),
            "2024-06-01T00:00:00Z",
            1.0,
            "hr",
            None,
        )
        .expect("second edge");

        // Re-fetch first edge to confirm it was closed.
        let updated = get_edge_by_id(&conn, first.id)
            .expect("query")
            .expect("edge still exists");

        assert_eq!(
            updated.valid_to.as_deref(),
            Some("2024-06-01T00:00:00Z"),
            "first edge should have been closed at the second edge's valid_from"
        );
    }

    // -------------------------------------------------------------------------
    // Test 3: Snapshot at a specific timestamp
    // -------------------------------------------------------------------------
    #[test]
    fn test_snapshot_at() {
        let conn = setup_db();

        // Edge valid in 2023 only.
        add_edge(
            &conn,
            1,
            2,
            "rel",
            &json!({}),
            "2023-01-01T00:00:00Z",
            1.0,
            "",
            None,
        )
        .unwrap();
        // Manually close it via a second edge (auto-invalidation).
        add_edge(
            &conn,
            1,
            2,
            "rel",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            None,
        )
        .unwrap();

        // Snapshot mid-2023 should return exactly 1 edge.
        let snap = snapshot_at(&conn, "2023-07-01T00:00:00Z", None).expect("snapshot");
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].valid_from, "2023-01-01T00:00:00Z");

        // Snapshot mid-2024 should return the second edge.
        let snap2 = snapshot_at(&conn, "2024-07-01T00:00:00Z", None).expect("snapshot");
        assert_eq!(snap2.len(), 1);
        assert_eq!(snap2[0].valid_from, "2024-01-01T00:00:00Z");
    }

    // -------------------------------------------------------------------------
    // Test 4: Timeline shows chronological history
    // -------------------------------------------------------------------------
    #[test]
    fn test_relationship_timeline_chronological() {
        let conn = setup_db();

        add_edge(
            &conn,
            10,
            20,
            "partner",
            &json!({}),
            "2020-01-01T00:00:00Z",
            1.0,
            "",
            None,
        )
        .unwrap();
        add_edge(
            &conn,
            10,
            20,
            "partner",
            &json!({}),
            "2021-06-01T00:00:00Z",
            1.0,
            "",
            None,
        )
        .unwrap();
        add_edge(
            &conn,
            10,
            20,
            "partner",
            &json!({}),
            "2022-09-01T00:00:00Z",
            1.0,
            "",
            None,
        )
        .unwrap();

        let timeline = relationship_timeline(&conn, 10, 20, None).expect("timeline");
        assert_eq!(timeline.len(), 3);

        // Verify ascending order.
        assert!(timeline[0].valid_from <= timeline[1].valid_from);
        assert!(timeline[1].valid_from <= timeline[2].valid_from);
    }

    // -------------------------------------------------------------------------
    // Test 5: Detect contradictions (manually injected overlap)
    // -------------------------------------------------------------------------
    #[test]
    fn test_detect_contradictions() {
        let conn = setup_db();

        // Insert two edges with overlapping validity directly (bypassing
        // the auto-invalidation logic that `add_edge` provides).
        conn.execute(
            "INSERT INTO temporal_edges
                 (from_id, to_id, relation, properties, valid_from, valid_to, confidence, source)
             VALUES (1, 2, 'rel', '{}', '2023-01-01T00:00:00Z', NULL, 1.0, '')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO temporal_edges
                 (from_id, to_id, relation, properties, valid_from, valid_to, confidence, source)
             VALUES (1, 2, 'rel', '{}', '2023-06-01T00:00:00Z', NULL, 1.0, '')",
            [],
        )
        .unwrap();

        let contradictions = detect_contradictions(&conn).expect("detect");
        assert_eq!(contradictions.len(), 1);

        let (a, b) = &contradictions[0];
        assert!(a.id < b.id);
    }

    // -------------------------------------------------------------------------
    // Test 6: Diff between two timestamps
    // -------------------------------------------------------------------------
    #[test]
    fn test_diff_between_timestamps() {
        let conn = setup_db();

        // Edge A: exists in 2023 and 2024.
        add_edge(
            &conn,
            1,
            2,
            "knows",
            &json!({}),
            "2022-01-01T00:00:00Z",
            1.0,
            "",
            None,
        )
        .unwrap();

        // Edge B: appears in 2024 only.
        add_edge(
            &conn,
            3,
            4,
            "likes",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            None,
        )
        .unwrap();

        let d = diff(&conn, "2023-01-01T00:00:00Z", "2025-01-01T00:00:00Z", None).expect("diff");

        // "knows" was present at both; "likes" was added.
        assert_eq!(d.added.len(), 1);
        assert_eq!(d.added[0].relation, "likes");
        assert_eq!(d.removed.len(), 0);
        // "knows" same edge, not changed.
        assert_eq!(d.changed.len(), 0);
    }

    // -------------------------------------------------------------------------
    // Test 7: Empty graph operations
    // -------------------------------------------------------------------------
    #[test]
    fn test_empty_graph_operations() {
        let conn = setup_db();

        let snap = snapshot_at(&conn, "2024-01-01T00:00:00Z", None).expect("snapshot");
        assert!(snap.is_empty());

        let timeline = relationship_timeline(&conn, 99, 100, None).expect("timeline");
        assert!(timeline.is_empty());

        let contradictions = detect_contradictions(&conn).expect("detect");
        assert!(contradictions.is_empty());

        let d = diff(&conn, "2024-01-01T00:00:00Z", "2025-01-01T00:00:00Z", None).expect("diff");
        assert!(d.added.is_empty());
        assert!(d.removed.is_empty());
        assert!(d.changed.is_empty());
    }

    // -------------------------------------------------------------------------
    // Test 8: Edge with rich JSON properties
    // -------------------------------------------------------------------------
    #[test]
    fn test_edge_with_json_properties() {
        let conn = setup_db();

        let props = json!({
            "title": "Senior Engineer",
            "department": "R&D",
            "salary": 120_000,
            "remote": true,
            "skills": ["Rust", "Python"]
        });

        let edge = add_edge(
            &conn,
            5,
            6,
            "employed_by",
            &props,
            "2024-03-01T00:00:00Z",
            0.95,
            "payroll",
            None,
        )
        .expect("add");

        assert_eq!(edge.properties["title"], "Senior Engineer");
        assert_eq!(edge.properties["salary"], 120_000);
        assert_eq!(edge.properties["remote"], true);
        assert_eq!(edge.properties["skills"][0], "Rust");
    }

    // -------------------------------------------------------------------------
    // Test 9: Invalidate edge manually
    // -------------------------------------------------------------------------
    #[test]
    fn test_invalidate_edge_manually() {
        let conn = setup_db();

        let edge = add_edge(
            &conn,
            7,
            8,
            "owns",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "legal",
            None,
        )
        .expect("add");

        assert!(edge.valid_to.is_none());

        invalidate_edge(&conn, edge.id, "2024-12-31T23:59:59Z").expect("invalidate");

        let updated = get_edge_by_id(&conn, edge.id)
            .expect("query")
            .expect("still exists");

        assert_eq!(updated.valid_to.as_deref(), Some("2024-12-31T23:59:59Z"));
    }

    // -------------------------------------------------------------------------
    // Test 10: Invalidating a non-existent edge returns NotFound
    // -------------------------------------------------------------------------
    #[test]
    fn test_invalidate_nonexistent_edge_returns_not_found() {
        let conn = setup_db();

        let result = invalidate_edge(&conn, 99999, "2025-01-01T00:00:00Z");
        assert!(
            matches!(result, Err(EngramError::NotFound(99999))),
            "expected NotFound(99999), got {:?}",
            result
        );
    }

    // -------------------------------------------------------------------------
    // Test 11: Diff detects edge supersession as "changed"
    // -------------------------------------------------------------------------
    #[test]
    fn test_diff_detects_changed_edge() {
        let conn = setup_db();

        // First version of the edge.
        add_edge(
            &conn,
            1,
            2,
            "role",
            &json!({"level": "junior"}),
            "2022-01-01T00:00:00Z",
            1.0,
            "",
            None,
        )
        .unwrap();

        // Supersede it (auto-invalidation closes the first).
        add_edge(
            &conn,
            1,
            2,
            "role",
            &json!({"level": "senior"}),
            "2023-06-01T00:00:00Z",
            1.0,
            "",
            None,
        )
        .unwrap();

        let d =
            diff(&conn, "2022-07-01T00:00:00Z", "2024-01-01T00:00:00Z", None).expect("diff");

        // The triple is present at both timestamps, but via a different edge id.
        assert_eq!(d.changed.len(), 1);
        let (old, new) = &d.changed[0];
        assert_eq!(old.properties["level"], "junior");
        assert_eq!(new.properties["level"], "senior");
    }

    // -------------------------------------------------------------------------
    // Test 12: Add edge with explicit scope, verify scope is stored
    // -------------------------------------------------------------------------
    #[test]
    fn test_add_edge_with_scope() {
        let conn = setup_db();

        // Edge in the default (global) scope.
        let global_edge = add_edge(
            &conn,
            1,
            2,
            "knows",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            None,
        )
        .expect("global edge");
        assert_eq!(global_edge.scope_path, "global");

        // Edge in a tenant-specific scope.
        let tenant_edge = add_edge(
            &conn,
            3,
            4,
            "manages",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            Some("global/org:acme"),
        )
        .expect("tenant edge");
        assert_eq!(tenant_edge.scope_path, "global/org:acme");

        // Edge in a deeper scope.
        let user_edge = add_edge(
            &conn,
            5,
            6,
            "reports_to",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            Some("global/org:acme/user:alice"),
        )
        .expect("user edge");
        assert_eq!(user_edge.scope_path, "global/org:acme/user:alice");

        // Auto-invalidation is scope-aware: adding another edge for the same
        // triple in a DIFFERENT scope must NOT close the first-scope edge.
        let acme_edge_1 = add_edge(
            &conn,
            10,
            20,
            "partner",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            Some("global/org:acme"),
        )
        .expect("acme edge 1");

        let _acme_edge_2 = add_edge(
            &conn,
            10,
            20,
            "partner",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            Some("global/org:beta"), // different scope — must not close acme_edge_1
        )
        .expect("beta edge");

        let refetched = get_edge_by_id(&conn, acme_edge_1.id)
            .expect("query")
            .expect("still exists");
        assert!(
            refetched.valid_to.is_none(),
            "edge in org:acme must not be closed by edge in org:beta"
        );
    }

    // -------------------------------------------------------------------------
    // Test 13: snapshot_at with scope_path filter
    // -------------------------------------------------------------------------
    #[test]
    fn test_snapshot_at_with_scope_filter() {
        let conn = setup_db();

        // Add one edge in "global" scope and one in "global/org:acme".
        add_edge(
            &conn,
            1,
            2,
            "rel",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            None, // defaults to "global"
        )
        .unwrap();

        add_edge(
            &conn,
            3,
            4,
            "rel",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            Some("global/org:acme"),
        )
        .unwrap();

        // No scope filter → both edges visible.
        let all = snapshot_at(&conn, "2025-01-01T00:00:00Z", None).unwrap();
        assert_eq!(all.len(), 2);

        // Filter to "global" includes all descendants (hierarchical prefix matching).
        // "global" matches exactly, and "global/org:acme" matches via LIKE 'global/%'.
        let global_tree = snapshot_at(&conn, "2025-01-01T00:00:00Z", Some("global")).unwrap();
        assert_eq!(global_tree.len(), 2, "global scope tree should include its child org:acme");

        // Filter to "global/org:acme" → only the acme edge (no further children here).
        let acme_only =
            snapshot_at(&conn, "2025-01-01T00:00:00Z", Some("global/org:acme")).unwrap();
        assert_eq!(acme_only.len(), 1);
        assert_eq!(acme_only[0].from_id, 3);

        // Demonstrate that "global" exact match can be queried by adding a non-child scope.
        add_edge(
            &conn,
            7,
            8,
            "rel",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            Some("global/org:beta"),
        )
        .unwrap();

        // "global/org:acme" filter should still only return the one acme edge.
        let acme_only2 =
            snapshot_at(&conn, "2025-01-01T00:00:00Z", Some("global/org:acme")).unwrap();
        assert_eq!(acme_only2.len(), 1);
        assert_eq!(acme_only2[0].from_id, 3);
    }

    // -------------------------------------------------------------------------
    // Test 14: scope prefix matching — hierarchy traversal
    // -------------------------------------------------------------------------
    #[test]
    fn test_scope_prefix_matching() {
        let conn = setup_db();

        // Three edges at different scope depths.
        add_edge(
            &conn,
            1,
            2,
            "a",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            Some("global/mbras"),
        )
        .unwrap();

        add_edge(
            &conn,
            3,
            4,
            "b",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            Some("global/mbras/broker_alice"),
        )
        .unwrap();

        add_edge(
            &conn,
            5,
            6,
            "c",
            &json!({}),
            "2024-01-01T00:00:00Z",
            1.0,
            "",
            Some("global/other"),
        )
        .unwrap();

        // Filtering on "global/mbras" should return:
        //   - the exact "global/mbras" edge
        //   - "global/mbras/broker_alice" (child)
        // but NOT "global/other".
        let mbras_snap =
            snapshot_at(&conn, "2025-01-01T00:00:00Z", Some("global/mbras")).unwrap();
        assert_eq!(
            mbras_snap.len(),
            2,
            "expected 2 edges under global/mbras, got: {:?}",
            mbras_snap
                .iter()
                .map(|e| &e.scope_path)
                .collect::<Vec<_>>()
        );

        let scope_paths: Vec<&str> = mbras_snap.iter().map(|e| e.scope_path.as_str()).collect();
        assert!(scope_paths.contains(&"global/mbras"));
        assert!(scope_paths.contains(&"global/mbras/broker_alice"));
    }
}
