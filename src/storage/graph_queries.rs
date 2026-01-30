//! Multi-hop graph traversal queries
//!
//! Provides graph traversal capabilities for exploring memory relationships
//! at various depths, with support for filtering by edge type and combining
//! with entity-based connections.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::error::Result;
use crate::types::{CrossReference, EdgeType, MemoryId, RelationSource};
use chrono::{DateTime, Utc};

/// Options for multi-hop graph traversal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalOptions {
    /// Maximum traversal depth (1 = direct relations only)
    #[serde(default = "default_depth")]
    pub depth: usize,
    /// Filter by edge types (empty = all types)
    #[serde(default)]
    pub edge_types: Vec<EdgeType>,
    /// Minimum score threshold
    #[serde(default)]
    pub min_score: f32,
    /// Minimum confidence threshold
    #[serde(default)]
    pub min_confidence: f32,
    /// Maximum number of results per hop
    #[serde(default = "default_limit_per_hop")]
    pub limit_per_hop: usize,
    /// Include entity-based connections
    #[serde(default = "default_include_entities")]
    pub include_entities: bool,
    /// Direction of traversal
    #[serde(default)]
    pub direction: TraversalDirection,
}

fn default_depth() -> usize {
    2
}

fn default_limit_per_hop() -> usize {
    50
}

fn default_include_entities() -> bool {
    true
}

impl Default for TraversalOptions {
    fn default() -> Self {
        Self {
            depth: 2,
            edge_types: vec![],
            min_score: 0.0,
            min_confidence: 0.0,
            limit_per_hop: 50,
            include_entities: true,
            direction: TraversalDirection::Both,
        }
    }
}

/// Direction of graph traversal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TraversalDirection {
    /// Follow outgoing edges only (from -> to)
    Outgoing,
    /// Follow incoming edges only (to -> from)
    Incoming,
    /// Follow edges in both directions
    #[default]
    Both,
}

/// A node in the traversal result with path information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalNode {
    /// Memory ID
    pub memory_id: MemoryId,
    /// Depth from the starting node (0 = start node)
    pub depth: usize,
    /// Path of memory IDs from start to this node
    pub path: Vec<MemoryId>,
    /// Edge types along the path
    pub edge_path: Vec<String>,
    /// Cumulative score (product of edge scores)
    pub cumulative_score: f32,
    /// How this node was reached
    pub connection_type: ConnectionType,
}

/// How a node was reached during traversal
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionType {
    /// Starting node
    Origin,
    /// Connected via cross-reference edge
    CrossReference,
    /// Connected via shared entity
    SharedEntity { entity_name: String },
}

/// Result of a multi-hop traversal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalResult {
    /// Starting memory ID
    pub start_id: MemoryId,
    /// All nodes found during traversal
    pub nodes: Vec<TraversalNode>,
    /// Edges that led to newly discovered nodes
    pub discovery_edges: Vec<CrossReference>,
    /// Statistics about the traversal
    pub stats: TraversalStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TraversalStats {
    /// Total nodes visited
    pub nodes_visited: usize,
    /// Nodes at each depth level
    pub nodes_per_depth: HashMap<usize, usize>,
    /// Count by connection type
    pub connection_type_counts: HashMap<String, usize>,
    /// Maximum depth reached
    pub max_depth_reached: usize,
}

/// Get related memories with multi-hop traversal
pub fn get_related_multi_hop(
    conn: &Connection,
    start_id: MemoryId,
    options: &TraversalOptions,
) -> Result<TraversalResult> {
    let mut visited: HashSet<MemoryId> = HashSet::new();
    let mut nodes: Vec<TraversalNode> = Vec::new();
    let mut discovery_edges: Vec<CrossReference> = Vec::new();
    let mut stats = TraversalStats::default();

    // Queue: (memory_id, depth, path, edge_path, cumulative_score)
    let mut queue: VecDeque<(MemoryId, usize, Vec<MemoryId>, Vec<String>, f32)> = VecDeque::new();

    // Start with the origin node
    visited.insert(start_id);
    nodes.push(TraversalNode {
        memory_id: start_id,
        depth: 0,
        path: vec![start_id],
        edge_path: vec![],
        cumulative_score: 1.0,
        connection_type: ConnectionType::Origin,
    });
    queue.push_back((start_id, 0, vec![start_id], vec![], 1.0));

    *stats.nodes_per_depth.entry(0).or_insert(0) += 1;
    *stats
        .connection_type_counts
        .entry("origin".to_string())
        .or_insert(0) += 1;

    // Level-based BFS traversal
    while !queue.is_empty() {
        let level_size = queue.len();
        let mut current_batch = Vec::with_capacity(level_size);
        for _ in 0..level_size {
            if let Some(item) = queue.pop_front() {
                current_batch.push(item);
            }
        }

        if current_batch.is_empty() {
            break;
        }

        // All nodes in this batch should be at the same depth
        let current_depth = current_batch[0].1;

        if current_depth >= options.depth {
            continue;
        }

        let node_ids: Vec<MemoryId> = current_batch.iter().map(|(id, _, _, _, _)| *id).collect();

        // Batch fetch cross-reference edges (with SQL-level per-node limiting)
        let crossrefs_map = get_edges_for_traversal_batch(
            conn,
            &node_ids,
            &options.edge_types,
            options.min_score,
            options.min_confidence,
            options.direction,
            options.limit_per_hop,
        )?;

        // Batch fetch entity-based connections if enabled
        let entity_connections_map = if options.include_entities {
            get_entity_connections_batch(conn, &node_ids, options.limit_per_hop)?
        } else {
            HashMap::new()
        };

        // Process each node in the batch
        for (current_id, _current_depth, current_path, current_edge_path, current_score) in
            current_batch
        {
            // Process cross-references (already limited per-node in SQL)
            if let Some(crossrefs) = crossrefs_map.get(&current_id) {
                for crossref in crossrefs.iter() {
                    // Determine the neighbor ID based on direction
                    let neighbor_id = if crossref.from_id == current_id {
                        crossref.to_id
                    } else {
                        crossref.from_id
                    };

                    if visited.contains(&neighbor_id) {
                        continue;
                    }

                    visited.insert(neighbor_id);

                    let mut new_path = current_path.clone();
                    new_path.push(neighbor_id);

                    let mut new_edge_path = current_edge_path.clone();
                    new_edge_path.push(crossref.edge_type.as_str().to_string());

                    let new_score = current_score * crossref.score * crossref.confidence;
                    let new_depth = current_depth + 1;

                    nodes.push(TraversalNode {
                        memory_id: neighbor_id,
                        depth: new_depth,
                        path: new_path.clone(),
                        edge_path: new_edge_path.clone(),
                        cumulative_score: new_score,
                        connection_type: ConnectionType::CrossReference,
                    });

                    discovery_edges.push(crossref.clone());

                    *stats.nodes_per_depth.entry(new_depth).or_insert(0) += 1;
                    *stats
                        .connection_type_counts
                        .entry("cross_reference".to_string())
                        .or_insert(0) += 1;

                    if new_depth < options.depth {
                        queue.push_back((
                            neighbor_id,
                            new_depth,
                            new_path,
                            new_edge_path,
                            new_score,
                        ));
                    }

                    stats.max_depth_reached = stats.max_depth_reached.max(new_depth);
                }
            }

            // Process entity connections
            if let Some(entity_connections) = entity_connections_map.get(&current_id) {
                for (neighbor_id, entity_name) in
                    entity_connections.iter().take(options.limit_per_hop)
                {
                    let neighbor_id = *neighbor_id;
                    if visited.contains(&neighbor_id) {
                        continue;
                    }

                    visited.insert(neighbor_id);

                    let mut new_path = current_path.clone();
                    new_path.push(neighbor_id);

                    let mut new_edge_path = current_edge_path.clone();
                    new_edge_path.push(format!("entity:{}", entity_name));

                    let new_depth = current_depth + 1;
                    // Entity connections get a base score of 0.5
                    let new_score = current_score * 0.5;

                    nodes.push(TraversalNode {
                        memory_id: neighbor_id,
                        depth: new_depth,
                        path: new_path.clone(),
                        edge_path: new_edge_path.clone(),
                        cumulative_score: new_score,
                        connection_type: ConnectionType::SharedEntity {
                            entity_name: entity_name.clone(),
                        },
                    });

                    *stats.nodes_per_depth.entry(new_depth).or_insert(0) += 1;
                    *stats
                        .connection_type_counts
                        .entry("shared_entity".to_string())
                        .or_insert(0) += 1;

                    if new_depth < options.depth {
                        queue.push_back((
                            neighbor_id,
                            new_depth,
                            new_path,
                            new_edge_path,
                            new_score,
                        ));
                    }

                    stats.max_depth_reached = stats.max_depth_reached.max(new_depth);
                }
            }
        }
    }

    stats.nodes_visited = nodes.len();

    Ok(TraversalResult {
        start_id,
        nodes,
        discovery_edges,
        stats,
    })
}

/// Get edges for multiple memory IDs with per-node SQL limiting
///
/// Uses ROW_NUMBER() window function to limit results per source node in SQL,
/// preventing memory/time blowup on high-degree nodes.
fn get_edges_for_traversal_batch(
    conn: &Connection,
    memory_ids: &[MemoryId],
    edge_types: &[EdgeType],
    min_score: f32,
    min_confidence: f32,
    direction: TraversalDirection,
    limit_per_node: usize,
) -> Result<HashMap<MemoryId, Vec<CrossReference>>> {
    if memory_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut result: HashMap<MemoryId, Vec<CrossReference>> = HashMap::new();
    let id_set: HashSet<MemoryId> = memory_ids.iter().cloned().collect();

    // SQLite limit safety: chunk the IDs
    for chunk in memory_ids.chunks(100) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");

        let edge_type_clause = if edge_types.is_empty() {
            String::new()
        } else {
            let types: Vec<String> = edge_types
                .iter()
                .map(|e| format!("'{}'", e.as_str()))
                .collect();
            format!(" AND edge_type IN ({})", types.join(", "))
        };

        // Build query based on direction, using ROW_NUMBER() to limit per source node
        let (partition_col, filter_clause) = match direction {
            TraversalDirection::Outgoing => ("from_id", format!("from_id IN ({})", placeholders)),
            TraversalDirection::Incoming => ("to_id", format!("to_id IN ({})", placeholders)),
            TraversalDirection::Both => {
                // For Both direction, we need a UNION approach to properly partition
                // by source node from both directions
                let query = format!(
                    r#"
                    WITH ranked_edges AS (
                        SELECT *, ROW_NUMBER() OVER (
                            PARTITION BY from_id ORDER BY score * confidence DESC
                        ) as rn
                        FROM crossrefs
                        WHERE from_id IN ({placeholders}) AND valid_to IS NULL
                          AND score >= ? AND confidence >= ?
                          {edge_type_clause}
                        UNION ALL
                        SELECT *, ROW_NUMBER() OVER (
                            PARTITION BY to_id ORDER BY score * confidence DESC
                        ) as rn
                        FROM crossrefs
                        WHERE to_id IN ({placeholders}) AND from_id NOT IN ({placeholders}) AND valid_to IS NULL
                          AND score >= ? AND confidence >= ?
                          {edge_type_clause}
                    )
                    SELECT from_id, to_id, edge_type, score, confidence, strength, source,
                           source_context, created_at, valid_from, valid_to, pinned, metadata
                    FROM ranked_edges
                    WHERE rn <= ?
                    "#,
                    placeholders = placeholders,
                    edge_type_clause = edge_type_clause,
                );

                let mut stmt = conn.prepare(&query)?;
                let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

                // First subquery params: from_id IN, min_score, min_confidence
                for id in chunk {
                    params.push(Box::new(*id));
                }
                params.push(Box::new(min_score));
                params.push(Box::new(min_confidence));

                // Second subquery params: to_id IN, from_id NOT IN, min_score, min_confidence
                for id in chunk {
                    params.push(Box::new(*id));
                }
                for id in chunk {
                    params.push(Box::new(*id));
                }
                params.push(Box::new(min_score));
                params.push(Box::new(min_confidence));

                // Limit param
                params.push(Box::new(limit_per_node as i64));

                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();

                let crossrefs = stmt
                    .query_map(param_refs.as_slice(), crossref_from_row)?
                    .filter_map(|r| r.ok());

                for crossref in crossrefs {
                    if id_set.contains(&crossref.from_id) {
                        result
                            .entry(crossref.from_id)
                            .or_default()
                            .push(crossref.clone());
                    }
                    if id_set.contains(&crossref.to_id) && crossref.from_id != crossref.to_id {
                        result.entry(crossref.to_id).or_default().push(crossref);
                    }
                }

                continue; // Skip the common path below for Both direction
            }
        };

        // Common path for Outgoing and Incoming directions
        let query = format!(
            r#"
            WITH ranked_edges AS (
                SELECT *, ROW_NUMBER() OVER (
                    PARTITION BY {partition_col} ORDER BY score * confidence DESC
                ) as rn
                FROM crossrefs
                WHERE {filter_clause} AND valid_to IS NULL
                  AND score >= ? AND confidence >= ?
                  {edge_type_clause}
            )
            SELECT from_id, to_id, edge_type, score, confidence, strength, source,
                   source_context, created_at, valid_from, valid_to, pinned, metadata
            FROM ranked_edges
            WHERE rn <= ?
            "#,
            partition_col = partition_col,
            filter_clause = filter_clause,
            edge_type_clause = edge_type_clause,
        );

        let mut stmt = conn.prepare(&query)?;

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for id in chunk {
            params.push(Box::new(*id));
        }
        params.push(Box::new(min_score));
        params.push(Box::new(min_confidence));
        params.push(Box::new(limit_per_node as i64));

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let crossrefs = stmt
            .query_map(param_refs.as_slice(), crossref_from_row)?
            .filter_map(|r| r.ok());

        for crossref in crossrefs {
            match direction {
                TraversalDirection::Outgoing => {
                    if id_set.contains(&crossref.from_id) {
                        result.entry(crossref.from_id).or_default().push(crossref);
                    }
                }
                TraversalDirection::Incoming => {
                    if id_set.contains(&crossref.to_id) {
                        result.entry(crossref.to_id).or_default().push(crossref);
                    }
                }
                TraversalDirection::Both => unreachable!(), // Handled above with continue
            }
        }
    }

    Ok(result)
}

/// Get memories connected through shared entities for multiple memory IDs
fn get_entity_connections_batch(
    conn: &Connection,
    memory_ids: &[MemoryId],
    _limit: usize,
) -> Result<HashMap<MemoryId, Vec<(MemoryId, String)>>> {
    if memory_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut result: HashMap<MemoryId, Vec<(MemoryId, String)>> = HashMap::new();
    let id_set: HashSet<MemoryId> = memory_ids.iter().cloned().collect();

    for chunk in memory_ids.chunks(100) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");

        let query = format!(
            r#"
            SELECT DISTINCT me1.memory_id, me2.memory_id, e.name
            FROM memory_entities me1
            JOIN memory_entities me2 ON me1.entity_id = me2.entity_id
            JOIN entities e ON me1.entity_id = e.id
            WHERE me1.memory_id IN ({}) AND me2.memory_id != me1.memory_id
            ORDER BY e.mention_count DESC
            "#,
            placeholders
        );

        let mut stmt = conn.prepare(&query)?;

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for id in chunk {
            params.push(Box::new(*id));
        }

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok());

        for (source_id, target_id, entity_name) in rows {
            if id_set.contains(&source_id) {
                result
                    .entry(source_id)
                    .or_default()
                    .push((target_id, entity_name));
            }
        }
    }

    Ok(result)
}

/// Helper to parse CrossReference from row
fn crossref_from_row(row: &rusqlite::Row) -> rusqlite::Result<CrossReference> {
    let edge_type_str: String = row.get("edge_type")?;
    let source_str: String = row.get("source")?;
    let created_at_str: String = row.get("created_at")?;
    let valid_from_str: String = row.get("valid_from")?;
    let valid_to_str: Option<String> = row.get("valid_to")?;
    let metadata_str: String = row.get("metadata")?;

    Ok(CrossReference {
        from_id: row.get("from_id")?,
        to_id: row.get("to_id")?,
        edge_type: edge_type_str.parse().unwrap_or(EdgeType::RelatedTo),
        score: row.get("score")?,
        confidence: row.get("confidence")?,
        strength: row.get("strength")?,
        source: match source_str.as_str() {
            "manual" => RelationSource::Manual,
            "llm" => RelationSource::Llm,
            _ => RelationSource::Auto,
        },
        source_context: row.get("source_context")?,
        created_at: DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        valid_from: DateTime::parse_from_rfc3339(&valid_from_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        valid_to: valid_to_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        }),
        pinned: row.get::<_, i32>("pinned")? != 0,
        metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
    })
}

/// Find shortest path between two memories
pub fn find_path(
    conn: &Connection,
    from_id: MemoryId,
    to_id: MemoryId,
    max_depth: usize,
) -> Result<Option<TraversalNode>> {
    let options = TraversalOptions {
        depth: max_depth,
        include_entities: true,
        ..Default::default()
    };

    let result = get_related_multi_hop(conn, from_id, &options)?;

    // Find the target node in results
    Ok(result.nodes.into_iter().find(|n| n.memory_id == to_id))
}

/// Get all memories within a certain graph distance
pub fn get_neighborhood(
    conn: &Connection,
    center_id: MemoryId,
    radius: usize,
) -> Result<Vec<MemoryId>> {
    let options = TraversalOptions {
        depth: radius,
        include_entities: true,
        ..Default::default()
    };

    let result = get_related_multi_hop(conn, center_id, &options)?;

    Ok(result.nodes.into_iter().map(|n| n.memory_id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intelligence::entities::{EntityRelation, EntityType, ExtractedEntity};
    use crate::storage::entity_queries::{link_entity_to_memory, upsert_entity};
    use crate::storage::queries::{create_crossref, create_memory};
    use crate::storage::Storage;
    use crate::types::{CreateCrossRefInput, CreateMemoryInput, MemoryType};

    fn create_test_memory(conn: &Connection, content: &str) -> MemoryId {
        let input = CreateMemoryInput {
            content: content.to_string(),
            memory_type: MemoryType::Note,
            tags: vec![],
            importance: None,
            metadata: Default::default(),
            scope: Default::default(),
            workspace: None,
            tier: Default::default(),
            defer_embedding: false,
            ttl_seconds: None,
            dedup_mode: Default::default(),
            dedup_threshold: None,
            event_time: None,
            event_duration_seconds: None,
            trigger_pattern: None,
            summary_of_id: None,
        };
        create_memory(conn, &input).unwrap().id
    }

    fn create_test_crossref(
        conn: &Connection,
        from_id: MemoryId,
        to_id: MemoryId,
        edge_type: EdgeType,
    ) -> crate::error::Result<()> {
        let input = CreateCrossRefInput {
            from_id,
            to_id,
            edge_type,
            strength: None,
            source_context: None,
            pinned: false,
        };
        create_crossref(conn, &input)?;
        Ok(())
    }

    #[test]
    fn test_multi_hop_traversal() {
        let storage = Storage::open_in_memory().unwrap();
        storage
            .with_transaction(|conn| {
                // Create a chain: A -> B -> C -> D
                let id_a = create_test_memory(conn, "Memory A");
                let id_b = create_test_memory(conn, "Memory B");
                let id_c = create_test_memory(conn, "Memory C");
                let id_d = create_test_memory(conn, "Memory D");

                // Create edges
                create_test_crossref(conn, id_a, id_b, EdgeType::RelatedTo)?;
                create_test_crossref(conn, id_b, id_c, EdgeType::RelatedTo)?;
                create_test_crossref(conn, id_c, id_d, EdgeType::RelatedTo)?;

                // Traverse from A with depth 1 - should only reach B
                let options = TraversalOptions {
                    depth: 1,
                    include_entities: false,
                    ..Default::default()
                };
                let result = get_related_multi_hop(conn, id_a, &options)?;
                assert_eq!(result.nodes.len(), 2); // A + B
                assert!(result.nodes.iter().any(|n| n.memory_id == id_a));
                assert!(result.nodes.iter().any(|n| n.memory_id == id_b));

                // Traverse from A with depth 2 - should reach B and C
                let options = TraversalOptions {
                    depth: 2,
                    include_entities: false,
                    ..Default::default()
                };
                let result = get_related_multi_hop(conn, id_a, &options)?;
                assert_eq!(result.nodes.len(), 3); // A + B + C
                assert!(result.nodes.iter().any(|n| n.memory_id == id_c));

                // Traverse from A with depth 3 - should reach all
                let options = TraversalOptions {
                    depth: 3,
                    include_entities: false,
                    ..Default::default()
                };
                let result = get_related_multi_hop(conn, id_a, &options)?;
                assert_eq!(result.nodes.len(), 4); // A + B + C + D

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_entity_based_connections() {
        let storage = Storage::open_in_memory().unwrap();
        storage
            .with_transaction(|conn| {
                // Create memories
                let id_a = create_test_memory(conn, "Memory about Rust programming");
                let id_b = create_test_memory(conn, "Another memory about Rust");
                let id_c = create_test_memory(conn, "Memory about Python");

                // Create shared entity using ExtractedEntity
                let entity = ExtractedEntity {
                    text: "Rust".to_string(),
                    normalized: "rust".to_string(),
                    entity_type: EntityType::Concept,
                    confidence: 0.9,
                    offset: 0,
                    length: 4,
                    suggested_relation: EntityRelation::Mentions,
                };
                let entity_id = upsert_entity(conn, &entity)?;
                let _ = link_entity_to_memory(
                    conn,
                    id_a,
                    entity_id,
                    EntityRelation::Mentions,
                    0.9,
                    None,
                )?;
                let _ = link_entity_to_memory(
                    conn,
                    id_b,
                    entity_id,
                    EntityRelation::Mentions,
                    0.8,
                    None,
                )?;

                // Traverse from A with entities enabled
                let options = TraversalOptions {
                    depth: 1,
                    include_entities: true,
                    ..Default::default()
                };
                let result = get_related_multi_hop(conn, id_a, &options)?;

                // Should find B through shared entity
                assert!(result.nodes.iter().any(|n| n.memory_id == id_b));
                let b_node = result.nodes.iter().find(|n| n.memory_id == id_b).unwrap();
                assert!(matches!(
                    &b_node.connection_type,
                    ConnectionType::SharedEntity { entity_name } if entity_name == "Rust"
                ));

                // Should NOT find C (no shared entity)
                assert!(!result.nodes.iter().any(|n| n.memory_id == id_c));

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_find_path() {
        let storage = Storage::open_in_memory().unwrap();
        storage
            .with_transaction(|conn| {
                let id_a = create_test_memory(conn, "Start");
                let id_b = create_test_memory(conn, "Middle");
                let id_c = create_test_memory(conn, "End");

                create_test_crossref(conn, id_a, id_b, EdgeType::RelatedTo)?;
                create_test_crossref(conn, id_b, id_c, EdgeType::DependsOn)?;

                let path = find_path(conn, id_a, id_c, 5)?;
                assert!(path.is_some());
                let path = path.unwrap();
                assert_eq!(path.memory_id, id_c);
                assert_eq!(path.depth, 2);
                assert_eq!(path.path.len(), 3);
                assert_eq!(path.path, vec![id_a, id_b, id_c]);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_traversal_direction() {
        let storage = Storage::open_in_memory().unwrap();
        storage
            .with_transaction(|conn| {
                let id_a = create_test_memory(conn, "A");
                let id_b = create_test_memory(conn, "B");
                let id_c = create_test_memory(conn, "C");

                // A -> B and C -> B (B has incoming from both)
                create_test_crossref(conn, id_a, id_b, EdgeType::RelatedTo)?;
                create_test_crossref(conn, id_c, id_b, EdgeType::RelatedTo)?;

                // Outgoing from B - should find nothing (B has no outgoing)
                let options = TraversalOptions {
                    depth: 1,
                    direction: TraversalDirection::Outgoing,
                    include_entities: false,
                    ..Default::default()
                };
                let result = get_related_multi_hop(conn, id_b, &options)?;
                assert_eq!(result.nodes.len(), 1); // Just B itself

                // Incoming to B - should find A and C
                let options = TraversalOptions {
                    depth: 1,
                    direction: TraversalDirection::Incoming,
                    include_entities: false,
                    ..Default::default()
                };
                let result = get_related_multi_hop(conn, id_b, &options)?;
                assert_eq!(result.nodes.len(), 3); // B, A, C

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_edge_type_filter() {
        let storage = Storage::open_in_memory().unwrap();
        storage
            .with_transaction(|conn| {
                let id_a = create_test_memory(conn, "A");
                let id_b = create_test_memory(conn, "B");
                let id_c = create_test_memory(conn, "C");

                create_test_crossref(conn, id_a, id_b, EdgeType::RelatedTo)?;
                create_test_crossref(conn, id_a, id_c, EdgeType::DependsOn)?;

                // Filter to only RelatedTo edges
                let options = TraversalOptions {
                    depth: 1,
                    edge_types: vec![EdgeType::RelatedTo],
                    include_entities: false,
                    ..Default::default()
                };
                let result = get_related_multi_hop(conn, id_a, &options)?;
                assert_eq!(result.nodes.len(), 2); // A + B only
                assert!(result.nodes.iter().any(|n| n.memory_id == id_b));
                assert!(!result.nodes.iter().any(|n| n.memory_id == id_c));

                Ok(())
            })
            .unwrap();
    }
}
