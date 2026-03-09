//! Graph Conflict Detection & Resolution (RML-1217)
//!
//! Inspired by Mem0's approach to managing conflicting knowledge in graphs.
//! Provides:
//! - Detection of four conflict types: direct contradictions, temporal inconsistencies,
//!   cyclic dependencies, and orphaned references.
//! - Resolution strategies: keep newer, keep higher confidence, merge, or manual.
//! - Persistence of detected conflicts in the `graph_conflicts` table.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::error::{EngramError, Result};

// =============================================================================
// DDL
// =============================================================================

/// SQL that creates the `graph_conflicts` table.
///
/// Safe to run on an existing database — uses `IF NOT EXISTS`.
pub const CREATE_CONFLICTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS graph_conflicts (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    conflict_type       TEXT    NOT NULL,
    edge_ids            TEXT    NOT NULL DEFAULT '[]',
    description         TEXT    NOT NULL,
    severity            TEXT    NOT NULL,
    resolved_at         TEXT,
    resolution_strategy TEXT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_graph_conflicts_type     ON graph_conflicts(conflict_type);
CREATE INDEX IF NOT EXISTS idx_graph_conflicts_severity ON graph_conflicts(severity);
CREATE INDEX IF NOT EXISTS idx_graph_conflicts_resolved ON graph_conflicts(resolved_at);
"#;

// =============================================================================
// Types
// =============================================================================

/// The category of graph conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictType {
    /// Two edges between the same pair of nodes carry contradicting relation
    /// types (e.g. "supports" AND "contradicts" for the same A→B pair).
    DirectContradiction,
    /// Two or more edges for the same entity pair have overlapping validity
    /// periods, indicating a temporal inconsistency.
    TemporalInconsistency,
    /// A cycle exists in the directed edge graph (A→B→C→A).
    CyclicDependency,
    /// An edge references a `from_id` or `to_id` that does not exist in the
    /// `memories` table.
    OrphanedReference,
}

impl ConflictType {
    fn as_str(&self) -> &'static str {
        match self {
            ConflictType::DirectContradiction => "direct_contradiction",
            ConflictType::TemporalInconsistency => "temporal_inconsistency",
            ConflictType::CyclicDependency => "cyclic_dependency",
            ConflictType::OrphanedReference => "orphaned_reference",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "direct_contradiction" => Some(ConflictType::DirectContradiction),
            "temporal_inconsistency" => Some(ConflictType::TemporalInconsistency),
            "cyclic_dependency" => Some(ConflictType::CyclicDependency),
            "orphaned_reference" => Some(ConflictType::OrphanedReference),
            _ => None,
        }
    }
}

/// Severity level of a detected conflict.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    fn as_str(&self) -> &'static str {
        match self {
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Severity::Low),
            "medium" => Some(Severity::Medium),
            "high" => Some(Severity::High),
            "critical" => Some(Severity::Critical),
            _ => None,
        }
    }
}

/// A detected conflict in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// Unique row identifier (`0` for unsaved conflicts).
    pub id: i64,
    /// Category of conflict.
    pub conflict_type: ConflictType,
    /// IDs of the edges involved in this conflict.
    pub edge_ids: Vec<i64>,
    /// Human-readable description of the conflict.
    pub description: String,
    /// How severe this conflict is.
    pub severity: Severity,
    /// When the conflict was resolved (`None` = unresolved).
    pub resolved_at: Option<String>,
    /// Which strategy was used to resolve this conflict.
    pub resolution_strategy: Option<ResolutionStrategy>,
}

/// Strategy to apply when resolving a conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStrategy {
    /// Remove all but the most recently created edge.
    KeepNewer,
    /// Remove all but the edge with the highest confidence / importance proxy.
    KeepHigherConfidence,
    /// Merge edge metadata into a single edge.
    Merge,
    /// Mark the conflict resolved without modifying any edges.
    Manual,
}

impl ResolutionStrategy {
    fn as_str(&self) -> &'static str {
        match self {
            ResolutionStrategy::KeepNewer => "keep_newer",
            ResolutionStrategy::KeepHigherConfidence => "keep_higher_confidence",
            ResolutionStrategy::Merge => "merge",
            ResolutionStrategy::Manual => "manual",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "keep_newer" => Some(ResolutionStrategy::KeepNewer),
            "keep_higher_confidence" => Some(ResolutionStrategy::KeepHigherConfidence),
            "merge" => Some(ResolutionStrategy::Merge),
            "manual" => Some(ResolutionStrategy::Manual),
            _ => None,
        }
    }
}

/// Outcome of resolving a conflict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionResult {
    /// The conflict that was resolved.
    pub conflict_id: i64,
    /// Strategy that was applied.
    pub strategy: ResolutionStrategy,
    /// Edge IDs that were deleted during resolution.
    pub edges_removed: Vec<i64>,
    /// Edge IDs that were kept during resolution.
    pub edges_kept: Vec<i64>,
}

// =============================================================================
// ConflictDetector
// =============================================================================

/// Detects conflicts in the `cross_references` graph.
pub struct ConflictDetector;

/// Pairs of relation types that are considered direct contradictions.
const CONTRADICTING_PAIRS: &[(&str, &str)] = &[
    ("supports", "contradicts"),
    ("agrees_with", "disagrees_with"),
    ("confirms", "refutes"),
    ("approves", "rejects"),
    ("enables", "prevents"),
    ("causes", "prevents"),
];

impl ConflictDetector {
    /// Run all detectors and return a combined, deduplicated list of conflicts.
    pub fn detect_all(conn: &Connection) -> Result<Vec<Conflict>> {
        let mut conflicts = Vec::new();
        conflicts.extend(Self::detect_contradictions(conn)?);
        conflicts.extend(Self::detect_temporal_inconsistencies(conn)?);
        conflicts.extend(Self::detect_cycles(conn)?);
        conflicts.extend(Self::detect_orphans(conn)?);
        Ok(conflicts)
    }

    /// Find edges where A→B has contradicting relation types
    /// (e.g. both "supports" and "contradicts" for the same pair).
    pub fn detect_contradictions(conn: &Connection) -> Result<Vec<Conflict>> {
        // Load all edges from cross_references.
        let edges = load_all_edges(conn)?;

        // Group by (from_id, to_id).
        let mut by_pair: HashMap<(i64, i64), Vec<EdgeRow>> = HashMap::new();
        for edge in edges {
            by_pair
                .entry((edge.from_id, edge.to_id))
                .or_default()
                .push(edge);
        }

        let mut conflicts = Vec::new();

        for ((from_id, to_id), group) in &by_pair {
            let relations: Vec<&str> = group.iter().map(|e| e.relation_type.as_str()).collect();

            for &(a, b) in CONTRADICTING_PAIRS {
                if relations.contains(&a) && relations.contains(&b) {
                    let involved_ids: Vec<i64> = group.iter().map(|e| e.id).collect();
                    conflicts.push(Conflict {
                        id: 0,
                        conflict_type: ConflictType::DirectContradiction,
                        edge_ids: involved_ids,
                        description: format!(
                            "Contradicting relations '{}' and '{}' between nodes {} and {}",
                            a, b, from_id, to_id
                        ),
                        severity: Severity::High,
                        resolved_at: None,
                        resolution_strategy: None,
                    });
                }
            }
        }

        Ok(conflicts)
    }

    /// Find edges with overlapping validity periods for the same entity pair.
    ///
    /// Queries the `cross_references` table and treats the `created_at` column
    /// as a proxy for validity start. If two edges share the same
    /// `(from_id, to_id, relation_type)` triple, that is considered a temporal
    /// inconsistency — one should have been closed when the next was created.
    pub fn detect_temporal_inconsistencies(conn: &Connection) -> Result<Vec<Conflict>> {
        // Self-join: same triple, both are unresolved / open, different IDs.
        let sql = "
            SELECT a.id, b.id, a.from_id, a.to_id, a.relation_type
            FROM   cross_references a
            JOIN   cross_references b
              ON   a.from_id       = b.from_id
             AND   a.to_id         = b.to_id
             AND   a.relation_type = b.relation_type
             AND   a.id < b.id
        ";

        let table_exists = table_exists(conn, "cross_references")?;
        if !table_exists {
            return Ok(Vec::new());
        }

        let mut stmt = conn.prepare(sql).map_err(EngramError::Database)?;

        let pairs = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(EngramError::Database)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(EngramError::Database)?;

        let conflicts = pairs
            .into_iter()
            .map(|(id_a, id_b, from_id, to_id, rel)| Conflict {
                id: 0,
                conflict_type: ConflictType::TemporalInconsistency,
                edge_ids: vec![id_a, id_b],
                description: format!(
                    "Duplicate '{}' edges between nodes {} and {} (ids {} and {}); possible temporal overlap",
                    rel, from_id, to_id, id_a, id_b
                ),
                severity: Severity::Medium,
                resolved_at: None,
                resolution_strategy: None,
            })
            .collect();

        Ok(conflicts)
    }

    /// Detect cycles in the directed edge graph using iterative DFS.
    ///
    /// Returns one conflict per cycle found, listing the edge IDs that form
    /// that cycle.
    pub fn detect_cycles(conn: &Connection) -> Result<Vec<Conflict>> {
        let table_exists = table_exists(conn, "cross_references")?;
        if !table_exists {
            return Ok(Vec::new());
        }

        let edges = load_all_edges(conn)?;

        // Build adjacency list: from_id -> Vec<(to_id, edge_id)>
        let mut adj: HashMap<i64, Vec<(i64, i64)>> = HashMap::new();
        for edge in &edges {
            adj.entry(edge.from_id)
                .or_default()
                .push((edge.to_id, edge.id));
        }

        // Build edge lookup: (from_id, to_id) -> edge_id for path reconstruction.
        let mut edge_map: HashMap<(i64, i64), i64> = HashMap::new();
        for edge in &edges {
            edge_map.insert((edge.from_id, edge.to_id), edge.id);
        }

        let all_nodes: HashSet<i64> = edges
            .iter()
            .flat_map(|e| [e.from_id, e.to_id])
            .collect();

        let mut visited: HashSet<i64> = HashSet::new();
        let mut rec_stack: HashSet<i64> = HashSet::new();
        let mut conflicts = Vec::new();

        for &start in &all_nodes {
            if !visited.contains(&start) {
                dfs_detect_cycle(
                    start,
                    &adj,
                    &edge_map,
                    &mut visited,
                    &mut rec_stack,
                    &mut conflicts,
                );
            }
        }

        Ok(conflicts)
    }

    /// Find edges whose `from_id` or `to_id` do not exist in the `memories`
    /// table.
    pub fn detect_orphans(conn: &Connection) -> Result<Vec<Conflict>> {
        let cr_exists = table_exists(conn, "cross_references")?;
        let mem_exists = table_exists(conn, "memories")?;

        if !cr_exists || !mem_exists {
            return Ok(Vec::new());
        }

        let sql = "
            SELECT cr.id, cr.from_id, cr.to_id
            FROM   cross_references cr
            WHERE  NOT EXISTS (SELECT 1 FROM memories m WHERE m.id = cr.from_id)
               OR  NOT EXISTS (SELECT 1 FROM memories m WHERE m.id = cr.to_id)
        ";

        let mut stmt = conn.prepare(sql).map_err(EngramError::Database)?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(EngramError::Database)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(EngramError::Database)?;

        let conflicts = rows
            .into_iter()
            .map(|(edge_id, from_id, to_id)| Conflict {
                id: 0,
                conflict_type: ConflictType::OrphanedReference,
                edge_ids: vec![edge_id],
                description: format!(
                    "Edge {} references non-existent memory node(s): from_id={}, to_id={}",
                    edge_id, from_id, to_id
                ),
                severity: Severity::Critical,
                resolved_at: None,
                resolution_strategy: None,
            })
            .collect();

        Ok(conflicts)
    }
}

// =============================================================================
// ConflictResolver
// =============================================================================

/// Resolves conflicts and persists them to the `graph_conflicts` table.
pub struct ConflictResolver;

impl ConflictResolver {
    /// Resolve a saved conflict by its ID using the given strategy.
    pub fn resolve(
        conn: &Connection,
        conflict_id: i64,
        strategy: ResolutionStrategy,
    ) -> Result<ResolutionResult> {
        let conflict = Self::get_conflict(conn, conflict_id)?.ok_or_else(|| {
            EngramError::NotFound(conflict_id)
        })?;

        if conflict.resolved_at.is_some() {
            return Err(EngramError::InvalidInput(format!(
                "Conflict {} is already resolved",
                conflict_id
            )));
        }

        let edge_ids = &conflict.edge_ids;

        let (edges_removed, edges_kept) = match strategy {
            ResolutionStrategy::KeepNewer => {
                resolve_keep_newer(conn, edge_ids)?
            }
            ResolutionStrategy::KeepHigherConfidence => {
                resolve_keep_higher_confidence(conn, edge_ids)?
            }
            ResolutionStrategy::Merge => {
                resolve_merge(conn, edge_ids)?
            }
            ResolutionStrategy::Manual => {
                // No edge modifications — just mark resolved.
                (Vec::new(), edge_ids.clone())
            }
        };

        // Mark the conflict as resolved.
        let now = chrono_now();
        conn.execute(
            "UPDATE graph_conflicts
             SET resolved_at = ?1, resolution_strategy = ?2
             WHERE id = ?3",
            params![now, strategy.as_str(), conflict_id],
        )
        .map_err(EngramError::Database)?;

        Ok(ResolutionResult {
            conflict_id,
            strategy,
            edges_removed,
            edges_kept,
        })
    }

    /// Persist a detected conflict to the `graph_conflicts` table.
    ///
    /// Returns the generated row ID.
    pub fn save_conflict(conn: &Connection, conflict: &Conflict) -> Result<i64> {
        let edge_ids_json = serde_json::to_string(&conflict.edge_ids)?;
        let resolution_strategy = conflict
            .resolution_strategy
            .as_ref()
            .map(|s| s.as_str());

        conn.execute(
            "INSERT INTO graph_conflicts
                 (conflict_type, edge_ids, description, severity, resolved_at, resolution_strategy)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                conflict.conflict_type.as_str(),
                edge_ids_json,
                conflict.description,
                conflict.severity.as_str(),
                conflict.resolved_at,
                resolution_strategy,
            ],
        )
        .map_err(EngramError::Database)?;

        Ok(conn.last_insert_rowid())
    }

    /// List conflicts from the `graph_conflicts` table.
    ///
    /// - `resolved = Some(true)`  — only resolved conflicts.
    /// - `resolved = Some(false)` — only unresolved conflicts.
    /// - `resolved = None`        — all conflicts.
    pub fn list_conflicts(conn: &Connection, resolved: Option<bool>) -> Result<Vec<Conflict>> {
        let sql = match resolved {
            Some(true) => {
                "SELECT id, conflict_type, edge_ids, description, severity,
                        resolved_at, resolution_strategy
                 FROM   graph_conflicts
                 WHERE  resolved_at IS NOT NULL
                 ORDER  BY id ASC"
            }
            Some(false) => {
                "SELECT id, conflict_type, edge_ids, description, severity,
                        resolved_at, resolution_strategy
                 FROM   graph_conflicts
                 WHERE  resolved_at IS NULL
                 ORDER  BY id ASC"
            }
            None => {
                "SELECT id, conflict_type, edge_ids, description, severity,
                        resolved_at, resolution_strategy
                 FROM   graph_conflicts
                 ORDER  BY id ASC"
            }
        };

        let mut stmt = conn.prepare(sql).map_err(EngramError::Database)?;

        let rows = stmt
            .query_map([], row_to_conflict)
            .map_err(EngramError::Database)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(EngramError::Database)?;

        Ok(rows)
    }

    /// Retrieve a single conflict by ID.
    pub fn get_conflict(conn: &Connection, id: i64) -> Result<Option<Conflict>> {
        let mut stmt = conn
            .prepare(
                "SELECT id, conflict_type, edge_ids, description, severity,
                        resolved_at, resolution_strategy
                 FROM   graph_conflicts
                 WHERE  id = ?1",
            )
            .map_err(EngramError::Database)?;

        let mut rows = stmt
            .query_map(params![id], row_to_conflict)
            .map_err(EngramError::Database)?;

        match rows.next() {
            Some(row) => Ok(Some(row.map_err(EngramError::Database)?)),
            None => Ok(None),
        }
    }
}

// =============================================================================
// Resolution helpers (private)
// =============================================================================

/// Keep the edge with the highest ID (most recently inserted) and remove the
/// rest.  Returns `(removed, kept)`.
fn resolve_keep_newer(conn: &Connection, edge_ids: &[i64]) -> Result<(Vec<i64>, Vec<i64>)> {
    if edge_ids.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // Load creation timestamps from cross_references.
    let mut id_times: Vec<(i64, String)> = edge_ids
        .iter()
        .filter_map(|&id| {
            let ts: rusqlite::Result<String> = conn.query_row(
                "SELECT created_at FROM cross_references WHERE id = ?1",
                params![id],
                |r| r.get(0),
            );
            ts.ok().map(|t| (id, t))
        })
        .collect();

    // Sort ascending; the last element is the newest.
    id_times.sort_by(|a, b| a.1.cmp(&b.1));

    if id_times.is_empty() {
        return Ok((Vec::new(), edge_ids.to_vec()));
    }

    let newest_id = id_times.last().unwrap().0;
    let to_remove: Vec<i64> = id_times
        .iter()
        .filter(|(id, _)| *id != newest_id)
        .map(|(id, _)| *id)
        .collect();

    for &id in &to_remove {
        conn.execute("DELETE FROM cross_references WHERE id = ?1", params![id])
            .map_err(EngramError::Database)?;
    }

    Ok((to_remove, vec![newest_id]))
}

/// Keep the edge with the highest `strength` (confidence proxy) and remove the
/// rest.  Returns `(removed, kept)`.
fn resolve_keep_higher_confidence(
    conn: &Connection,
    edge_ids: &[i64],
) -> Result<(Vec<i64>, Vec<i64>)> {
    if edge_ids.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // Load strength from cross_references.
    let mut id_strengths: Vec<(i64, f64)> = edge_ids
        .iter()
        .filter_map(|&id| {
            let s: rusqlite::Result<f64> = conn.query_row(
                "SELECT strength FROM cross_references WHERE id = ?1",
                params![id],
                |r| r.get(0),
            );
            s.ok().map(|strength| (id, strength))
        })
        .collect();

    // Sort ascending; last element has highest strength.
    id_strengths.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    if id_strengths.is_empty() {
        return Ok((Vec::new(), edge_ids.to_vec()));
    }

    let best_id = id_strengths.last().unwrap().0;
    let to_remove: Vec<i64> = id_strengths
        .iter()
        .filter(|(id, _)| *id != best_id)
        .map(|(id, _)| *id)
        .collect();

    for &id in &to_remove {
        conn.execute("DELETE FROM cross_references WHERE id = ?1", params![id])
            .map_err(EngramError::Database)?;
    }

    Ok((to_remove, vec![best_id]))
}

/// Merge edges: keep the one with the highest strength, update its metadata to
/// be the JSON merge of all involved edges, then delete the rest.
/// Returns `(removed, kept)`.
fn resolve_merge(conn: &Connection, edge_ids: &[i64]) -> Result<(Vec<i64>, Vec<i64>)> {
    if edge_ids.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // Load all edge metadata.
    let mut rows: Vec<(i64, f64, String)> = edge_ids
        .iter()
        .filter_map(|&id| {
            conn.query_row(
                "SELECT id, strength, metadata FROM cross_references WHERE id = ?1",
                params![id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, f64>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .ok()
        })
        .collect();

    if rows.is_empty() {
        return Ok((Vec::new(), edge_ids.to_vec()));
    }

    // Sort by strength desc; first element is the keeper.
    rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let (keep_id, keep_strength, keep_meta_str) = rows.remove(0);

    // Merge metadata JSON objects.
    let mut merged: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&keep_meta_str)
        .unwrap_or_default();

    for (_, _, meta_str) in &rows {
        if let Ok(serde_json::Value::Object(extra)) = serde_json::from_str(meta_str) {
            for (k, v) in extra {
                merged.entry(k).or_insert(v);
            }
        }
    }

    let merged_str = serde_json::to_string(&serde_json::Value::Object(merged))?;

    conn.execute(
        "UPDATE cross_references SET metadata = ?1, strength = ?2 WHERE id = ?3",
        params![merged_str, keep_strength, keep_id],
    )
    .map_err(EngramError::Database)?;

    let to_remove: Vec<i64> = rows.iter().map(|(id, _, _)| *id).collect();

    for &id in &to_remove {
        conn.execute("DELETE FROM cross_references WHERE id = ?1", params![id])
            .map_err(EngramError::Database)?;
    }

    Ok((to_remove, vec![keep_id]))
}

// =============================================================================
// Private helpers
// =============================================================================

/// Minimal representation of a row in `cross_references`.
#[derive(Debug)]
struct EdgeRow {
    id: i64,
    from_id: i64,
    to_id: i64,
    relation_type: String,
    /// Stored for temporal ordering; not read directly in Rust but used in SQL
    /// ordering.
    #[allow(dead_code)]
    created_at: String,
}

/// Load all rows from `cross_references`. Returns empty vec if table does not
/// exist.
fn load_all_edges(conn: &Connection) -> Result<Vec<EdgeRow>> {
    if !table_exists(conn, "cross_references")? {
        return Ok(Vec::new());
    }

    let mut stmt = conn
        .prepare(
            "SELECT id, from_id, to_id, relation_type, created_at
             FROM   cross_references
             ORDER  BY id ASC",
        )
        .map_err(EngramError::Database)?;

    let rows = stmt
        .query_map([], |row| {
            Ok(EdgeRow {
                id: row.get(0)?,
                from_id: row.get(1)?,
                to_id: row.get(2)?,
                relation_type: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(EngramError::Database)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(EngramError::Database)?;

    Ok(rows)
}

/// Iterative DFS for cycle detection. Detected cycles are appended to
/// `conflicts`.
fn dfs_detect_cycle(
    start: i64,
    adj: &HashMap<i64, Vec<(i64, i64)>>,
    edge_map: &HashMap<(i64, i64), i64>,
    visited: &mut HashSet<i64>,
    rec_stack: &mut HashSet<i64>,
    conflicts: &mut Vec<Conflict>,
) {
    // Stack items: (node, index into adj[node], parent_edge_id)
    let mut stack: Vec<(i64, usize, Option<i64>)> = vec![(start, 0, None)];
    let mut path: Vec<i64> = vec![start];
    let mut path_set: HashSet<i64> = {
        let mut s = HashSet::new();
        s.insert(start);
        s
    };

    visited.insert(start);
    rec_stack.insert(start);

    while let Some((node, idx, _parent_edge)) = stack.last_mut() {
        let node = *node;
        let neighbors = adj.get(&node).map(|v| v.as_slice()).unwrap_or(&[]);

        if *idx < neighbors.len() {
            let (neighbor, edge_id) = neighbors[*idx];
            *idx += 1;

            if !visited.contains(&neighbor) {
                visited.insert(neighbor);
                rec_stack.insert(neighbor);
                path.push(neighbor);
                path_set.insert(neighbor);
                stack.push((neighbor, 0, Some(edge_id)));
            } else if rec_stack.contains(&neighbor) {
                // Cycle detected — collect the edge IDs that form the cycle.
                let cycle_start_pos = path.iter().position(|&n| n == neighbor).unwrap_or(0);
                let cycle_nodes = &path[cycle_start_pos..];
                let mut cycle_edge_ids: Vec<i64> = Vec::new();
                for window in cycle_nodes.windows(2) {
                    if let Some(&eid) = edge_map.get(&(window[0], window[1])) {
                        cycle_edge_ids.push(eid);
                    }
                }
                // Close the cycle: last node -> neighbor
                if let Some(&eid) = edge_map.get(&(*cycle_nodes.last().unwrap_or(&neighbor), neighbor)) {
                    cycle_edge_ids.push(eid);
                }

                if !cycle_edge_ids.is_empty() {
                    conflicts.push(Conflict {
                        id: 0,
                        conflict_type: ConflictType::CyclicDependency,
                        edge_ids: cycle_edge_ids.clone(),
                        description: format!(
                            "Cycle detected involving nodes: {:?}",
                            cycle_nodes
                        ),
                        severity: Severity::Medium,
                        resolved_at: None,
                        resolution_strategy: None,
                    });
                }
            }
        } else {
            // Done with this node — pop.
            stack.pop();
            path.pop();
            path_set.remove(&node);
            rec_stack.remove(&node);
        }
    }
}

/// Check whether a table exists in the current SQLite database.
fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
            |r| r.get(0),
        )
        .map_err(EngramError::Database)?;
    Ok(count > 0)
}

/// Map a rusqlite row to a `Conflict`.
fn row_to_conflict(row: &rusqlite::Row<'_>) -> rusqlite::Result<Conflict> {
    let id: i64 = row.get(0)?;
    let conflict_type_str: String = row.get(1)?;
    let edge_ids_str: String = row.get(2)?;
    let description: String = row.get(3)?;
    let severity_str: String = row.get(4)?;
    let resolved_at: Option<String> = row.get(5)?;
    let resolution_strategy_str: Option<String> = row.get(6)?;

    let conflict_type = ConflictType::from_str(&conflict_type_str)
        .unwrap_or(ConflictType::DirectContradiction);
    let edge_ids: Vec<i64> = serde_json::from_str(&edge_ids_str).unwrap_or_default();
    let severity = Severity::from_str(&severity_str).unwrap_or(Severity::Low);
    let resolution_strategy = resolution_strategy_str
        .as_deref()
        .and_then(ResolutionStrategy::from_str);

    Ok(Conflict {
        id,
        conflict_type,
        edge_ids,
        description,
        severity,
        resolved_at,
        resolution_strategy,
    })
}

/// Return the current UTC timestamp in RFC3339 format.
fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Use chrono if available; otherwise fall back to a formatted timestamp.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(secs as i64, 0)
        .unwrap_or(chrono::Utc::now());
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    const CREATE_CROSS_REFS: &str = "
        CREATE TABLE IF NOT EXISTS cross_references (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id         INTEGER NOT NULL,
            to_id           INTEGER NOT NULL,
            relation_type   TEXT    NOT NULL DEFAULT 'related',
            strength        REAL    NOT NULL DEFAULT 0.5,
            metadata        TEXT    DEFAULT '{}',
            created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );
    ";

    const CREATE_MEMORIES: &str = "
        CREATE TABLE IF NOT EXISTS memories (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            content    TEXT    NOT NULL,
            created_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );
    ";

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory DB");
        conn.execute_batch(CREATE_CROSS_REFS).expect("create cross_references");
        conn.execute_batch(CREATE_MEMORIES).expect("create memories");
        conn.execute_batch(CREATE_CONFLICTS_TABLE).expect("create graph_conflicts");
        conn
    }

    fn insert_edge(conn: &Connection, from_id: i64, to_id: i64, rel: &str, strength: f64) -> i64 {
        conn.execute(
            "INSERT INTO cross_references (from_id, to_id, relation_type, strength)
             VALUES (?1, ?2, ?3, ?4)",
            params![from_id, to_id, rel, strength],
        )
        .expect("insert edge");
        conn.last_insert_rowid()
    }

    fn insert_memory(conn: &Connection, id: i64) {
        conn.execute(
            "INSERT INTO memories (id, content) VALUES (?1, 'test')",
            params![id],
        )
        .expect("insert memory");
    }

    // -------------------------------------------------------------------------
    // Test 1: detect_contradictions — finds contradicting relation pair
    // -------------------------------------------------------------------------
    #[test]
    fn test_detect_contradiction() {
        let conn = setup_db();

        // A --supports--> B
        insert_edge(&conn, 1, 2, "supports", 0.8);
        // A --contradicts--> B  (same pair, contradicting semantics)
        insert_edge(&conn, 1, 2, "contradicts", 0.8);

        let conflicts = ConflictDetector::detect_contradictions(&conn)
            .expect("detect_contradictions");

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_type, ConflictType::DirectContradiction);
        assert_eq!(conflicts[0].severity, Severity::High);
        assert!(conflicts[0].edge_ids.len() >= 2);
        assert!(conflicts[0].description.contains("Contradicting"));
    }

    // -------------------------------------------------------------------------
    // Test 2: detect_temporal_inconsistencies — duplicate triple
    // -------------------------------------------------------------------------
    #[test]
    fn test_detect_temporal_inconsistency() {
        let conn = setup_db();

        // Two edges for the exact same (from, to, relation) triple.
        let id_a = insert_edge(&conn, 10, 20, "works_at", 0.9);
        let id_b = insert_edge(&conn, 10, 20, "works_at", 0.7);

        let conflicts = ConflictDetector::detect_temporal_inconsistencies(&conn)
            .expect("detect_temporal_inconsistencies");

        assert_eq!(conflicts.len(), 1);
        assert_eq!(
            conflicts[0].conflict_type,
            ConflictType::TemporalInconsistency
        );
        assert_eq!(conflicts[0].severity, Severity::Medium);
        assert!(conflicts[0].edge_ids.contains(&id_a));
        assert!(conflicts[0].edge_ids.contains(&id_b));
    }

    // -------------------------------------------------------------------------
    // Test 3: detect_cycles — simple A→B→C→A cycle
    // -------------------------------------------------------------------------
    #[test]
    fn test_detect_cycle() {
        let conn = setup_db();

        // A→B, B→C, C→A forms a cycle.
        insert_edge(&conn, 1, 2, "depends_on", 0.9);
        insert_edge(&conn, 2, 3, "depends_on", 0.9);
        insert_edge(&conn, 3, 1, "depends_on", 0.9); // closes the cycle

        let conflicts = ConflictDetector::detect_cycles(&conn).expect("detect_cycles");

        assert!(
            !conflicts.is_empty(),
            "expected at least one cycle conflict"
        );
        assert_eq!(conflicts[0].conflict_type, ConflictType::CyclicDependency);
        assert!(conflicts[0].description.contains("Cycle"));
    }

    // -------------------------------------------------------------------------
    // Test 4: detect_orphans — edge references missing memory
    // -------------------------------------------------------------------------
    #[test]
    fn test_detect_orphan() {
        let conn = setup_db();

        // Only memory 1 exists; edge references memory 99 which doesn't exist.
        insert_memory(&conn, 1);
        let edge_id = insert_edge(&conn, 1, 99, "related", 0.5); // to_id=99 is orphan

        let conflicts = ConflictDetector::detect_orphans(&conn).expect("detect_orphans");

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_type, ConflictType::OrphanedReference);
        assert_eq!(conflicts[0].severity, Severity::Critical);
        assert!(conflicts[0].edge_ids.contains(&edge_id));
    }

    // -------------------------------------------------------------------------
    // Test 5: resolve with KeepNewer removes older edges
    // -------------------------------------------------------------------------
    #[test]
    fn test_resolve_keep_newer() {
        let conn = setup_db();

        let id_old = insert_edge(&conn, 5, 6, "supports", 0.5);
        // Ensure the second edge has a later created_at by updating it.
        conn.execute(
            "UPDATE cross_references SET created_at = '2099-01-01T00:00:00.000Z' WHERE id = ?1",
            params![id_old + 1],
        )
        .ok();
        let id_new = insert_edge(&conn, 5, 6, "supports", 0.5);
        // Make the new edge newer.
        conn.execute(
            "UPDATE cross_references SET created_at = '2099-01-02T00:00:00.000Z' WHERE id = ?1",
            params![id_new],
        )
        .expect("update ts");

        // Save a conflict manually.
        let conflict = Conflict {
            id: 0,
            conflict_type: ConflictType::TemporalInconsistency,
            edge_ids: vec![id_old, id_new],
            description: "duplicate triple".to_string(),
            severity: Severity::Medium,
            resolved_at: None,
            resolution_strategy: None,
        };
        let cid = ConflictResolver::save_conflict(&conn, &conflict).expect("save");

        let result = ConflictResolver::resolve(&conn, cid, ResolutionStrategy::KeepNewer)
            .expect("resolve");

        assert_eq!(result.conflict_id, cid);
        assert_eq!(result.strategy, ResolutionStrategy::KeepNewer);
        assert_eq!(result.edges_removed.len(), 1);
        assert_eq!(result.edges_kept.len(), 1);
        assert!(result.edges_kept.contains(&id_new));
        assert!(result.edges_removed.contains(&id_old));

        // Verify the conflict is marked resolved.
        let saved = ConflictResolver::get_conflict(&conn, cid)
            .expect("get")
            .expect("exists");
        assert!(saved.resolved_at.is_some());
    }

    // -------------------------------------------------------------------------
    // Test 6: no conflicts when graph is clean
    // -------------------------------------------------------------------------
    #[test]
    fn test_no_conflicts_clean_graph() {
        let conn = setup_db();

        // Insert valid memories and non-contradicting edges.
        insert_memory(&conn, 1);
        insert_memory(&conn, 2);
        insert_memory(&conn, 3);
        insert_edge(&conn, 1, 2, "supports", 0.9);
        insert_edge(&conn, 2, 3, "related", 0.7);

        let all = ConflictDetector::detect_all(&conn).expect("detect_all");

        // No cycles (1→2→3, no back-edge), no orphans, no contradictions, no temporal.
        assert!(all.is_empty(), "expected zero conflicts, got: {:?}", all);
    }

    // -------------------------------------------------------------------------
    // Test 7: save and list conflicts
    // -------------------------------------------------------------------------
    #[test]
    fn test_save_and_list_conflicts() {
        let conn = setup_db();

        let c1 = Conflict {
            id: 0,
            conflict_type: ConflictType::DirectContradiction,
            edge_ids: vec![1, 2],
            description: "supports vs contradicts".to_string(),
            severity: Severity::High,
            resolved_at: None,
            resolution_strategy: None,
        };
        let c2 = Conflict {
            id: 0,
            conflict_type: ConflictType::OrphanedReference,
            edge_ids: vec![3],
            description: "missing node 99".to_string(),
            severity: Severity::Critical,
            resolved_at: None,
            resolution_strategy: None,
        };

        let id1 = ConflictResolver::save_conflict(&conn, &c1).expect("save c1");
        let id2 = ConflictResolver::save_conflict(&conn, &c2).expect("save c2");

        let all = ConflictResolver::list_conflicts(&conn, None).expect("list all");
        assert_eq!(all.len(), 2);

        let unresolved = ConflictResolver::list_conflicts(&conn, Some(false))
            .expect("list unresolved");
        assert_eq!(unresolved.len(), 2);

        let resolved = ConflictResolver::list_conflicts(&conn, Some(true))
            .expect("list resolved");
        assert_eq!(resolved.len(), 0);

        // Verify we can retrieve by ID.
        let fetched = ConflictResolver::get_conflict(&conn, id1)
            .expect("get c1")
            .expect("exists");
        assert_eq!(fetched.conflict_type, ConflictType::DirectContradiction);
        assert_eq!(fetched.severity, Severity::High);

        let fetched2 = ConflictResolver::get_conflict(&conn, id2)
            .expect("get c2")
            .expect("exists");
        assert_eq!(fetched2.conflict_type, ConflictType::OrphanedReference);
    }

    // -------------------------------------------------------------------------
    // Test 8: multiple conflict types in one scan
    // -------------------------------------------------------------------------
    #[test]
    fn test_detect_all_multiple_types() {
        let conn = setup_db();

        // Memory 1 exists, 99 does not → orphan.
        insert_memory(&conn, 1);
        insert_memory(&conn, 2);

        // Contradiction: supports + contradicts between same pair.
        insert_edge(&conn, 1, 2, "supports", 0.8);
        insert_edge(&conn, 1, 2, "contradicts", 0.6);

        // Orphan: edge references non-existent memory 99.
        insert_edge(&conn, 1, 99, "related", 0.5);

        let all = ConflictDetector::detect_all(&conn).expect("detect_all");

        let types: Vec<&ConflictType> = all.iter().map(|c| &c.conflict_type).collect();

        assert!(
            types.contains(&&ConflictType::DirectContradiction),
            "expected DirectContradiction in {:?}",
            types
        );
        assert!(
            types.contains(&&ConflictType::OrphanedReference),
            "expected OrphanedReference in {:?}",
            types
        );
    }

    // -------------------------------------------------------------------------
    // Test 9: resolve already-resolved conflict returns error
    // -------------------------------------------------------------------------
    #[test]
    fn test_resolve_already_resolved_returns_error() {
        let conn = setup_db();

        let id_a = insert_edge(&conn, 5, 6, "rel", 0.5);
        let id_b = insert_edge(&conn, 5, 6, "rel", 0.5);

        let conflict = Conflict {
            id: 0,
            conflict_type: ConflictType::TemporalInconsistency,
            edge_ids: vec![id_a, id_b],
            description: "dup".to_string(),
            severity: Severity::Medium,
            resolved_at: None,
            resolution_strategy: None,
        };
        let cid = ConflictResolver::save_conflict(&conn, &conflict).expect("save");

        // First resolution should succeed.
        ConflictResolver::resolve(&conn, cid, ResolutionStrategy::Manual).expect("first resolve");

        // Second resolution on the same conflict should fail.
        let result = ConflictResolver::resolve(&conn, cid, ResolutionStrategy::Manual);
        assert!(result.is_err(), "expected error on double-resolve");
    }
}
