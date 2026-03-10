//! DuckDB CQRS graph integration tests (Phase M).
//!
//! Exercises the full CQRS flow: write via rusqlite (SQLite write side),
//! read via DuckDB's TemporalGraph (OLAP read side).
//!
//! Run with:
//!   cargo test --features duckdb-graph --test duckdb_graph_tests

#![cfg(feature = "duckdb-graph")]

use engram::graph::duckdb_graph::TemporalGraph;
use rusqlite::Connection as SqliteConnection;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a minimal SQLite database at `path` with the v33 schema tables.
fn setup_test_db(path: &str) -> SqliteConnection {
    let conn = SqliteConnection::open(path).expect("open sqlite");
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS graph_entities (
            id          TEXT PRIMARY KEY,
            scope_path  TEXT NOT NULL DEFAULT 'global',
            name        TEXT NOT NULL,
            entity_type TEXT NOT NULL,
            metadata    TEXT NOT NULL DEFAULT '{}',
            created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );

        CREATE TABLE IF NOT EXISTS temporal_edges (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id     INTEGER NOT NULL,
            to_id       INTEGER NOT NULL,
            relation    TEXT NOT NULL,
            properties  TEXT NOT NULL DEFAULT '{}',
            valid_from  TEXT NOT NULL,
            valid_to    TEXT,
            confidence  REAL NOT NULL DEFAULT 1.0,
            source      TEXT NOT NULL DEFAULT '',
            scope_path  TEXT NOT NULL DEFAULT 'global',
            created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );
    ",
    )
    .expect("create schema tables");
    conn
}

/// Insert one edge into `temporal_edges`.
#[allow(clippy::too_many_arguments)]
fn insert_edge(
    conn: &SqliteConnection,
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

/// Build a unique temp-file path for each test.
fn tmp_path(label: &str) -> String {
    format!(
        "/tmp/engram_integ_{}_{}.db",
        label,
        std::process::id()
    )
}

fn cleanup(path: &str) {
    let _ = std::fs::remove_file(path);
}

// ---------------------------------------------------------------------------
// Test 1: CQRS round-trip — write SQLite, read DuckDB
// ---------------------------------------------------------------------------

/// Insert edges via rusqlite (write side) and verify DuckDB reads them back
/// correctly through `snapshot_at`.
#[test]
fn test_cqrs_write_sqlite_read_duckdb() {
    let path = tmp_path("cqrs_roundtrip");
    cleanup(&path);

    // Write side: populate via SQLite.
    {
        let conn = setup_test_db(&path);
        insert_edge(&conn, 1, 2, "knows", "2024-01-01", None, 0.9, "global");
        insert_edge(&conn, 2, 3, "follows", "2024-01-01", None, 0.8, "global");
        insert_edge(&conn, 3, 4, "linked", "2024-01-01", None, 0.7, "global");
        // This edge drops the conn (and flushes WAL) after the block ends.
    }

    // Read side: open DuckDB and query.
    let graph = TemporalGraph::new(&path).expect("TemporalGraph::new should succeed");
    let edges = graph
        .snapshot_at("global", "2024-06-01")
        .expect("snapshot_at should succeed");

    assert_eq!(edges.len(), 3, "all three edges should be visible via DuckDB");

    let relations: Vec<&str> = edges.iter().map(|e| e.relation.as_str()).collect();
    assert!(relations.contains(&"knows"), "edge 'knows' missing");
    assert!(relations.contains(&"follows"), "edge 'follows' missing");
    assert!(relations.contains(&"linked"), "edge 'linked' missing");

    // Verify structural fields are preserved correctly.
    let knows_edge = edges.iter().find(|e| e.relation == "knows").unwrap();
    assert_eq!(knows_edge.from_id, 1);
    assert_eq!(knows_edge.to_id, 2);
    assert!((knows_edge.confidence - 0.9_f32).abs() < 0.001, "confidence mismatch");
    assert_eq!(knows_edge.scope_path, "global");
    assert!(knows_edge.valid_to.is_none(), "open edge should have valid_to = None");

    cleanup(&path);
}

// ---------------------------------------------------------------------------
// Test 2: Scope isolation
// ---------------------------------------------------------------------------

/// Edges in different scope branches must not bleed across scope boundaries.
/// Querying `global/org_a` must return ONLY edges whose scope_path starts
/// with `global/org_a`, never those under `global/org_b`.
#[test]
fn test_scope_isolation() {
    let path = tmp_path("scope_isolation");
    cleanup(&path);

    {
        let conn = setup_test_db(&path);

        // Org-A edges (should be visible when scoping to global/org_a).
        insert_edge(&conn, 10, 11, "member_of", "2024-01-01", None, 1.0, "global/org_a/user_1");
        insert_edge(&conn, 11, 12, "reports_to", "2024-01-01", None, 1.0, "global/org_a");

        // Org-B edges (must NOT appear under global/org_a queries).
        insert_edge(&conn, 20, 21, "member_of", "2024-01-01", None, 1.0, "global/org_b/user_2");
        insert_edge(&conn, 21, 22, "reports_to", "2024-01-01", None, 1.0, "global/org_b");

        // Root scope edge (also must NOT appear — LIKE 'global/org_a%' won't match 'global').
        insert_edge(&conn, 30, 31, "root_link", "2024-01-01", None, 1.0, "global");
    }

    let graph = TemporalGraph::new(&path).expect("TemporalGraph::new");

    // Query scoped to org_a.
    let org_a_edges = graph
        .snapshot_at("global/org_a", "2024-06-01")
        .expect("snapshot_at org_a");

    assert_eq!(
        org_a_edges.len(),
        2,
        "only the two org_a edges should be returned; got {:?}",
        org_a_edges.iter().map(|e| &e.scope_path).collect::<Vec<_>>()
    );

    for edge in &org_a_edges {
        assert!(
            edge.scope_path.starts_with("global/org_a"),
            "unexpected scope '{}' in org_a result set",
            edge.scope_path
        );
    }

    // Verify org_b scope is also isolated.
    let org_b_edges = graph
        .snapshot_at("global/org_b", "2024-06-01")
        .expect("snapshot_at org_b");

    assert_eq!(org_b_edges.len(), 2, "org_b should have exactly two edges");
    for edge in &org_b_edges {
        assert!(
            edge.scope_path.starts_with("global/org_b"),
            "unexpected scope '{}' in org_b result set",
            edge.scope_path
        );
    }

    cleanup(&path);
}

// ---------------------------------------------------------------------------
// Test 3: Temporal snapshot correctness
// ---------------------------------------------------------------------------

/// Edges have different validity periods.  Querying at specific timestamps
/// must return exactly the edges that are active at each point in time.
#[test]
fn test_temporal_snapshot_correctness() {
    let path = tmp_path("temporal_snapshot");
    cleanup(&path);

    {
        let conn = setup_test_db(&path);

        // Edge A: closed window 2023-Q1.
        insert_edge(&conn, 1, 2, "alpha", "2023-01-01", Some("2023-03-31"), 1.0, "global");
        // Edge B: closed window 2023-Q2.
        insert_edge(&conn, 2, 3, "beta", "2023-04-01", Some("2023-06-30"), 1.0, "global");
        // Edge C: starts 2023-07-01, still open.
        insert_edge(&conn, 3, 4, "gamma", "2023-07-01", None, 1.0, "global");
        // Edge D: starts in the future relative to our queries.
        insert_edge(&conn, 4, 5, "delta", "2025-01-01", None, 1.0, "global");
    }

    let graph = TemporalGraph::new(&path).expect("TemporalGraph::new");

    // At 2023-02-01: only edge A active.
    let snap_feb = graph.snapshot_at("global", "2023-02-01").expect("snap feb");
    assert_eq!(snap_feb.len(), 1, "only 'alpha' active in Feb 2023");
    assert_eq!(snap_feb[0].relation, "alpha");

    // At 2023-05-01: only edge B active.
    let snap_may = graph.snapshot_at("global", "2023-05-01").expect("snap may");
    assert_eq!(snap_may.len(), 1, "only 'beta' active in May 2023");
    assert_eq!(snap_may[0].relation, "beta");

    // At 2023-08-01: only edge C active (A and B expired, D not started).
    let snap_aug = graph.snapshot_at("global", "2023-08-01").expect("snap aug");
    assert_eq!(snap_aug.len(), 1, "only 'gamma' active in Aug 2023");
    assert_eq!(snap_aug[0].relation, "gamma");

    // At 2025-06-01: edges C and D both active.
    let snap_2025 = graph.snapshot_at("global", "2025-06-01").expect("snap 2025");
    assert_eq!(snap_2025.len(), 2, "both 'gamma' and 'delta' active in mid-2025");
    let rels_2025: Vec<&str> = snap_2025.iter().map(|e| e.relation.as_str()).collect();
    assert!(rels_2025.contains(&"gamma"));
    assert!(rels_2025.contains(&"delta"));

    cleanup(&path);
}

// ---------------------------------------------------------------------------
// Test 4: Path-finding end-to-end
// ---------------------------------------------------------------------------

/// Build a multi-hop graph via SQLite, verify DuckDB `find_connection`
/// discovers the shortest path correctly across multiple hops.
///
/// Graph (all edges open / valid_to IS NULL):
///   1 -[works_at]-> 2 -[located_in]-> 3 -[part_of]-> 4
///   1 -[knows]-----> 5
#[test]
fn test_path_finding_end_to_end() {
    let path = tmp_path("path_finding");
    cleanup(&path);

    {
        let conn = setup_test_db(&path);
        // Chain: 1 -> 2 -> 3 -> 4
        insert_edge(&conn, 1, 2, "works_at", "2024-01-01", None, 1.0, "global");
        insert_edge(&conn, 2, 3, "located_in", "2024-01-01", None, 1.0, "global");
        insert_edge(&conn, 3, 4, "part_of", "2024-01-01", None, 1.0, "global");
        // Side branch: 1 -> 5 (dead end for reaching 4)
        insert_edge(&conn, 1, 5, "knows", "2024-01-01", None, 1.0, "global");
    }

    let graph = TemporalGraph::new(&path).expect("TemporalGraph::new");

    // Direct 1-hop: 1 -> 2
    let paths_1hop = graph
        .find_connection("global", 1, 2, 5)
        .expect("find_connection 1->2");
    assert!(!paths_1hop.is_empty(), "should find 1->2 path");
    assert_eq!(paths_1hop[0].depth, 1, "1->2 is a single hop");

    // 2-hop: 1 -> 2 -> 3
    let paths_2hop = graph
        .find_connection("global", 1, 3, 5)
        .expect("find_connection 1->3");
    assert!(!paths_2hop.is_empty(), "should find 1->2->3 path");
    assert_eq!(paths_2hop[0].depth, 2, "1->2->3 is two hops");
    assert!(
        paths_2hop[0].path.contains("-[works_at]->"),
        "path should traverse works_at"
    );
    assert!(
        paths_2hop[0].path.contains("-[located_in]->"),
        "path should traverse located_in"
    );

    // 3-hop: 1 -> 2 -> 3 -> 4
    let paths_3hop = graph
        .find_connection("global", 1, 4, 5)
        .expect("find_connection 1->4");
    assert!(!paths_3hop.is_empty(), "should find 3-hop path to node 4");
    assert_eq!(paths_3hop[0].depth, 3, "1->2->3->4 is three hops");

    // No path from 5 to 4 (5 is a dead-end sink).
    let paths_none = graph
        .find_connection("global", 5, 4, 5)
        .expect("find_connection 5->4");
    assert!(
        paths_none.is_empty(),
        "node 5 is a sink — no path to node 4"
    );

    // Neighbor exploration from 1 at depth 2 should reach 2, 3, and 5.
    let neighbors = graph
        .find_neighbors("global", 1, 2)
        .expect("find_neighbors from 1");
    let depth1_count = neighbors.iter().filter(|n| n.depth == 1).count();
    let depth2_count = neighbors.iter().filter(|n| n.depth == 2).count();
    assert_eq!(depth1_count, 2, "nodes 2 and 5 are at depth 1");
    assert_eq!(depth2_count, 1, "node 3 is at depth 2 via 2");

    cleanup(&path);
}

// ---------------------------------------------------------------------------
// Test 5: refresh() picks up new SQLite writes
// ---------------------------------------------------------------------------

/// Verifies that calling `refresh()` causes DuckDB to see rows that were
/// committed to SQLite *after* the initial `TemporalGraph::new` call.
#[test]
fn test_refresh_picks_up_new_writes() {
    let path = tmp_path("refresh_new_writes");
    cleanup(&path);

    // Create the schema with no data.
    let conn = setup_test_db(&path);

    // Open DuckDB while the table is still empty.
    let graph = TemporalGraph::new(&path).expect("TemporalGraph::new");

    // Confirm nothing visible yet.
    let before = graph
        .snapshot_at("global", "2024-06-01")
        .expect("snapshot before insert");
    assert_eq!(before.len(), 0, "no edges should be visible before insert");

    // Now write new edges via the still-open SQLite connection.
    insert_edge(&conn, 100, 101, "new_edge_a", "2024-01-01", None, 0.9, "global");
    insert_edge(&conn, 101, 102, "new_edge_b", "2024-01-01", None, 0.8, "global");
    // Drop conn to ensure WAL is flushed before DuckDB re-attaches.
    drop(conn);

    // Without refresh, DuckDB is still looking at the stale snapshot.
    // Call refresh() to detach + re-attach the SQLite file.
    graph.refresh().expect("refresh should succeed");

    let after = graph
        .snapshot_at("global", "2024-06-01")
        .expect("snapshot after refresh");
    assert_eq!(
        after.len(),
        2,
        "both new edges should be visible after refresh(); got {} edges",
        after.len()
    );

    let rels: Vec<&str> = after.iter().map(|e| e.relation.as_str()).collect();
    assert!(rels.contains(&"new_edge_a"), "'new_edge_a' not found after refresh");
    assert!(rels.contains(&"new_edge_b"), "'new_edge_b' not found after refresh");

    cleanup(&path);
}

// ---------------------------------------------------------------------------
// Test 6: graph_diff between timestamps
// ---------------------------------------------------------------------------

/// Insert edges at different times and verify that `graph_diff` correctly
/// identifies additions and removals between two timestamp snapshots.
#[test]
fn test_graph_diff_between_timestamps() {
    let path = tmp_path("graph_diff");
    cleanup(&path);

    {
        let conn = setup_test_db(&path);

        // Edge present at both t1 (2024-01-15) and t2 (2024-09-01): no change.
        insert_edge(&conn, 1, 2, "stable", "2024-01-01", None, 1.0, "global");

        // Edge present at t1 but expired before t2: should appear as removed.
        insert_edge(
            &conn,
            1,
            3,
            "expired",
            "2024-01-01",
            Some("2024-03-31"),
            1.0,
            "global",
        );

        // Edge starting after t1: should appear as added at t2.
        insert_edge(&conn, 2, 3, "added_later", "2024-06-01", None, 0.5, "global");

        // Edge not yet started at either snapshot: invisible at both.
        insert_edge(&conn, 3, 4, "future", "2025-01-01", None, 1.0, "global");
    }

    let graph = TemporalGraph::new(&path).expect("TemporalGraph::new");

    let diff = graph
        .graph_diff("global", "2024-01-15", "2024-09-01")
        .expect("graph_diff");

    // One edge added (added_later, starts 2024-06-01, visible at 2024-09-01).
    assert_eq!(
        diff.added.len(),
        1,
        "exactly one edge should be added; got added={:?}",
        diff.added.iter().map(|e| &e.relation).collect::<Vec<_>>()
    );
    assert_eq!(diff.added[0].relation, "added_later");

    // One edge removed (expired before t2).
    assert_eq!(
        diff.removed.len(),
        1,
        "exactly one edge should be removed; got removed={:?}",
        diff.removed.iter().map(|e| &e.relation).collect::<Vec<_>>()
    );
    assert_eq!(diff.removed[0].relation, "expired");

    // No edges changed (stable edge has same confidence at both times).
    assert_eq!(
        diff.changed.len(),
        0,
        "no edges should be marked changed"
    );

    // Verify the stable edge is in neither added nor removed sets.
    let added_rels: Vec<&str> = diff.added.iter().map(|e| e.relation.as_str()).collect();
    let removed_rels: Vec<&str> = diff.removed.iter().map(|e| e.relation.as_str()).collect();
    assert!(!added_rels.contains(&"stable"), "'stable' must not appear in added");
    assert!(!removed_rels.contains(&"stable"), "'stable' must not appear in removed");

    cleanup(&path);
}
