//! Database queries for memory operations

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Row};
use std::collections::HashMap;

use crate::error::{EngramError, Result};
use crate::types::*;

/// Parse a memory from a database row
pub fn memory_from_row(row: &Row) -> rusqlite::Result<Memory> {
    let id: i64 = row.get("id")?;
    let content: String = row.get("content")?;
    let memory_type_str: String = row.get("memory_type")?;
    let importance: f32 = row.get("importance")?;
    let access_count: i32 = row.get("access_count")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let last_accessed_at: Option<String> = row.get("last_accessed_at")?;
    let owner_id: Option<String> = row.get("owner_id")?;
    let visibility_str: String = row.get("visibility")?;
    let version: i32 = row.get("version")?;
    let has_embedding: i32 = row.get("has_embedding")?;
    let metadata_str: String = row.get("metadata")?;

    let memory_type = memory_type_str.parse().unwrap_or(MemoryType::Note);
    let visibility = match visibility_str.as_str() {
        "shared" => Visibility::Shared,
        "public" => Visibility::Public,
        _ => Visibility::Private,
    };

    let metadata: HashMap<String, serde_json::Value> =
        serde_json::from_str(&metadata_str).unwrap_or_default();

    Ok(Memory {
        id,
        content,
        memory_type,
        tags: vec![], // Loaded separately
        metadata,
        importance,
        access_count,
        created_at: DateTime::parse_from_rfc3339(&created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        updated_at: DateTime::parse_from_rfc3339(&updated_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        last_accessed_at: last_accessed_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        }),
        owner_id,
        visibility,
        version,
        has_embedding: has_embedding != 0,
    })
}

fn metadata_value_to_param(
    key: &str,
    value: &serde_json::Value,
    conditions: &mut Vec<String>,
    params: &mut Vec<Box<dyn rusqlite::ToSql>>,
) -> Result<()> {
    match value {
        serde_json::Value::String(s) => {
            conditions.push(format!("json_extract(m.metadata, '$.{}') = ?", key));
            params.push(Box::new(s.clone()));
        }
        serde_json::Value::Number(n) => {
            conditions.push(format!("json_extract(m.metadata, '$.{}') = ?", key));
            if let Some(i) = n.as_i64() {
                params.push(Box::new(i));
            } else if let Some(f) = n.as_f64() {
                params.push(Box::new(f));
            } else {
                return Err(EngramError::InvalidInput("Invalid number".to_string()));
            }
        }
        serde_json::Value::Bool(b) => {
            conditions.push(format!("json_extract(m.metadata, '$.{}') = ?", key));
            params.push(Box::new(*b));
        }
        serde_json::Value::Null => {
            conditions.push(format!("json_extract(m.metadata, '$.{}') IS NULL", key));
        }
        _ => {
            return Err(EngramError::InvalidInput(format!(
                "Unsupported metadata filter value for key: {}",
                key
            )));
        }
    }

    Ok(())
}

fn get_memory_internal(conn: &Connection, id: i64, track_access: bool) -> Result<Memory> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, content, memory_type, importance, access_count,
                created_at, updated_at, last_accessed_at, owner_id,
                visibility, version, has_embedding, metadata
         FROM memories WHERE id = ? AND valid_to IS NULL",
    )?;

    let mut memory = stmt
        .query_row([id], memory_from_row)
        .map_err(|_| EngramError::NotFound(id))?;

    memory.tags = load_tags(conn, id)?;

    if track_access {
        // Update access tracking
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET access_count = access_count + 1, last_accessed_at = ?
             WHERE id = ?",
            params![now, id],
        )?;
    }

    Ok(memory)
}

/// Load tags for a memory
pub fn load_tags(conn: &Connection, memory_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.name FROM tags t
         JOIN memory_tags mt ON t.id = mt.tag_id
         WHERE mt.memory_id = ?",
    )?;

    let tags: Vec<String> = stmt
        .query_map([memory_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(tags)
}

/// Create a new memory
pub fn create_memory(conn: &Connection, input: &CreateMemoryInput) -> Result<Memory> {
    let now = Utc::now().to_rfc3339();
    let metadata_json = serde_json::to_string(&input.metadata)?;
    let importance = input.importance.unwrap_or(0.5);

    conn.execute(
        "INSERT INTO memories (content, memory_type, importance, metadata, created_at, updated_at, valid_from)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![
            input.content,
            input.memory_type.as_str(),
            importance,
            metadata_json,
            now,
            now,
            now,
        ],
    )?;

    let id = conn.last_insert_rowid();

    // Insert tags
    for tag in &input.tags {
        ensure_tag(conn, tag)?;
        conn.execute(
            "INSERT OR IGNORE INTO memory_tags (memory_id, tag_id)
             SELECT ?, id FROM tags WHERE name = ?",
            params![id, tag],
        )?;
    }

    // Queue for embedding if not deferred
    if !input.defer_embedding {
        conn.execute(
            "INSERT INTO embedding_queue (memory_id, status, queued_at)
             VALUES (?, 'pending', ?)",
            params![id, now],
        )?;
    }

    // Create initial version
    let tags_json = serde_json::to_string(&input.tags)?;
    conn.execute(
        "INSERT INTO memory_versions (memory_id, version, content, tags, metadata, created_at)
         VALUES (?, 1, ?, ?, ?, ?)",
        params![id, input.content, tags_json, metadata_json, now],
    )?;

    // Update sync state
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + 1 WHERE id = 1",
        [],
    )?;

    get_memory_internal(conn, id, false)
}

/// Ensure a tag exists and return its ID
fn ensure_tag(conn: &Connection, tag: &str) -> Result<i64> {
    conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?)", params![tag])?;

    let id: i64 = conn.query_row("SELECT id FROM tags WHERE name = ?", params![tag], |row| {
        row.get(0)
    })?;

    Ok(id)
}

/// Get a memory by ID
pub fn get_memory(conn: &Connection, id: i64) -> Result<Memory> {
    get_memory_internal(conn, id, true)
}

/// Update a memory
pub fn update_memory(conn: &Connection, id: i64, input: &UpdateMemoryInput) -> Result<Memory> {
    // Get current memory for versioning
    let current = get_memory_internal(conn, id, false)?;
    let now = Utc::now().to_rfc3339();

    // Build update query dynamically
    let mut updates = vec!["updated_at = ?".to_string()];
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now.clone())];

    if let Some(ref content) = input.content {
        updates.push("content = ?".to_string());
        values.push(Box::new(content.clone()));
    }

    if let Some(ref memory_type) = input.memory_type {
        updates.push("memory_type = ?".to_string());
        values.push(Box::new(memory_type.as_str().to_string()));
    }

    if let Some(importance) = input.importance {
        updates.push("importance = ?".to_string());
        values.push(Box::new(importance));
    }

    if let Some(ref metadata) = input.metadata {
        let metadata_json = serde_json::to_string(metadata)?;
        updates.push("metadata = ?".to_string());
        values.push(Box::new(metadata_json));
    }

    // Increment version
    updates.push("version = version + 1".to_string());

    // Execute update
    let sql = format!("UPDATE memories SET {} WHERE id = ?", updates.join(", "));
    values.push(Box::new(id));

    let params: Vec<&dyn rusqlite::ToSql> = values.iter().map(|b| b.as_ref()).collect();
    conn.execute(&sql, params.as_slice())?;

    // Update tags if provided
    if let Some(ref tags) = input.tags {
        conn.execute("DELETE FROM memory_tags WHERE memory_id = ?", params![id])?;
        for tag in tags {
            ensure_tag(conn, tag)?;
            conn.execute(
                "INSERT OR IGNORE INTO memory_tags (memory_id, tag_id)
                 SELECT ?, id FROM tags WHERE name = ?",
                params![id, tag],
            )?;
        }
    }

    // Create new version
    let new_content = input.content.as_ref().unwrap_or(&current.content);
    let new_tags = input.tags.as_ref().unwrap_or(&current.tags);
    let new_metadata = input.metadata.as_ref().unwrap_or(&current.metadata);
    let tags_json = serde_json::to_string(new_tags)?;
    let metadata_json = serde_json::to_string(new_metadata)?;

    conn.execute(
        "INSERT INTO memory_versions (memory_id, version, content, tags, metadata, created_at)
         VALUES (?, (SELECT version FROM memories WHERE id = ?), ?, ?, ?, ?)",
        params![id, id, new_content, tags_json, metadata_json, now],
    )?;

    // Re-queue for embedding if content changed
    if input.content.is_some() {
        conn.execute(
            "INSERT OR REPLACE INTO embedding_queue (memory_id, status, queued_at)
             VALUES (?, 'pending', ?)",
            params![id, now],
        )?;
        conn.execute(
            "UPDATE memories SET has_embedding = 0 WHERE id = ?",
            params![id],
        )?;
    }

    // Update sync state
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + 1 WHERE id = 1",
        [],
    )?;

    get_memory_internal(conn, id, false)
}

/// Delete a memory (soft delete by setting valid_to)
pub fn delete_memory(conn: &Connection, id: i64) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    let affected = conn.execute(
        "UPDATE memories SET valid_to = ? WHERE id = ? AND valid_to IS NULL",
        params![now, id],
    )?;

    if affected == 0 {
        return Err(EngramError::NotFound(id));
    }

    // Also invalidate cross-references
    conn.execute(
        "UPDATE crossrefs SET valid_to = ? WHERE (from_id = ? OR to_id = ?) AND valid_to IS NULL",
        params![now, id, id],
    )?;

    // Update sync state
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + 1 WHERE id = 1",
        [],
    )?;

    Ok(())
}

/// List memories with filtering and pagination
pub fn list_memories(conn: &Connection, options: &ListOptions) -> Result<Vec<Memory>> {
    let mut sql = String::from(
        "SELECT DISTINCT m.id, m.content, m.memory_type, m.importance, m.access_count,
                m.created_at, m.updated_at, m.last_accessed_at, m.owner_id,
                m.visibility, m.version, m.has_embedding, m.metadata
         FROM memories m",
    );

    let mut conditions = vec!["m.valid_to IS NULL".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    // Tag filter (requires join)
    if let Some(ref tags) = options.tags {
        if !tags.is_empty() {
            sql.push_str(
                " JOIN memory_tags mt ON m.id = mt.memory_id
                  JOIN tags t ON mt.tag_id = t.id",
            );
            let placeholders: Vec<String> = tags.iter().map(|_| "?".to_string()).collect();
            conditions.push(format!("t.name IN ({})", placeholders.join(", ")));
            for tag in tags {
                params.push(Box::new(tag.clone()));
            }
        }
    }

    // Type filter
    if let Some(ref memory_type) = options.memory_type {
        conditions.push("m.memory_type = ?".to_string());
        params.push(Box::new(memory_type.as_str().to_string()));
    }

    // Metadata filter (JSON)
    if let Some(ref metadata_filter) = options.metadata_filter {
        for (key, value) in metadata_filter {
            metadata_value_to_param(key, value, &mut conditions, &mut params)?;
        }
    }

    sql.push_str(" WHERE ");
    sql.push_str(&conditions.join(" AND "));

    // Sorting
    let sort_field = match options.sort_by.unwrap_or_default() {
        SortField::CreatedAt => "m.created_at",
        SortField::UpdatedAt => "m.updated_at",
        SortField::LastAccessedAt => "m.last_accessed_at",
        SortField::Importance => "m.importance",
        SortField::AccessCount => "m.access_count",
    };
    let sort_order = match options.sort_order.unwrap_or_default() {
        SortOrder::Asc => "ASC",
        SortOrder::Desc => "DESC",
    };
    sql.push_str(&format!(" ORDER BY {} {}", sort_field, sort_order));

    // Pagination
    let limit = options.limit.unwrap_or(100);
    let offset = options.offset.unwrap_or(0);
    sql.push_str(&format!(" LIMIT {} OFFSET {}", limit, offset));

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;

    let memories: Vec<Memory> = stmt
        .query_map(param_refs.as_slice(), memory_from_row)?
        .filter_map(|r| r.ok())
        .map(|mut m| {
            m.tags = load_tags(conn, m.id).unwrap_or_default();
            m
        })
        .collect();

    Ok(memories)
}

/// Create a cross-reference between memories
pub fn create_crossref(conn: &Connection, input: &CreateCrossRefInput) -> Result<CrossReference> {
    let now = Utc::now().to_rfc3339();

    // Verify both memories exist
    let _ = get_memory_internal(conn, input.from_id, false)?;
    let _ = get_memory_internal(conn, input.to_id, false)?;

    let strength = input.strength.unwrap_or(1.0);

    conn.execute(
        "INSERT INTO crossrefs (from_id, to_id, edge_type, score, strength, source, source_context, pinned, created_at, valid_from)
         VALUES (?, ?, ?, 1.0, ?, 'manual', ?, ?, ?, ?)
         ON CONFLICT(from_id, to_id, edge_type) DO UPDATE SET
            strength = excluded.strength,
            source_context = COALESCE(excluded.source_context, crossrefs.source_context),
            pinned = excluded.pinned",
        params![
            input.from_id,
            input.to_id,
            input.edge_type.as_str(),
            strength,
            input.source_context,
            input.pinned,
            now,
            now,
        ],
    )?;

    get_crossref(conn, input.from_id, input.to_id, input.edge_type)
}

/// Get a cross-reference
pub fn get_crossref(
    conn: &Connection,
    from_id: i64,
    to_id: i64,
    edge_type: EdgeType,
) -> Result<CrossReference> {
    let mut stmt = conn.prepare_cached(
        "SELECT from_id, to_id, edge_type, score, confidence, strength, source,
                source_context, created_at, valid_from, valid_to, pinned, metadata
         FROM crossrefs
         WHERE from_id = ? AND to_id = ? AND edge_type = ? AND valid_to IS NULL",
    )?;

    let crossref = stmt.query_row(params![from_id, to_id, edge_type.as_str()], |row| {
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
    })?;

    Ok(crossref)
}

/// Get all cross-references for a memory
pub fn get_related(conn: &Connection, memory_id: i64) -> Result<Vec<CrossReference>> {
    let mut stmt = conn.prepare_cached(
        "SELECT from_id, to_id, edge_type, score, confidence, strength, source,
                source_context, created_at, valid_from, valid_to, pinned, metadata
         FROM crossrefs
         WHERE (from_id = ? OR to_id = ?) AND valid_to IS NULL
         ORDER BY score DESC",
    )?;

    let crossrefs: Vec<CrossReference> = stmt
        .query_map(params![memory_id, memory_id], |row| {
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
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(crossrefs)
}

/// Delete a cross-reference (soft delete)
pub fn delete_crossref(
    conn: &Connection,
    from_id: i64,
    to_id: i64,
    edge_type: EdgeType,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    let affected = conn.execute(
        "UPDATE crossrefs SET valid_to = ?
         WHERE from_id = ? AND to_id = ? AND edge_type = ? AND valid_to IS NULL",
        params![now, from_id, to_id, edge_type.as_str()],
    )?;

    if affected == 0 {
        return Err(EngramError::NotFound(from_id));
    }

    Ok(())
}

/// Get storage statistics
pub fn get_stats(conn: &Connection) -> Result<StorageStats> {
    let total_memories: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memories WHERE valid_to IS NULL",
        [],
        |row| row.get(0),
    )?;

    let total_tags: i64 = conn.query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))?;

    let total_crossrefs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM crossrefs WHERE valid_to IS NULL",
        [],
        |row| row.get(0),
    )?;

    let total_versions: i64 =
        conn.query_row("SELECT COUNT(*) FROM memory_versions", [], |row| row.get(0))?;

    let db_size_bytes: i64 = conn.query_row(
        "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
        [],
        |row| row.get(0),
    )?;

    let memories_with_embeddings: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memories WHERE has_embedding = 1 AND valid_to IS NULL",
        [],
        |row| row.get(0),
    )?;

    let memories_pending_embedding: i64 = conn.query_row(
        "SELECT COUNT(*) FROM embedding_queue WHERE status = 'pending'",
        [],
        |row| row.get(0),
    )?;

    let (last_sync, sync_pending): (Option<String>, i64) = conn.query_row(
        "SELECT last_sync, pending_changes FROM sync_state WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    Ok(StorageStats {
        total_memories,
        total_tags,
        total_crossrefs,
        total_versions,
        db_size_bytes,
        memories_with_embeddings,
        memories_pending_embedding,
        last_sync: last_sync.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        }),
        sync_pending: sync_pending > 0,
    })
}

/// Get memory versions
pub fn get_memory_versions(conn: &Connection, memory_id: i64) -> Result<Vec<MemoryVersion>> {
    let mut stmt = conn.prepare_cached(
        "SELECT version, content, tags, metadata, created_at, created_by, change_summary
         FROM memory_versions WHERE memory_id = ? ORDER BY version DESC",
    )?;

    let versions: Vec<MemoryVersion> = stmt
        .query_map([memory_id], |row| {
            let tags_str: String = row.get("tags")?;
            let metadata_str: String = row.get("metadata")?;
            let created_at_str: String = row.get("created_at")?;

            Ok(MemoryVersion {
                version: row.get("version")?,
                content: row.get("content")?,
                tags: serde_json::from_str(&tags_str).unwrap_or_default(),
                metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
                created_at: DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                created_by: row.get("created_by")?,
                change_summary: row.get("change_summary")?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(versions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn test_list_memories_metadata_filter_types() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_connection(|conn| {
                let mut metadata1 = HashMap::new();
                metadata1.insert("status".to_string(), json!("active"));
                metadata1.insert("count".to_string(), json!(3));
                metadata1.insert("flag".to_string(), json!(true));

                let mut metadata2 = HashMap::new();
                metadata2.insert("status".to_string(), json!("inactive"));
                metadata2.insert("count".to_string(), json!(5));
                metadata2.insert("flag".to_string(), json!(false));
                metadata2.insert("optional".to_string(), json!("set"));

                let memory1 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "First".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: metadata1,
                        importance: None,
                        defer_embedding: true,
                    },
                )?;
                let memory2 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Second".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: metadata2,
                        importance: None,
                        defer_embedding: true,
                    },
                )?;

                let mut filter = HashMap::new();
                filter.insert("status".to_string(), json!("active"));
                let results = list_memories(
                    conn,
                    &ListOptions {
                        metadata_filter: Some(filter),
                        ..Default::default()
                    },
                )?;
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].id, memory1.id);

                let mut filter = HashMap::new();
                filter.insert("count".to_string(), json!(5));
                let results = list_memories(
                    conn,
                    &ListOptions {
                        metadata_filter: Some(filter),
                        ..Default::default()
                    },
                )?;
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].id, memory2.id);

                let mut filter = HashMap::new();
                filter.insert("flag".to_string(), json!(true));
                let results = list_memories(
                    conn,
                    &ListOptions {
                        metadata_filter: Some(filter),
                        ..Default::default()
                    },
                )?;
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].id, memory1.id);

                let mut filter = HashMap::new();
                filter.insert("optional".to_string(), serde_json::Value::Null);
                let results = list_memories(
                    conn,
                    &ListOptions {
                        metadata_filter: Some(filter),
                        ..Default::default()
                    },
                )?;
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].id, memory1.id);

                Ok(())
            })
            .unwrap();
    }
}
