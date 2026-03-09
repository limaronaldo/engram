//! Graph and entity tool handlers.

use serde_json::{json, Value};

use crate::graph::KnowledgeGraph;
use crate::storage::queries::*;
use crate::types::*;

use super::HandlerContext;

pub fn memory_link(ctx: &HandlerContext, params: Value) -> Value {
    let input: CreateCrossRefInput = match serde_json::from_value(params) {
        Ok(i) => i,
        Err(e) => return json!({"error": e.to_string()}),
    };

    ctx.storage
        .with_transaction(|conn| {
            let crossref = create_crossref(conn, &input)?;
            Ok(json!(crossref))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_unlink(ctx: &HandlerContext, params: Value) -> Value {
    let from_id = params.get("from_id").and_then(|v| v.as_i64()).unwrap_or(0);
    let to_id = params.get("to_id").and_then(|v| v.as_i64()).unwrap_or(0);
    let edge_type_str = params
        .get("edge_type")
        .and_then(|v| v.as_str())
        .unwrap_or("related_to");
    let edge_type: EdgeType = edge_type_str.parse().unwrap_or_default();

    ctx.storage
        .with_transaction(|conn| {
            delete_crossref(conn, from_id, to_id, edge_type)?;
            Ok(json!({"unlinked": true}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_related(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::graph_queries::{get_related_multi_hop, TraversalOptions};

    let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let depth = params.get("depth").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    let include_entities = params
        .get("include_entities")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let include_decayed = params
        .get("include_decayed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let edge_type = params
        .get("edge_type")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<EdgeType>().ok());

    if depth <= 1 && !include_entities && !include_decayed {
        return ctx
            .storage
            .with_connection(|conn| {
                let mut related = get_related(conn, id)?;
                if let Some(edge_type) = edge_type {
                    related.retain(|r| r.edge_type == edge_type);
                }
                Ok(json!(related))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}));
    }

    let options = TraversalOptions {
        depth,
        edge_types: edge_type.map(|t| vec![t]).unwrap_or_default(),
        include_entities,
        ..Default::default()
    };

    ctx.storage
        .with_connection(|conn| {
            if include_decayed && depth <= 1 && !include_entities {
                use crate::storage::{get_related_with_decay, DEFAULT_HALF_LIFE_DAYS};

                let mut results = get_related_with_decay(conn, id, DEFAULT_HALF_LIFE_DAYS, 0.0)?;
                if let Some(edge_type) = edge_type {
                    let edge_type = edge_type.as_str();
                    results.retain(|r| r.edge_type == edge_type);
                }
                Ok(json!(results))
            } else {
                let result = get_related_multi_hop(conn, id, &options)?;
                Ok(json!(result))
            }
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_traverse(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::graph_queries::{
        get_related_multi_hop, TraversalDirection, TraversalOptions,
    };

    let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let depth = params.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as usize;
    let include_entities = params
        .get("include_entities")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let min_score = params
        .get("min_score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    let min_confidence = params
        .get("min_confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    let limit_per_hop = params
        .get("limit_per_hop")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let direction = match params.get("direction").and_then(|v| v.as_str()) {
        Some("outgoing") => TraversalDirection::Outgoing,
        Some("incoming") => TraversalDirection::Incoming,
        _ => TraversalDirection::Both,
    };

    let edge_types: Vec<EdgeType> = params
        .get("edge_types")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .filter_map(|s| s.parse().ok())
                .collect()
        })
        .unwrap_or_default();

    let options = TraversalOptions {
        depth,
        edge_types,
        min_score,
        min_confidence,
        limit_per_hop,
        include_entities,
        direction,
    };

    ctx.storage
        .with_connection(|conn| {
            let result = get_related_multi_hop(conn, id, &options)?;
            Ok(json!(result))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn find_path(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::graph_queries::find_path;

    let from_id = params.get("from_id").and_then(|v| v.as_i64()).unwrap_or(0);
    let to_id = params.get("to_id").and_then(|v| v.as_i64()).unwrap_or(0);
    let max_depth = params
        .get("max_depth")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as usize;

    ctx.storage
        .with_connection(|conn| {
            let path = find_path(conn, from_id, to_id, max_depth)?;
            match path {
                Some(node) => Ok(json!({
                    "found": true,
                    "path": node.path,
                    "edge_path": node.edge_path,
                    "depth": node.depth,
                    "cumulative_score": node.cumulative_score,
                    "connection_type": node.connection_type
                })),
                None => Ok(json!({
                    "found": false,
                    "from_id": from_id,
                    "to_id": to_id
                })),
            }
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn export_graph(ctx: &HandlerContext, params: Value) -> Value {
    let format = params
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("html");
    let max_nodes = params
        .get("max_nodes")
        .and_then(|v| v.as_i64())
        .unwrap_or(500);

    ctx.storage
        .with_connection(|conn| {
            let options = ListOptions {
                limit: Some(max_nodes),
                ..Default::default()
            };
            let memories = list_memories(conn, &options)?;

            let mut all_crossrefs = Vec::new();
            for memory in &memories {
                if let Ok(refs) = get_related(conn, memory.id) {
                    all_crossrefs.extend(refs);
                }
            }

            let graph = KnowledgeGraph::from_data(&memories, &all_crossrefs);

            match format {
                "json" => Ok(graph.to_visjs_json()),
                _ => Ok(json!({"html": graph.to_html()})),
            }
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn extract_entities(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{EntityExtractionConfig, EntityExtractor};
    use crate::storage::{link_entity_to_memory, upsert_entity};

    let memory_id = match params
        .get("memory_id")
        .or_else(|| params.get("id"))
        .and_then(|v| v.as_i64())
    {
        Some(id) => id,
        None => return json!({"error": "memory_id (or id) is required"}),
    };

    let min_confidence = params
        .get("min_confidence")
        .and_then(|v| v.as_f64())
        .map(|f| f as f32)
        .unwrap_or(0.5);

    ctx.storage
        .with_transaction(|conn| {
            let memory = get_memory(conn, memory_id)?;

            let config = EntityExtractionConfig {
                min_confidence,
                ..Default::default()
            };
            let extractor = EntityExtractor::new(config);
            let result = extractor.extract(&memory.content);

            let mut stored_entities = Vec::new();
            for extracted in &result.entities {
                let entity_id = upsert_entity(conn, extracted)?;
                let _inserted = link_entity_to_memory(
                    conn,
                    memory_id,
                    entity_id,
                    extracted.suggested_relation,
                    extracted.confidence,
                    Some(extracted.offset),
                )?;

                stored_entities.push(json!({
                    "entity_id": entity_id,
                    "text": extracted.text,
                    "type": extracted.entity_type.as_str(),
                    "confidence": extracted.confidence,
                    "relation": extracted.suggested_relation.as_str(),
                }));
            }

            Ok(json!({
                "memory_id": memory_id,
                "entities_found": result.entities.len(),
                "extraction_time_ms": result.extraction_time_ms,
                "entities": stored_entities
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn get_entities(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::get_entities_for_memory;

    let memory_id = match params
        .get("memory_id")
        .or_else(|| params.get("id"))
        .and_then(|v| v.as_i64())
    {
        Some(id) => id,
        None => return json!({"error": "memory_id (or id) is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let entities = get_entities_for_memory(conn, memory_id)?;

            let result: Vec<_> = entities
                .into_iter()
                .map(|(entity, relation, confidence)| {
                    json!({
                        "id": entity.id,
                        "name": entity.name,
                        "type": entity.entity_type.as_str(),
                        "mention_count": entity.mention_count,
                        "relation": relation.as_str(),
                        "confidence": confidence,
                    })
                })
                .collect();

            Ok(json!({
                "memory_id": memory_id,
                "count": result.len(),
                "entities": result
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn search_entities(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::EntityType;
    use crate::storage::search_entities;

    let query = match params.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return json!({"error": "query is required"}),
    };

    let entity_type: Option<EntityType> = params
        .get("entity_type")
        .or_else(|| params.get("type"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok());

    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);

    ctx.storage
        .with_connection(|conn| {
            let entities = search_entities(conn, query, entity_type, limit)?;

            let result: Vec<_> = entities
                .into_iter()
                .map(|entity| {
                    json!({
                        "id": entity.id,
                        "name": entity.name,
                        "normalized_name": entity.normalized_name,
                        "type": entity.entity_type.as_str(),
                        "mention_count": entity.mention_count,
                        "created_at": entity.created_at.to_rfc3339(),
                    })
                })
                .collect();

            Ok(json!({
                "query": query,
                "count": result.len(),
                "entities": result
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn entity_stats(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::get_entity_stats;

    ctx.storage
        .with_connection(|conn| {
            let stats = get_entity_stats(conn)?;
            Ok(json!(stats))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
