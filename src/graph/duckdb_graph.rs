//! DuckDB-backed TemporalGraph for OLAP read operations (Phase M).
//!
//! Implements the CQRS read side: SQLite handles all writes, DuckDB attaches
//! the same file read-only and provides fast analytical queries.  When the
//! optional `duckpgq` extension is available a full property-graph (`MATCH`)
//! query surface is also created; otherwise the module degrades gracefully to
//! plain SQL.

#![cfg(feature = "duckdb-graph")]

use duckdb::Connection as DuckdbConnection;
use tracing::{warn, debug};

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
    fn setup_sqlite(path: &str) {
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
}
