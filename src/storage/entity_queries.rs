//! Database queries for entity operations (RML-925)

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Row};
use std::collections::HashMap;

use crate::error::{EngramError, Result};
use crate::intelligence::{Entity, EntityRelation, EntityType, ExtractedEntity};
use crate::types::MemoryId;

// =============================================================================
// Entity Queries
// =============================================================================

/// Parse an entity from a database row
fn entity_from_row(row: &Row) -> rusqlite::Result<Entity> {
    let id: i64 = row.get("id")?;
    let name: String = row.get("name")?;
    let normalized_name: String = row.get("normalized_name")?;
    let entity_type_str: String = row.get("entity_type")?;
    let aliases_str: String = row.get("aliases")?;
    let metadata_str: String = row.get("metadata")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let mention_count: i32 = row.get("mention_count")?;

    let entity_type = entity_type_str.parse().unwrap_or(EntityType::Other);
    let aliases: Vec<String> = serde_json::from_str(&aliases_str).unwrap_or_default();
    let metadata: HashMap<String, serde_json::Value> =
        serde_json::from_str(&metadata_str).unwrap_or_default();

    Ok(Entity {
        id,
        name,
        normalized_name,
        entity_type,
        aliases,
        metadata,
        created_at: DateTime::parse_from_rfc3339(&created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        updated_at: DateTime::parse_from_rfc3339(&updated_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        mention_count,
    })
}

/// Create or update an entity, returning its ID
pub fn upsert_entity(conn: &Connection, extracted: &ExtractedEntity) -> Result<i64> {
    let now = Utc::now().to_rfc3339();

    // Try to find existing entity
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM entities WHERE normalized_name = ? AND entity_type = ?",
            params![extracted.normalized, extracted.entity_type.as_str()],
            |row| row.get(0),
        )
        .ok();

    if let Some(id) = existing {
        // Update timestamp only; mention_count is incremented when a new link is created
        conn.execute(
            "UPDATE entities SET updated_at = ? WHERE id = ?",
            params![now, id],
        )?;
        Ok(id)
    } else {
        // Insert new entity with zero mentions; links drive mention_count
        conn.execute(
            "INSERT INTO entities (name, normalized_name, entity_type, created_at, updated_at, mention_count)
             VALUES (?, ?, ?, ?, ?, 0)",
            params![
                extracted.text,
                extracted.normalized,
                extracted.entity_type.as_str(),
                now,
                now,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

/// Link an entity to a memory
pub fn link_entity_to_memory(
    conn: &Connection,
    memory_id: MemoryId,
    entity_id: i64,
    relation: EntityRelation,
    confidence: f32,
    offset: Option<usize>,
) -> Result<bool> {
    let now = Utc::now().to_rfc3339();

    let inserted = conn.execute(
        "INSERT OR IGNORE INTO memory_entities (memory_id, entity_id, relation, confidence, char_offset, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![
            memory_id,
            entity_id,
            relation.as_str(),
            confidence,
            offset.map(|o| o as i64),
            now,
        ],
    )? > 0;

    if inserted {
        conn.execute(
            "UPDATE entities SET mention_count = mention_count + 1, updated_at = ? WHERE id = ?",
            params![now, entity_id],
        )?;
    }

    Ok(inserted)
}

/// Get an entity by ID
pub fn get_entity(conn: &Connection, id: i64) -> Result<Entity> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, normalized_name, entity_type, aliases, metadata,
                created_at, updated_at, mention_count
         FROM entities WHERE id = ?",
    )?;

    stmt.query_row([id], entity_from_row)
        .map_err(|_| EngramError::NotFound(id))
}

/// Find entity by name and type
pub fn find_entity(
    conn: &Connection,
    name: &str,
    entity_type: Option<EntityType>,
) -> Result<Option<Entity>> {
    let normalized = name.trim().to_lowercase();

    let sql = if entity_type.is_some() {
        "SELECT id, name, normalized_name, entity_type, aliases, metadata,
                created_at, updated_at, mention_count
         FROM entities WHERE normalized_name = ? AND entity_type = ?"
    } else {
        "SELECT id, name, normalized_name, entity_type, aliases, metadata,
                created_at, updated_at, mention_count
         FROM entities WHERE normalized_name = ?"
    };

    let mut stmt = conn.prepare(sql)?;

    let result = if let Some(et) = entity_type {
        stmt.query_row(params![normalized, et.as_str()], entity_from_row)
    } else {
        stmt.query_row(params![normalized], entity_from_row)
    };

    match result {
        Ok(entity) => Ok(Some(entity)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(EngramError::from(e)),
    }
}

/// List entities with optional filtering
pub fn list_entities(
    conn: &Connection,
    entity_type: Option<EntityType>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Entity>> {
    let sql = if entity_type.is_some() {
        "SELECT id, name, normalized_name, entity_type, aliases, metadata,
                created_at, updated_at, mention_count
         FROM entities WHERE entity_type = ?
         ORDER BY mention_count DESC, updated_at DESC
         LIMIT ? OFFSET ?"
    } else {
        "SELECT id, name, normalized_name, entity_type, aliases, metadata,
                created_at, updated_at, mention_count
         FROM entities
         ORDER BY mention_count DESC, updated_at DESC
         LIMIT ? OFFSET ?"
    };

    let mut stmt = conn.prepare(sql)?;

    let entities = if let Some(et) = entity_type {
        stmt.query_map(params![et.as_str(), limit, offset], entity_from_row)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map(params![limit, offset], entity_from_row)?
            .filter_map(|r| r.ok())
            .collect()
    };

    Ok(entities)
}

/// Get all entities linked to a memory
pub fn get_entities_for_memory(
    conn: &Connection,
    memory_id: MemoryId,
) -> Result<Vec<(Entity, EntityRelation, f32)>> {
    let mut stmt = conn.prepare(
        "SELECT e.id, e.name, e.normalized_name, e.entity_type, e.aliases, e.metadata,
                e.created_at, e.updated_at, e.mention_count,
                me.relation, me.confidence
         FROM entities e
         JOIN memory_entities me ON e.id = me.entity_id
         WHERE me.memory_id = ?
         ORDER BY me.confidence DESC",
    )?;

    let results: Vec<(Entity, EntityRelation, f32)> = stmt
        .query_map([memory_id], |row| {
            let entity = entity_from_row(row)?;
            let relation_str: String = row.get("relation")?;
            let confidence: f32 = row.get("confidence")?;
            let relation = relation_str.parse().unwrap_or(EntityRelation::Mentions);
            Ok((entity, relation, confidence))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Get all memories that mention an entity
pub fn get_memories_for_entity(
    conn: &Connection,
    entity_id: i64,
) -> Result<Vec<(MemoryId, EntityRelation, f32)>> {
    let mut stmt = conn.prepare(
        "SELECT memory_id, relation, confidence
         FROM memory_entities
         WHERE entity_id = ?
         ORDER BY confidence DESC",
    )?;

    let results: Vec<(MemoryId, EntityRelation, f32)> = stmt
        .query_map([entity_id], |row| {
            let memory_id: MemoryId = row.get("memory_id")?;
            let relation_str: String = row.get("relation")?;
            let confidence: f32 = row.get("confidence")?;
            let relation = relation_str.parse().unwrap_or(EntityRelation::Mentions);
            Ok((memory_id, relation, confidence))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Search entities by name prefix
pub fn search_entities(
    conn: &Connection,
    query: &str,
    entity_type: Option<EntityType>,
    limit: i64,
) -> Result<Vec<Entity>> {
    let pattern = format!("{}%", query.to_lowercase());

    let sql = if entity_type.is_some() {
        "SELECT id, name, normalized_name, entity_type, aliases, metadata,
                created_at, updated_at, mention_count
         FROM entities
         WHERE normalized_name LIKE ? AND entity_type = ?
         ORDER BY mention_count DESC
         LIMIT ?"
    } else {
        "SELECT id, name, normalized_name, entity_type, aliases, metadata,
                created_at, updated_at, mention_count
         FROM entities
         WHERE normalized_name LIKE ?
         ORDER BY mention_count DESC
         LIMIT ?"
    };

    let mut stmt = conn.prepare(sql)?;

    let entities = if let Some(et) = entity_type {
        stmt.query_map(params![pattern, et.as_str(), limit], entity_from_row)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map(params![pattern, limit], entity_from_row)?
            .filter_map(|r| r.ok())
            .collect()
    };

    Ok(entities)
}

/// Delete an entity and its links
pub fn delete_entity(conn: &Connection, id: i64) -> Result<()> {
    // Links are deleted by CASCADE
    let affected = conn.execute("DELETE FROM entities WHERE id = ?", params![id])?;

    if affected == 0 {
        return Err(EngramError::NotFound(id));
    }

    Ok(())
}

/// Remove entity link from a memory
pub fn unlink_entity_from_memory(
    conn: &Connection,
    memory_id: MemoryId,
    entity_id: i64,
) -> Result<()> {
    conn.execute(
        "DELETE FROM memory_entities WHERE memory_id = ? AND entity_id = ?",
        params![memory_id, entity_id],
    )?;

    Ok(())
}

/// Get entity statistics
pub fn get_entity_stats(conn: &Connection) -> Result<EntityStats> {
    let total_entities: i64 =
        conn.query_row("SELECT COUNT(*) FROM entities", [], |row| row.get(0))?;

    let total_links: i64 =
        conn.query_row("SELECT COUNT(*) FROM memory_entities", [], |row| row.get(0))?;

    let by_type: HashMap<String, i64> = {
        let mut stmt =
            conn.prepare("SELECT entity_type, COUNT(*) FROM entities GROUP BY entity_type")?;
        let results: Vec<(String, i64)> = stmt
            .query_map([], |row| {
                let entity_type: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((entity_type, count))
            })?
            .filter_map(|r| r.ok())
            .collect();
        results.into_iter().collect()
    };

    Ok(EntityStats {
        total_entities,
        total_links,
        by_type,
    })
}

/// Entity statistics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntityStats {
    pub total_entities: i64,
    pub total_links: i64,
    pub by_type: HashMap<String, i64>,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    #[test]
    fn test_upsert_and_find_entity() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_connection(|conn| {
                let extracted = ExtractedEntity {
                    text: "Anthropic".to_string(),
                    normalized: "anthropic".to_string(),
                    entity_type: EntityType::Organization,
                    confidence: 0.9,
                    offset: 0,
                    length: 9,
                    suggested_relation: EntityRelation::Mentions,
                };

                // First insert
                let id1 = upsert_entity(conn, &extracted)?;
                assert!(id1 > 0);

                // Second insert should update, not create
                let id2 = upsert_entity(conn, &extracted)?;
                assert_eq!(id1, id2);

                // Verify mention count unchanged (links drive mention_count)
                let entity = get_entity(conn, id1)?;
                assert_eq!(entity.mention_count, 0);
                assert_eq!(entity.name, "Anthropic");

                // Find by name
                let found = find_entity(conn, "anthropic", Some(EntityType::Organization))?;
                assert!(found.is_some());
                assert_eq!(found.unwrap().id, id1);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_link_entity_to_memory() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                use crate::storage::queries::create_memory;
                use crate::types::{CreateMemoryInput, MemoryType};

                // Create a memory
                let memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Testing Anthropic's Claude model".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
                    },
                )?;

                // Create an entity
                let extracted = ExtractedEntity {
                    text: "Anthropic".to_string(),
                    normalized: "anthropic".to_string(),
                    entity_type: EntityType::Organization,
                    confidence: 0.9,
                    offset: 8,
                    length: 9,
                    suggested_relation: EntityRelation::Mentions,
                };
                let entity_id = upsert_entity(conn, &extracted)?;

                // Link them
                let inserted = link_entity_to_memory(
                    conn,
                    memory.id,
                    entity_id,
                    EntityRelation::Mentions,
                    0.9,
                    Some(8),
                )?;
                assert!(inserted);

                // Verify link
                let entities = get_entities_for_memory(conn, memory.id)?;
                assert_eq!(entities.len(), 1);
                assert_eq!(entities[0].0.name, "Anthropic");
                assert_eq!(entities[0].1, EntityRelation::Mentions);
                assert_eq!(entities[0].0.mention_count, 1);

                // Duplicate link should be ignored and not inflate mention_count
                let inserted_again = link_entity_to_memory(
                    conn,
                    memory.id,
                    entity_id,
                    EntityRelation::Mentions,
                    0.9,
                    Some(8),
                )?;
                assert!(!inserted_again);

                let entity = get_entity(conn, entity_id)?;
                assert_eq!(entity.mention_count, 1);

                // Verify reverse lookup
                let memories = get_memories_for_entity(conn, entity_id)?;
                assert_eq!(memories.len(), 1);
                assert_eq!(memories[0].0, memory.id);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_entity_search() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_connection(|conn| {
                // Create some entities
                for name in &["Anthropic", "Apple", "Amazon", "Microsoft"] {
                    let extracted = ExtractedEntity {
                        text: name.to_string(),
                        normalized: name.to_lowercase(),
                        entity_type: EntityType::Organization,
                        confidence: 0.9,
                        offset: 0,
                        length: name.len(),
                        suggested_relation: EntityRelation::Mentions,
                    };
                    upsert_entity(conn, &extracted)?;
                }

                // Search for "a" prefix
                let results = search_entities(conn, "a", Some(EntityType::Organization), 10)?;
                assert_eq!(results.len(), 3); // Anthropic, Apple, Amazon

                // Search for "mi" prefix
                let results = search_entities(conn, "mi", None, 10)?;
                assert_eq!(results.len(), 1); // Microsoft

                Ok(())
            })
            .unwrap();
    }
}
