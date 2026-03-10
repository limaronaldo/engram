//! MCP tool handlers for DuckDB-backed temporal graph queries (Phase M).
//!
//! Exposes three analytical graph tools over the DuckDB CQRS read side:
//! - `memory_graph_path`       — find shortest paths between two nodes
//! - `memory_temporal_snapshot` — point-in-time graph snapshot
//! - `memory_scope_snapshot`   — structural diff between two snapshots
//!
//! All handlers are feature-gated with `#[cfg(feature = "duckdb-graph")]`.

#[cfg(feature = "duckdb-graph")]
use serde_json::{json, Value};

#[cfg(feature = "duckdb-graph")]
use super::HandlerContext;

#[cfg(feature = "duckdb-graph")]
use crate::graph::duckdb_graph::TemporalGraph;

// ── memory_graph_path ────────────────────────────────────────────────────────

/// Find all shortest paths between two memory nodes within a scope.
///
/// Params:
/// - `scope` (string, required) — scope prefix to restrict traversal
/// - `source_id` (i64, required) — origin node id
/// - `target_id` (i64, required) — destination node id
/// - `max_depth` (u8, optional, default: 4) — maximum hops to explore
///
/// Returns a JSON array of path objects: `[{"path": "1 -[r]-> 2", "depth": 1}, …]`
#[cfg(feature = "duckdb-graph")]
pub fn handle_memory_graph_path(ctx: &HandlerContext, params: Value) -> Value {
    let scope = match params.get("scope").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "missing required param: scope"}),
    };

    let source_id = match params.get("source_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "missing required param: source_id"}),
    };

    let target_id = match params.get("target_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "missing required param: target_id"}),
    };

    let max_depth = params
        .get("max_depth")
        .and_then(|v| v.as_u64())
        .unwrap_or(4)
        .min(255) as u8;

    let db_path = ctx.storage.db_path().to_string();

    let graph = match TemporalGraph::new(&db_path) {
        Ok(g) => g,
        Err(e) => return json!({"error": format!("failed to open DuckDB graph: {}", e)}),
    };

    match graph.find_connection(&scope, source_id, target_id, max_depth) {
        Ok(paths) => json!(paths),
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── memory_temporal_snapshot ─────────────────────────────────────────────────

/// Return all edges active at a given point in time within a scope.
///
/// Params:
/// - `scope` (string, required) — scope prefix filter
/// - `timestamp` (string, required) — ISO-8601 point-in-time (e.g. "2024-06-01")
///
/// Returns a JSON array of temporal edge objects.
#[cfg(feature = "duckdb-graph")]
pub fn handle_memory_temporal_snapshot(ctx: &HandlerContext, params: Value) -> Value {
    let scope = match params.get("scope").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "missing required param: scope"}),
    };

    let timestamp = match params.get("timestamp").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return json!({"error": "missing required param: timestamp"}),
    };

    let db_path = ctx.storage.db_path().to_string();

    let graph = match TemporalGraph::new(&db_path) {
        Ok(g) => g,
        Err(e) => return json!({"error": format!("failed to open DuckDB graph: {}", e)}),
    };

    match graph.snapshot_at(&scope, &timestamp) {
        Ok(edges) => json!(edges),
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── memory_scope_snapshot ────────────────────────────────────────────────────

/// Compute the structural diff between two graph snapshots within a scope.
///
/// Params:
/// - `scope` (string, required) — scope prefix filter
/// - `from_timestamp` (string, required) — ISO-8601 start snapshot time
/// - `to_timestamp` (string, required) — ISO-8601 end snapshot time
///
/// Returns a JSON object:
/// ```json
/// {
///   "added":   [ … ],
///   "removed": [ … ],
///   "changed": [ … ]
/// }
/// ```
#[cfg(feature = "duckdb-graph")]
pub fn handle_memory_scope_snapshot(ctx: &HandlerContext, params: Value) -> Value {
    let scope = match params.get("scope").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "missing required param: scope"}),
    };

    let from_timestamp = match params.get("from_timestamp").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return json!({"error": "missing required param: from_timestamp"}),
    };

    let to_timestamp = match params.get("to_timestamp").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return json!({"error": "missing required param: to_timestamp"}),
    };

    let db_path = ctx.storage.db_path().to_string();

    let graph = match TemporalGraph::new(&db_path) {
        Ok(g) => g,
        Err(e) => return json!({"error": format!("failed to open DuckDB graph: {}", e)}),
    };

    match graph.graph_diff(&scope, &from_timestamp, &to_timestamp) {
        Ok(diff) => json!({
            "added":   diff.added,
            "removed": diff.removed,
            "changed": diff.changed.into_iter().map(|(before, after)| json!({
                "before": before,
                "after":  after,
            })).collect::<Vec<_>>(),
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}
