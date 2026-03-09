//! Temporal graph and scoping tool handlers (Round 3 — T12/T13).
//!
//! Covers:
//! - Temporal knowledge graph edges (bi-temporal validity)
//! - Scope management for hierarchical multi-tenant memory

use serde_json::{json, Value};

use super::HandlerContext;

// ── Temporal graph ────────────────────────────────────────────────────────────

/// Add a temporal edge to the knowledge graph.
///
/// Params:
/// - `from_id` (i64, required) — source memory id
/// - `to_id` (i64, required) — target memory id
/// - `relation` (string, required) — semantic label (e.g. "works_at")
/// - `valid_from` (string, required) — RFC3339 start of validity
/// - `properties` (object, optional) — arbitrary JSON metadata
/// - `confidence` (f64, optional, default: 1.0)
/// - `source` (string, optional) — provenance string
pub fn temporal_add_edge(ctx: &HandlerContext, params: Value) -> Value {
    use crate::graph::temporal::add_edge;

    let from_id = match params.get("from_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "from_id is required"}),
    };

    let to_id = match params.get("to_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "to_id is required"}),
    };

    let relation = match params.get("relation").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => return json!({"error": "relation is required"}),
    };

    let valid_from = match params.get("valid_from").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return json!({"error": "valid_from is required"}),
    };

    let properties = params.get("properties").cloned().unwrap_or(json!({}));

    let confidence = params
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0) as f32;

    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    ctx.storage
        .with_connection(|conn| {
            let edge = add_edge(
                conn,
                from_id,
                to_id,
                &relation,
                &properties,
                &valid_from,
                confidence,
                &source,
            )?;
            Ok(json!(edge))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Return a snapshot of all currently-valid edges at a given timestamp.
///
/// Params:
/// - `timestamp` (string, required) — RFC3339 point in time
pub fn temporal_snapshot(ctx: &HandlerContext, params: Value) -> Value {
    use crate::graph::temporal::snapshot_at;

    let timestamp = match params.get("timestamp").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return json!({"error": "timestamp is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let edges = snapshot_at(conn, &timestamp)?;
            Ok(json!({"timestamp": timestamp, "edges": edges, "count": edges.len()}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Return the full relationship timeline for a (from_id, to_id) pair.
///
/// Params:
/// - `from_id` (i64, required)
/// - `to_id` (i64, required)
pub fn temporal_timeline(ctx: &HandlerContext, params: Value) -> Value {
    use crate::graph::temporal::relationship_timeline;

    let from_id = match params.get("from_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "from_id is required"}),
    };

    let to_id = match params.get("to_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "to_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let edges = relationship_timeline(conn, from_id, to_id)?;
            Ok(json!({
                "from_id": from_id,
                "to_id": to_id,
                "timeline": edges,
                "count": edges.len()
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Detect overlapping (contradictory) edges in the temporal graph.
pub fn temporal_contradictions(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::graph::temporal::detect_contradictions;

    ctx.storage
        .with_connection(|conn| {
            let pairs = detect_contradictions(conn)?;
            let items: Vec<Value> = pairs
                .iter()
                .map(|(a, b)| {
                    json!({
                        "edge_a": a,
                        "edge_b": b
                    })
                })
                .collect();
            Ok(json!({
                "contradictions": items,
                "count": items.len()
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Compute the graph diff between two timestamps.
///
/// Params:
/// - `t1` (string, required) — earlier RFC3339 timestamp
/// - `t2` (string, required) — later RFC3339 timestamp
pub fn temporal_diff(ctx: &HandlerContext, params: Value) -> Value {
    use crate::graph::temporal::diff;

    let t1 = match params.get("t1").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return json!({"error": "t1 is required"}),
    };

    let t2 = match params.get("t2").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return json!({"error": "t2 is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let graph_diff = diff(conn, &t1, &t2)?;
            Ok(json!({
                "t1": t1,
                "t2": t2,
                "added": graph_diff.added,
                "removed": graph_diff.removed,
                "changed": graph_diff.changed.iter().map(|(a, b)| json!({"before": a, "after": b})).collect::<Vec<_>>()
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── Scope management ──────────────────────────────────────────────────────────

/// Set the scope of a memory.
///
/// Params:
/// - `memory_id` (i64, required)
/// - `scope_path` (string, required) — e.g. "global/org:acme/user:alice"
pub fn scope_set(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::scoping::{set_scope, MemoryScope};

    let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "memory_id is required"}),
    };

    let scope_path = match params.get("scope_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return json!({"error": "scope_path is required"}),
    };

    let scope = match MemoryScope::parse(&scope_path) {
        Ok(s) => s,
        Err(e) => return json!({"error": e.to_string()}),
    };

    ctx.storage
        .with_connection(|conn| {
            set_scope(conn, memory_id, &scope)?;
            Ok(json!({"success": true, "memory_id": memory_id, "scope_path": scope_path}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Get the current scope of a memory.
///
/// Params:
/// - `memory_id` (i64, required)
pub fn scope_get(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::scoping::get_scope;

    let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "memory_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let scope = get_scope(conn, memory_id)?;
            Ok(json!({
                "memory_id": memory_id,
                "scope_path": scope.path,
                "level": scope.level.to_string()
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// List all distinct scopes in the database.
pub fn scope_list(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::scoping::list_scopes;

    ctx.storage
        .with_connection(|conn| {
            let scopes = list_scopes(conn)?;
            let items: Vec<Value> = scopes
                .iter()
                .map(|s| json!({"path": s.path, "level": s.level.to_string()}))
                .collect();
            Ok(json!({"scopes": items, "count": items.len()}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Search for memories within a scope (includes ancestor scopes).
///
/// Params:
/// - `query` (string, required) — substring search
/// - `scope_path` (string, required) — target scope
pub fn scope_search(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::scoping::{search_scoped, MemoryScope};

    let query = match params.get("query").and_then(|v| v.as_str()) {
        Some(q) => q.to_string(),
        None => return json!({"error": "query is required"}),
    };

    let scope_path = match params.get("scope_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return json!({"error": "scope_path is required"}),
    };

    let scope = match MemoryScope::parse(&scope_path) {
        Ok(s) => s,
        Err(e) => return json!({"error": e.to_string()}),
    };

    ctx.storage
        .with_connection(|conn| {
            let ids = search_scoped(conn, &query, &scope)?;
            Ok(json!({
                "scope_path": scope_path,
                "query": query,
                "memory_ids": ids,
                "count": ids.len()
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Return a hierarchical tree of all scopes.
pub fn scope_tree_handler(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::scoping::scope_tree;

    ctx.storage
        .with_connection(|conn| {
            let tree = scope_tree(conn)?;
            Ok(json!({"tree": tree}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
