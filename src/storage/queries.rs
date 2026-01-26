//! Database queries for memory operations

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Row};
use sha2::{Digest, Sha256};
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

    // Scope columns (with fallback for backward compatibility)
    let scope_type: String = row
        .get("scope_type")
        .unwrap_or_else(|_| "global".to_string());
    let scope_id: Option<String> = row.get("scope_id").unwrap_or(None);

    // TTL column (with fallback for backward compatibility)
    let expires_at: Option<String> = row.get("expires_at").unwrap_or(None);

    // Content hash column (with fallback for backward compatibility)
    let content_hash: Option<String> = row.get("content_hash").unwrap_or(None);

    let memory_type = memory_type_str.parse().unwrap_or(MemoryType::Note);
    let visibility = match visibility_str.as_str() {
        "shared" => Visibility::Shared,
        "public" => Visibility::Public,
        _ => Visibility::Private,
    };

    // Parse scope from type and id
    let scope = match (scope_type.as_str(), scope_id) {
        ("user", Some(id)) => MemoryScope::User { user_id: id },
        ("session", Some(id)) => MemoryScope::Session { session_id: id },
        ("agent", Some(id)) => MemoryScope::Agent { agent_id: id },
        _ => MemoryScope::Global,
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
        scope,
        version,
        has_embedding: has_embedding != 0,
        expires_at: expires_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        }),
        content_hash,
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
    let now = Utc::now().to_rfc3339();

    let mut stmt = conn.prepare_cached(
        "SELECT id, content, memory_type, importance, access_count,
                created_at, updated_at, last_accessed_at, owner_id,
                visibility, version, has_embedding, metadata,
                scope_type, scope_id, expires_at, content_hash
         FROM memories
         WHERE id = ? AND valid_to IS NULL
           AND (expires_at IS NULL OR expires_at > ?)",
    )?;

    let mut memory = stmt
        .query_row(params![id, now], memory_from_row)
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

/// Compute SHA256 hash of normalized content for deduplication
pub fn compute_content_hash(content: &str) -> String {
    // Normalize: lowercase, collapse whitespace, trim
    let normalized = content
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Find a memory by content hash within the same scope (exact duplicate detection)
///
/// Deduplication respects scope isolation:
/// - User-scoped memories only dedupe against other memories with same user_id
/// - Session-scoped memories only dedupe against other memories with same session_id
/// - Global memories only dedupe against other global memories
pub fn find_by_content_hash(
    conn: &Connection,
    content_hash: &str,
    scope: &MemoryScope,
) -> Result<Option<Memory>> {
    let now = Utc::now().to_rfc3339();
    let scope_type = scope.scope_type();
    let scope_id = scope.scope_id().map(|s| s.to_string());

    let mut stmt = conn.prepare_cached(
        "SELECT id, content, memory_type, importance, access_count,
                created_at, updated_at, last_accessed_at, owner_id,
                visibility, version, has_embedding, metadata,
                scope_type, scope_id, expires_at, content_hash
         FROM memories
         WHERE content_hash = ? AND valid_to IS NULL
           AND (expires_at IS NULL OR expires_at > ?)
           AND scope_type = ?
           AND (scope_id = ? OR (scope_id IS NULL AND ? IS NULL))
         LIMIT 1",
    )?;

    let result = stmt
        .query_row(
            params![content_hash, now, scope_type, scope_id, scope_id],
            memory_from_row,
        )
        .ok();

    if let Some(mut memory) = result {
        memory.tags = load_tags(conn, memory.id)?;
        Ok(Some(memory))
    } else {
        Ok(None)
    }
}

/// Find the most similar memory to given embedding within the same scope (semantic duplicate detection)
///
/// Returns the memory with the highest similarity score if it meets the threshold.
/// Only checks memories that have embeddings computed.
pub fn find_similar_by_embedding(
    conn: &Connection,
    query_embedding: &[f32],
    scope: &MemoryScope,
    threshold: f32,
) -> Result<Option<(Memory, f32)>> {
    use crate::embedding::{cosine_similarity, get_embedding};

    let now = Utc::now().to_rfc3339();
    let scope_type = scope.scope_type();
    let scope_id = scope.scope_id().map(|s| s.to_string());

    // Get all memories with embeddings in the same scope
    let mut stmt = conn.prepare_cached(
        "SELECT id, content, memory_type, importance, access_count,
                created_at, updated_at, last_accessed_at, owner_id,
                visibility, version, has_embedding, metadata,
                scope_type, scope_id, expires_at, content_hash
         FROM memories
         WHERE has_embedding = 1 AND valid_to IS NULL
           AND (expires_at IS NULL OR expires_at > ?)
           AND scope_type = ?
           AND (scope_id = ? OR (scope_id IS NULL AND ? IS NULL))",
    )?;

    let memories: Vec<Memory> = stmt
        .query_map(
            params![now, scope_type, scope_id, scope_id],
            memory_from_row,
        )?
        .filter_map(|r| r.ok())
        .collect();

    let mut best_match: Option<(Memory, f32)> = None;

    for memory in memories {
        if let Ok(Some(embedding)) = get_embedding(conn, memory.id) {
            let similarity = cosine_similarity(query_embedding, &embedding);
            if similarity >= threshold {
                match &best_match {
                    None => best_match = Some((memory, similarity)),
                    Some((_, best_score)) if similarity > *best_score => {
                        best_match = Some((memory, similarity));
                    }
                    _ => {}
                }
            }
        }
    }

    // Load tags for the best match
    if let Some((mut memory, score)) = best_match {
        memory.tags = load_tags(conn, memory.id)?;
        Ok(Some((memory, score)))
    } else {
        Ok(None)
    }
}

/// A pair of potentially duplicate memories with their similarity score
#[derive(Debug, Clone, serde::Serialize)]
pub struct DuplicatePair {
    pub memory_a: Memory,
    pub memory_b: Memory,
    pub similarity_score: f64,
    pub match_type: DuplicateMatchType,
}

/// How the duplicate was detected
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateMatchType {
    /// Exact content hash match
    ExactHash,
    /// High similarity score from crossrefs
    HighSimilarity,
}

/// Find all potential duplicate memory pairs
///
/// Returns pairs of memories that are either:
/// 1. Exact duplicates (same content hash within same scope)
/// 2. High similarity (crossref score >= threshold within same scope)
///
/// Duplicates are scoped - memories in different scopes are not considered duplicates.
pub fn find_duplicates(conn: &Connection, threshold: f64) -> Result<Vec<DuplicatePair>> {
    let now = Utc::now().to_rfc3339();
    let mut duplicates = Vec::new();

    // First, find exact hash duplicates (same content_hash within same scope)
    let mut hash_stmt = conn.prepare_cached(
        "SELECT content_hash, scope_type, scope_id, GROUP_CONCAT(id) as ids
         FROM memories
         WHERE content_hash IS NOT NULL
           AND valid_to IS NULL
           AND (expires_at IS NULL OR expires_at > ?)
         GROUP BY content_hash, scope_type, scope_id
         HAVING COUNT(*) > 1",
    )?;

    let hash_rows = hash_stmt.query_map(params![&now], |row| {
        // Column 3 is now the ids after adding scope_type and scope_id
        let ids_str: String = row.get(3)?;
        Ok(ids_str)
    })?;

    for ids_result in hash_rows {
        let ids_str = ids_result?;
        let ids: Vec<i64> = ids_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        // Create pairs from all IDs with same hash
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let memory_a = get_memory(conn, ids[i])?;
                let memory_b = get_memory(conn, ids[j])?;
                duplicates.push(DuplicatePair {
                    memory_a,
                    memory_b,
                    similarity_score: 1.0, // Exact match
                    match_type: DuplicateMatchType::ExactHash,
                });
            }
        }
    }

    // Second, find high-similarity pairs from crossrefs (within same scope)
    let mut sim_stmt = conn.prepare_cached(
        "SELECT DISTINCT c.from_id, c.to_id, c.score
         FROM crossrefs c
         JOIN memories m1 ON c.from_id = m1.id
         JOIN memories m2 ON c.to_id = m2.id
         WHERE c.score >= ?
           AND m1.valid_to IS NULL
           AND m2.valid_to IS NULL
           AND (m1.expires_at IS NULL OR m1.expires_at > ?)
           AND (m2.expires_at IS NULL OR m2.expires_at > ?)
           AND c.from_id < c.to_id  -- Avoid duplicate pairs
           AND m1.scope_type = m2.scope_type  -- Same scope type
           AND (m1.scope_id = m2.scope_id OR (m1.scope_id IS NULL AND m2.scope_id IS NULL))  -- Same scope id
         ORDER BY c.score DESC",
    )?;

    let sim_rows = sim_stmt.query_map(params![threshold, &now, &now], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, f64>(2)?,
        ))
    })?;

    for row_result in sim_rows {
        let (from_id, to_id, score) = row_result?;

        // Skip if this pair was already found as exact hash match
        let already_found = duplicates.iter().any(|d| {
            (d.memory_a.id == from_id && d.memory_b.id == to_id)
                || (d.memory_a.id == to_id && d.memory_b.id == from_id)
        });

        if !already_found {
            let memory_a = get_memory(conn, from_id)?;
            let memory_b = get_memory(conn, to_id)?;
            duplicates.push(DuplicatePair {
                memory_a,
                memory_b,
                similarity_score: score,
                match_type: DuplicateMatchType::HighSimilarity,
            });
        }
    }

    Ok(duplicates)
}

/// Create a new memory with deduplication support
pub fn create_memory(conn: &Connection, input: &CreateMemoryInput) -> Result<Memory> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let metadata_json = serde_json::to_string(&input.metadata)?;
    let importance = input.importance.unwrap_or(0.5);

    // Compute content hash for deduplication
    let content_hash = compute_content_hash(&input.content);

    // Check for duplicates based on dedup_mode (scoped to same scope)
    if input.dedup_mode != DedupMode::Allow {
        if let Some(existing) = find_by_content_hash(conn, &content_hash, &input.scope)? {
            match input.dedup_mode {
                DedupMode::Reject => {
                    return Err(EngramError::Duplicate {
                        existing_id: existing.id,
                        message: format!(
                            "Duplicate memory detected (id={}). Content hash: {}",
                            existing.id, content_hash
                        ),
                    });
                }
                DedupMode::Skip => {
                    // Return existing memory without modification
                    return Ok(existing);
                }
                DedupMode::Merge => {
                    // Merge: update existing memory with new tags and metadata
                    let mut merged_tags = existing.tags.clone();
                    for tag in &input.tags {
                        if !merged_tags.contains(tag) {
                            merged_tags.push(tag.clone());
                        }
                    }

                    let mut merged_metadata = existing.metadata.clone();
                    for (key, value) in &input.metadata {
                        merged_metadata.insert(key.clone(), value.clone());
                    }

                    let update_input = UpdateMemoryInput {
                        content: None, // Keep existing content
                        memory_type: None,
                        tags: Some(merged_tags),
                        metadata: Some(merged_metadata),
                        importance: input.importance, // Use new importance if provided
                        scope: None,
                        ttl_seconds: input.ttl_seconds, // Apply new TTL if provided
                    };

                    return update_memory(conn, existing.id, &update_input);
                }
                DedupMode::Allow => unreachable!(),
            }
        }
    }

    // Extract scope type and id for database storage
    let scope_type = input.scope.scope_type();
    let scope_id = input.scope.scope_id().map(|s| s.to_string());

    // Calculate expires_at from ttl_seconds
    let expires_at = input
        .ttl_seconds
        .map(|ttl| (now + chrono::Duration::seconds(ttl)).to_rfc3339());

    conn.execute(
        "INSERT INTO memories (content, memory_type, importance, metadata, created_at, updated_at, valid_from, scope_type, scope_id, expires_at, content_hash)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            input.content,
            input.memory_type.as_str(),
            importance,
            metadata_json,
            now_str,
            now_str,
            now_str,
            scope_type,
            scope_id,
            expires_at,
            content_hash,
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
            params![id, now_str],
        )?;
    }

    // Create initial version
    let tags_json = serde_json::to_string(&input.tags)?;
    conn.execute(
        "INSERT INTO memory_versions (memory_id, version, content, tags, metadata, created_at)
         VALUES (?, 1, ?, ?, ?, ?)",
        params![id, input.content, tags_json, metadata_json, now_str],
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
        // Recalculate content_hash when content changes
        let new_hash = compute_content_hash(content);
        updates.push("content_hash = ?".to_string());
        values.push(Box::new(new_hash));
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

    if let Some(ref scope) = input.scope {
        updates.push("scope_type = ?".to_string());
        values.push(Box::new(scope.scope_type().to_string()));
        updates.push("scope_id = ?".to_string());
        values.push(Box::new(scope.scope_id().map(|s| s.to_string())));
    }

    // Handle TTL update
    if let Some(ttl) = input.ttl_seconds {
        if ttl == 0 {
            // Remove expiration
            updates.push("expires_at = NULL".to_string());
        } else {
            // Set new expiration
            let expires_at = (Utc::now() + chrono::Duration::seconds(ttl)).to_rfc3339();
            updates.push("expires_at = ?".to_string());
            values.push(Box::new(expires_at));
        }
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
    let now = Utc::now().to_rfc3339();

    let mut sql = String::from(
        "SELECT DISTINCT m.id, m.content, m.memory_type, m.importance, m.access_count,
                m.created_at, m.updated_at, m.last_accessed_at, m.owner_id,
                m.visibility, m.version, m.has_embedding, m.metadata,
                m.scope_type, m.scope_id, m.expires_at, m.content_hash
         FROM memories m",
    );

    let mut conditions = vec!["m.valid_to IS NULL".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    // Exclude expired memories
    conditions.push("(m.expires_at IS NULL OR m.expires_at > ?)".to_string());
    params.push(Box::new(now));

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

    // Scope filter
    if let Some(ref scope) = options.scope {
        conditions.push("m.scope_type = ?".to_string());
        params.push(Box::new(scope.scope_type().to_string()));
        if let Some(scope_id) = scope.scope_id() {
            conditions.push("m.scope_id = ?".to_string());
            params.push(Box::new(scope_id.to_string()));
        } else {
            conditions.push("m.scope_id IS NULL".to_string());
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

/// Set expiration on an existing memory
///
/// # Arguments
/// * `conn` - Database connection
/// * `id` - Memory ID
/// * `ttl_seconds` - Time-to-live in seconds (0 = remove expiration, None = no change)
pub fn set_memory_expiration(
    conn: &Connection,
    id: i64,
    ttl_seconds: Option<i64>,
) -> Result<Memory> {
    // Verify memory exists and is not expired
    let _ = get_memory_internal(conn, id, false)?;

    match ttl_seconds {
        Some(0) => {
            // Remove expiration
            conn.execute(
                "UPDATE memories SET expires_at = NULL, updated_at = ? WHERE id = ?",
                params![Utc::now().to_rfc3339(), id],
            )?;
        }
        Some(ttl) => {
            // Set new expiration
            let expires_at = (Utc::now() + chrono::Duration::seconds(ttl)).to_rfc3339();
            conn.execute(
                "UPDATE memories SET expires_at = ?, updated_at = ? WHERE id = ?",
                params![expires_at, Utc::now().to_rfc3339(), id],
            )?;
        }
        None => {
            // No change
        }
    }

    // Update sync state
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + 1 WHERE id = 1",
        [],
    )?;

    get_memory_internal(conn, id, false)
}

/// Delete all expired memories (cleanup job)
///
/// Returns the number of memories deleted
pub fn cleanup_expired_memories(conn: &Connection) -> Result<i64> {
    let now = Utc::now().to_rfc3339();

    // Soft delete expired memories by setting valid_to
    let affected = conn.execute(
        "UPDATE memories SET valid_to = ?
         WHERE expires_at IS NOT NULL AND expires_at <= ? AND valid_to IS NULL",
        params![now, now],
    )?;

    if affected > 0 {
        // Also invalidate cross-references involving expired memories
        conn.execute(
            "UPDATE crossrefs SET valid_to = ?
             WHERE valid_to IS NULL AND (
                 from_id IN (SELECT id FROM memories WHERE valid_to IS NOT NULL AND expires_at IS NOT NULL AND expires_at <= ?)
                 OR to_id IN (SELECT id FROM memories WHERE valid_to IS NOT NULL AND expires_at IS NOT NULL AND expires_at <= ?)
             )",
            params![now, now, now],
        )?;

        // Remove memory_entities links for expired memories
        // This ensures expired memories don't appear in entity-based queries
        conn.execute(
            "DELETE FROM memory_entities
             WHERE memory_id IN (
                 SELECT id FROM memories
                 WHERE valid_to IS NOT NULL AND expires_at IS NOT NULL AND expires_at <= ?
             )",
            params![now],
        )?;

        // Remove memory_tags links for expired memories
        conn.execute(
            "DELETE FROM memory_tags
             WHERE memory_id IN (
                 SELECT id FROM memories
                 WHERE valid_to IS NOT NULL AND expires_at IS NOT NULL AND expires_at <= ?
             )",
            params![now],
        )?;

        // Update sync state
        conn.execute(
            "UPDATE sync_state SET pending_changes = pending_changes + ? WHERE id = 1",
            params![affected as i64],
        )?;
    }

    Ok(affected as i64)
}

/// Get count of expired memories (for monitoring)
pub fn count_expired_memories(conn: &Connection) -> Result<i64> {
    let now = Utc::now().to_rfc3339();

    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memories
         WHERE expires_at IS NOT NULL AND expires_at <= ? AND valid_to IS NULL",
        params![now],
        |row| row.get(0),
    )?;

    Ok(count)
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
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
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
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
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

    #[test]
    fn test_memory_scope_isolation() {
        use crate::types::MemoryScope;

        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_connection(|conn| {
                // Create memory with user scope
                let user1_memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "User 1 memory".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["test".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: MemoryScope::user("user-1"),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                // Create memory with different user scope
                let user2_memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "User 2 memory".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["test".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: MemoryScope::user("user-2"),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                // Create memory with session scope
                let session_memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Session memory".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["test".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: MemoryScope::session("session-abc"),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                // Create memory with global scope
                let global_memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Global memory".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["test".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: MemoryScope::Global,
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                // Test: List all memories (no scope filter) should return all 4
                let all_results = list_memories(conn, &ListOptions::default())?;
                assert_eq!(all_results.len(), 4);

                // Test: Filter by user-1 scope should return only user-1's memory
                let user1_results = list_memories(
                    conn,
                    &ListOptions {
                        scope: Some(MemoryScope::user("user-1")),
                        ..Default::default()
                    },
                )?;
                assert_eq!(user1_results.len(), 1);
                assert_eq!(user1_results[0].id, user1_memory.id);
                assert_eq!(user1_results[0].scope, MemoryScope::user("user-1"));

                // Test: Filter by user-2 scope should return only user-2's memory
                let user2_results = list_memories(
                    conn,
                    &ListOptions {
                        scope: Some(MemoryScope::user("user-2")),
                        ..Default::default()
                    },
                )?;
                assert_eq!(user2_results.len(), 1);
                assert_eq!(user2_results[0].id, user2_memory.id);

                // Test: Filter by session scope should return only session memory
                let session_results = list_memories(
                    conn,
                    &ListOptions {
                        scope: Some(MemoryScope::session("session-abc")),
                        ..Default::default()
                    },
                )?;
                assert_eq!(session_results.len(), 1);
                assert_eq!(session_results[0].id, session_memory.id);

                // Test: Filter by global scope should return only global memory
                let global_results = list_memories(
                    conn,
                    &ListOptions {
                        scope: Some(MemoryScope::Global),
                        ..Default::default()
                    },
                )?;
                assert_eq!(global_results.len(), 1);
                assert_eq!(global_results[0].id, global_memory.id);

                // Test: Verify scope is correctly stored and retrieved
                let retrieved = get_memory(conn, user1_memory.id)?;
                assert_eq!(retrieved.scope, MemoryScope::user("user-1"));

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_memory_scope_can_access() {
        use crate::types::MemoryScope;

        // Global can access everything
        assert!(MemoryScope::Global.can_access(&MemoryScope::user("user-1")));
        assert!(MemoryScope::Global.can_access(&MemoryScope::session("session-1")));
        assert!(MemoryScope::Global.can_access(&MemoryScope::agent("agent-1")));
        assert!(MemoryScope::Global.can_access(&MemoryScope::Global));

        // Same scope can access
        assert!(MemoryScope::user("user-1").can_access(&MemoryScope::user("user-1")));
        assert!(MemoryScope::session("s1").can_access(&MemoryScope::session("s1")));
        assert!(MemoryScope::agent("a1").can_access(&MemoryScope::agent("a1")));

        // Different scope IDs cannot access each other
        assert!(!MemoryScope::user("user-1").can_access(&MemoryScope::user("user-2")));
        assert!(!MemoryScope::session("s1").can_access(&MemoryScope::session("s2")));
        assert!(!MemoryScope::agent("a1").can_access(&MemoryScope::agent("a2")));

        // Different scope types cannot access each other
        assert!(!MemoryScope::user("user-1").can_access(&MemoryScope::session("s1")));
        assert!(!MemoryScope::session("s1").can_access(&MemoryScope::agent("a1")));

        // Anyone can access global memories
        assert!(MemoryScope::user("user-1").can_access(&MemoryScope::Global));
        assert!(MemoryScope::session("s1").can_access(&MemoryScope::Global));
        assert!(MemoryScope::agent("a1").can_access(&MemoryScope::Global));
    }

    #[test]
    fn test_memory_ttl_creation() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                // Create memory with TTL of 1 hour
                let memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Temporary memory".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: Some(3600), // 1 hour
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                // Verify expires_at is set
                assert!(memory.expires_at.is_some());
                let expires_at = memory.expires_at.unwrap();
                let now = Utc::now();

                // Should expire approximately 1 hour from now (within 5 seconds tolerance)
                let diff = (expires_at - now).num_seconds();
                assert!(
                    (3595..=3605).contains(&diff),
                    "Expected ~3600 seconds, got {}",
                    diff
                );

                // Create memory without TTL
                let permanent = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Permanent memory".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                // Verify expires_at is None for permanent memory
                assert!(permanent.expires_at.is_none());

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_expired_memories_excluded_from_queries() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                // Create two memories with TTL
                let memory1 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Memory to expire".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["test".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: Some(3600), // 1 hour TTL
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                // Create a permanent memory
                let active = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Active memory".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["test".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                // Both should be visible initially
                let results = list_memories(conn, &ListOptions::default())?;
                assert_eq!(results.len(), 2);

                // Manually expire memory1 by setting expires_at to the past
                let past = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
                conn.execute(
                    "UPDATE memories SET expires_at = ? WHERE id = ?",
                    params![past, memory1.id],
                )?;

                // List should only return active memory now
                let results = list_memories(conn, &ListOptions::default())?;
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].id, active.id);

                // Direct get_memory should fail for expired
                let get_result = get_memory(conn, memory1.id);
                assert!(get_result.is_err());

                // Direct get_memory should succeed for active
                let get_result = get_memory(conn, active.id);
                assert!(get_result.is_ok());

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_set_memory_expiration() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                // Create a permanent memory
                let memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Initially permanent".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                assert!(memory.expires_at.is_none());

                // Set expiration to 30 minutes
                let updated = set_memory_expiration(conn, memory.id, Some(1800))?;
                assert!(updated.expires_at.is_some());

                // Remove expiration (make permanent again) - use Some(0) to clear
                let permanent_again = set_memory_expiration(conn, memory.id, Some(0))?;
                assert!(permanent_again.expires_at.is_none());

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_cleanup_expired_memories() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                // Create 3 memories that we'll expire manually
                let mut expired_ids = vec![];
                for i in 0..3 {
                    let mem = create_memory(
                        conn,
                        &CreateMemoryInput {
                            content: format!("To expire {}", i),
                            memory_type: MemoryType::Note,
                            tags: vec![],
                            metadata: HashMap::new(),
                            importance: None,
                            scope: Default::default(),
                            defer_embedding: true,
                            ttl_seconds: Some(3600), // 1 hour TTL
                            dedup_mode: Default::default(),
                            dedup_threshold: None,
                        },
                    )?;
                    expired_ids.push(mem.id);
                }

                // Create 2 active memories (permanent)
                for i in 0..2 {
                    create_memory(
                        conn,
                        &CreateMemoryInput {
                            content: format!("Active {}", i),
                            memory_type: MemoryType::Note,
                            tags: vec![],
                            metadata: HashMap::new(),
                            importance: None,
                            scope: Default::default(),
                            defer_embedding: true,
                            ttl_seconds: None,
                            dedup_mode: Default::default(),
                            dedup_threshold: None,
                        },
                    )?;
                }

                // All 5 should be visible initially
                let results = list_memories(conn, &ListOptions::default())?;
                assert_eq!(results.len(), 5);

                // Manually expire the first 3 memories
                let past = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
                for id in &expired_ids {
                    conn.execute(
                        "UPDATE memories SET expires_at = ? WHERE id = ?",
                        params![past, id],
                    )?;
                }

                // Count expired
                let expired_count = count_expired_memories(conn)?;
                assert_eq!(expired_count, 3);

                // Cleanup should delete 3
                let deleted = cleanup_expired_memories(conn)?;
                assert_eq!(deleted, 3);

                // Verify only 2 remain
                let remaining = list_memories(conn, &ListOptions::default())?;
                assert_eq!(remaining.len(), 2);

                // No more expired
                let expired_count = count_expired_memories(conn)?;
                assert_eq!(expired_count, 0);

                Ok(())
            })
            .unwrap();
    }

    // ========== Deduplication Tests (RML-931) ==========

    #[test]
    fn test_content_hash_computation() {
        // Test that content hash is consistent and normalized
        let hash1 = compute_content_hash("Hello World");
        let hash2 = compute_content_hash("hello world"); // Different case
        let hash3 = compute_content_hash("  hello   world  "); // Extra whitespace
        let hash4 = compute_content_hash("Hello World!"); // Different content

        // Same normalized content should produce same hash
        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);

        // Different content should produce different hash
        assert_ne!(hash1, hash4);

        // Hash should be prefixed with algorithm
        assert!(hash1.starts_with("sha256:"));
    }

    #[test]
    fn test_dedup_mode_reject() {
        use crate::types::DedupMode;

        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                // Create first memory
                let _memory1 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Unique content for testing".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow, // First one allows
                        dedup_threshold: None,
                    },
                )?;

                // Try to create duplicate with reject mode
                let result = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Unique content for testing".to_string(), // Same content
                        memory_type: MemoryType::Note,
                        tags: vec!["new-tag".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Reject,
                        dedup_threshold: None,
                    },
                );

                // Should fail with Duplicate error
                assert!(result.is_err());
                let err = result.unwrap_err();
                assert!(matches!(err, crate::error::EngramError::Duplicate { .. }));

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_dedup_mode_skip() {
        use crate::types::DedupMode;

        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                // Create first memory
                let memory1 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Skip test content".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["original".to_string()],
                        metadata: HashMap::new(),
                        importance: Some(0.5),
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                    },
                )?;

                // Try to create duplicate with skip mode
                let memory2 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Skip test content".to_string(), // Same content
                        memory_type: MemoryType::Note,
                        tags: vec!["new-tag".to_string()], // Different tags
                        metadata: HashMap::new(),
                        importance: Some(0.9), // Different importance
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Skip,
                        dedup_threshold: None,
                    },
                )?;

                // Should return existing memory unchanged
                assert_eq!(memory1.id, memory2.id);
                assert_eq!(memory2.tags, vec!["original".to_string()]); // Original tags
                assert!((memory2.importance - 0.5).abs() < 0.01); // Original importance

                // Only one memory should exist
                let all = list_memories(conn, &ListOptions::default())?;
                assert_eq!(all.len(), 1);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_dedup_mode_merge() {
        use crate::types::DedupMode;

        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                // Create first memory with some tags and metadata
                let memory1 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Merge test content".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["tag1".to_string(), "tag2".to_string()],
                        metadata: {
                            let mut m = HashMap::new();
                            m.insert("key1".to_string(), serde_json::json!("value1"));
                            m
                        },
                        importance: Some(0.5),
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                    },
                )?;

                // Try to create duplicate with merge mode
                let memory2 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Merge test content".to_string(), // Same content
                        memory_type: MemoryType::Note,
                        tags: vec!["tag2".to_string(), "tag3".to_string()], // Overlapping + new
                        metadata: {
                            let mut m = HashMap::new();
                            m.insert("key2".to_string(), serde_json::json!("value2"));
                            m
                        },
                        importance: Some(0.8), // Higher importance
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Merge,
                        dedup_threshold: None,
                    },
                )?;

                // Should return same memory ID
                assert_eq!(memory1.id, memory2.id);

                // Tags should be merged (no duplicates)
                assert!(memory2.tags.contains(&"tag1".to_string()));
                assert!(memory2.tags.contains(&"tag2".to_string()));
                assert!(memory2.tags.contains(&"tag3".to_string()));
                assert_eq!(memory2.tags.len(), 3);

                // Metadata should be merged
                assert!(memory2.metadata.contains_key("key1"));
                assert!(memory2.metadata.contains_key("key2"));

                // Only one memory should exist
                let all = list_memories(conn, &ListOptions::default())?;
                assert_eq!(all.len(), 1);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_dedup_mode_allow() {
        use crate::types::DedupMode;

        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                // Create first memory
                let memory1 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Allow duplicates content".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                    },
                )?;

                // Create duplicate with allow mode (default)
                let memory2 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Allow duplicates content".to_string(), // Same content
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                    },
                )?;

                // Should create separate memory
                assert_ne!(memory1.id, memory2.id);

                // Both memories should exist
                let all = list_memories(conn, &ListOptions::default())?;
                assert_eq!(all.len(), 2);

                // Both should have same content hash
                assert_eq!(memory1.content_hash, memory2.content_hash);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_find_duplicates_exact_hash() {
        use crate::types::DedupMode;

        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                // Create two memories with same content (exact hash duplicates)
                let _memory1 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Duplicate content".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["first".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                    },
                )?;

                let _memory2 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Duplicate content".to_string(), // Same content
                        memory_type: MemoryType::Note,
                        tags: vec!["second".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                    },
                )?;

                // Create a unique memory (not a duplicate)
                let _memory3 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Unique content".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                    },
                )?;

                // Find duplicates
                let duplicates = find_duplicates(conn, 0.9)?;

                // Should find one duplicate pair
                assert_eq!(duplicates.len(), 1);

                // Should be exact hash match
                assert_eq!(duplicates[0].match_type, DuplicateMatchType::ExactHash);
                assert!((duplicates[0].similarity_score - 1.0).abs() < 0.01);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_content_hash_stored_on_create() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                let memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Test content for hash".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                // Content hash should be set
                assert!(memory.content_hash.is_some());
                let hash = memory.content_hash.as_ref().unwrap();
                assert!(hash.starts_with("sha256:"));

                // Fetch from DB and verify hash is persisted
                let fetched = get_memory(conn, memory.id)?;
                assert_eq!(fetched.content_hash, memory.content_hash);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_update_memory_recalculates_hash() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                // Create a memory
                let memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Original content".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                let original_hash = memory.content_hash.clone();

                // Update the content
                let updated = update_memory(
                    conn,
                    memory.id,
                    &UpdateMemoryInput {
                        content: Some("Updated content".to_string()),
                        memory_type: None,
                        tags: None,
                        metadata: None,
                        importance: None,
                        scope: None,
                        ttl_seconds: None,
                    },
                )?;

                // Hash should be different
                assert_ne!(updated.content_hash, original_hash);
                assert!(updated.content_hash.is_some());

                // Verify against expected hash
                let expected_hash = compute_content_hash("Updated content");
                assert_eq!(updated.content_hash.as_ref().unwrap(), &expected_hash);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_dedup_scope_isolation() {
        use crate::types::{DedupMode, MemoryScope};

        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_transaction(|conn| {
                // Create memory in user-1 scope
                let _user1_memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Shared content".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["user1".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: MemoryScope::user("user-1"),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                    },
                )?;

                // Create same content in user-2 scope with Reject mode
                // Should succeed because scopes are different
                let user2_result = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Shared content".to_string(), // Same content!
                        memory_type: MemoryType::Note,
                        tags: vec!["user2".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: MemoryScope::user("user-2"), // Different scope
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Reject, // Should not reject - different scope
                        dedup_threshold: None,
                    },
                );

                // Should succeed - different scopes are not considered duplicates
                assert!(user2_result.is_ok());
                let _user2_memory = user2_result.unwrap();

                // Now try to create duplicate in same scope (user-2)
                let duplicate_result = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Shared content".to_string(), // Same content
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: MemoryScope::user("user-2"), // Same scope as user2_memory
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Reject, // Should reject - same scope
                        dedup_threshold: None,
                    },
                );

                // Should fail - same scope with same content
                assert!(duplicate_result.is_err());
                assert!(matches!(
                    duplicate_result.unwrap_err(),
                    crate::error::EngramError::Duplicate { .. }
                ));

                // Verify we have exactly 2 memories (one per user)
                let all = list_memories(conn, &ListOptions::default())?;
                assert_eq!(all.len(), 2);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_find_similar_by_embedding() {
        // Helper to store embedding (convert f32 vec to bytes for SQLite)
        fn store_test_embedding(
            conn: &Connection,
            memory_id: i64,
            embedding: &[f32],
        ) -> crate::error::Result<()> {
            let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
            conn.execute(
                "INSERT INTO embeddings (memory_id, embedding, model, dimensions, created_at)
                 VALUES (?, ?, ?, ?, datetime('now'))",
                params![memory_id, bytes, "test", embedding.len() as i32],
            )?;
            // Mark memory as having embedding
            conn.execute(
                "UPDATE memories SET has_embedding = 1 WHERE id = ?",
                params![memory_id],
            )?;
            Ok(())
        }

        let storage = Storage::open_in_memory().unwrap();
        storage
            .with_transaction(|conn| {
                // Create a memory with an embedding
                let memory1 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Rust is a systems programming language".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["rust".to_string()],
                        metadata: std::collections::HashMap::new(),
                        importance: None,
                        scope: MemoryScope::Global,
                        defer_embedding: false,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                    },
                )?;

                // Store an embedding for it (simple test embedding)
                let embedding1 = vec![0.8, 0.4, 0.2, 0.1]; // Normalized-ish vector
                store_test_embedding(conn, memory1.id, &embedding1)?;

                // Create another memory with different embedding
                let memory2 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Python is a scripting language".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["python".to_string()],
                        metadata: std::collections::HashMap::new(),
                        importance: None,
                        scope: MemoryScope::Global,
                        defer_embedding: false,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                    },
                )?;

                // Store a very different embedding
                let embedding2 = vec![0.1, 0.2, 0.8, 0.4]; // Different direction
                store_test_embedding(conn, memory2.id, &embedding2)?;

                // Test 1: Query with embedding similar to memory1
                let query_similar_to_1 = vec![0.79, 0.41, 0.21, 0.11]; // Very similar to embedding1
                let result = find_similar_by_embedding(
                    conn,
                    &query_similar_to_1,
                    &MemoryScope::Global,
                    0.95, // High threshold
                )?;
                assert!(result.is_some());
                let (found_memory, similarity) = result.unwrap();
                assert_eq!(found_memory.id, memory1.id);
                assert!(similarity > 0.95);

                // Test 2: Query with low threshold should still find memory1
                let result_low_threshold = find_similar_by_embedding(
                    conn,
                    &query_similar_to_1,
                    &MemoryScope::Global,
                    0.5,
                )?;
                assert!(result_low_threshold.is_some());

                // Test 3: Query with embedding not similar to anything (threshold too high)
                let query_orthogonal = vec![0.0, 0.0, 0.0, 1.0]; // Different direction
                let result_no_match = find_similar_by_embedding(
                    conn,
                    &query_orthogonal,
                    &MemoryScope::Global,
                    0.99, // Very high threshold
                )?;
                assert!(result_no_match.is_none());

                // Test 4: Different scope should not find anything
                let result_wrong_scope = find_similar_by_embedding(
                    conn,
                    &query_similar_to_1,
                    &MemoryScope::User {
                        user_id: "other-user".to_string(),
                    },
                    0.5,
                )?;
                assert!(result_wrong_scope.is_none());

                Ok(())
            })
            .unwrap();
    }
}
