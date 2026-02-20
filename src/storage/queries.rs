//! Database queries for memory operations

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
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

    // Workspace column (with fallback for backward compatibility)
    let workspace: String = row
        .get("workspace")
        .unwrap_or_else(|_| "default".to_string());

    // Tier column (with fallback for backward compatibility)
    let tier_str: String = row.get("tier").unwrap_or_else(|_| "permanent".to_string());
    let tier = tier_str.parse().unwrap_or_default();

    let event_time: Option<String> = row.get("event_time").unwrap_or(None);
    let event_duration_seconds: Option<i64> = row.get("event_duration_seconds").unwrap_or(None);
    let trigger_pattern: Option<String> = row.get("trigger_pattern").unwrap_or(None);
    let procedure_success_count: i32 = row.get("procedure_success_count").unwrap_or(0);
    let procedure_failure_count: i32 = row.get("procedure_failure_count").unwrap_or(0);
    let summary_of_id: Option<i64> = row.get("summary_of_id").unwrap_or(None);
    let lifecycle_state_str: Option<String> = row.get("lifecycle_state").unwrap_or(None);

    let lifecycle_state = lifecycle_state_str
        .and_then(|s| s.parse().ok())
        .unwrap_or(crate::types::LifecycleState::Active);

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
        workspace,
        tier,
        version,
        has_embedding: has_embedding != 0,
        expires_at: expires_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        }),
        content_hash,
        event_time: event_time.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        }),
        event_duration_seconds,
        trigger_pattern,
        procedure_success_count,
        procedure_failure_count,
        summary_of_id,
        lifecycle_state,
    })
}

pub(crate) fn metadata_value_to_param(
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
                scope_type, scope_id, workspace, tier, expires_at, content_hash
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

/// Find a memory by content hash within the same scope and workspace (exact duplicate detection)
///
/// Deduplication respects both scope and workspace isolation:
/// - User-scoped memories only dedupe against other memories with same user_id
/// - Session-scoped memories only dedupe against other memories with same session_id
/// - Global memories only dedupe against other global memories
/// - All deduplication is workspace-scoped (memories in different workspaces are never duplicates)
pub fn find_by_content_hash(
    conn: &Connection,
    content_hash: &str,
    scope: &MemoryScope,
    workspace: Option<&str>,
) -> Result<Option<Memory>> {
    let now = Utc::now().to_rfc3339();
    let scope_type = scope.scope_type();
    let scope_id = scope.scope_id().map(|s| s.to_string());
    let workspace = workspace.unwrap_or("default");

    let mut stmt = conn.prepare_cached(
        "SELECT id, content, memory_type, importance, access_count,
                created_at, updated_at, last_accessed_at, owner_id,
                visibility, version, has_embedding, metadata,
                scope_type, scope_id, workspace, tier, expires_at, content_hash
         FROM memories
         WHERE content_hash = ? AND valid_to IS NULL
           AND (expires_at IS NULL OR expires_at > ?)
           AND scope_type = ?
           AND (scope_id = ? OR (scope_id IS NULL AND ? IS NULL))
           AND workspace = ?
         LIMIT 1",
    )?;

    let result = stmt
        .query_row(
            params![content_hash, now, scope_type, scope_id, scope_id, workspace],
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

/// Find the most similar memory to given embedding within the same scope AND workspace (semantic duplicate detection)
///
/// Returns the memory with the highest similarity score if it meets the threshold.
/// Only checks memories that have embeddings computed.
pub fn find_similar_by_embedding(
    conn: &Connection,
    query_embedding: &[f32],
    scope: &MemoryScope,
    workspace: Option<&str>,
    threshold: f32,
) -> Result<Option<(Memory, f32)>> {
    use crate::embedding::{cosine_similarity, get_embedding};

    let now = Utc::now().to_rfc3339();
    let scope_type = scope.scope_type();
    let scope_id = scope.scope_id().map(|s| s.to_string());
    let workspace = workspace.unwrap_or("default");

    // Get all memories with embeddings in the same scope AND workspace
    let mut stmt = conn.prepare_cached(
        "SELECT id, content, memory_type, importance, access_count,
                created_at, updated_at, last_accessed_at, owner_id,
                visibility, version, has_embedding, metadata,
                scope_type, scope_id, workspace, tier, expires_at, content_hash
         FROM memories
         WHERE has_embedding = 1 AND valid_to IS NULL
           AND (expires_at IS NULL OR expires_at > ?)
           AND scope_type = ?
           AND (scope_id = ? OR (scope_id IS NULL AND ? IS NULL))
           AND workspace = ?",
    )?;

    let memories: Vec<Memory> = stmt
        .query_map(
            params![now, scope_type, scope_id, scope_id, workspace],
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
    /// Semantic similarity via embedding cosine distance
    EmbeddingSimilarity,
}

/// Find all potential duplicate memory pairs
///
/// Returns pairs of memories that are either:
/// 1. Exact duplicates (same content hash within same scope)
/// 2. High similarity (crossref score >= threshold within same scope)
///
/// Duplicates are scoped - memories in different scopes are not considered duplicates.
pub fn find_duplicates(conn: &Connection, threshold: f64) -> Result<Vec<DuplicatePair>> {
    find_duplicates_in_workspace(conn, threshold, None)
}

/// Find duplicate memories within a specific workspace (or all if None)
pub fn find_duplicates_in_workspace(
    conn: &Connection,
    threshold: f64,
    workspace: Option<&str>,
) -> Result<Vec<DuplicatePair>> {
    let now = Utc::now().to_rfc3339();
    let mut duplicates = Vec::new();

    // First, find exact hash duplicates (same content_hash within same scope AND workspace)
    let (hash_sql, hash_params): (&str, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(ws) = workspace
    {
        (
            "SELECT content_hash, scope_type, scope_id, GROUP_CONCAT(id) as ids
             FROM memories
             WHERE content_hash IS NOT NULL
               AND valid_to IS NULL
               AND (expires_at IS NULL OR expires_at > ?)
               AND workspace = ?
             GROUP BY content_hash, scope_type, scope_id, workspace
             HAVING COUNT(*) > 1",
            vec![Box::new(now.clone()), Box::new(ws.to_string())],
        )
    } else {
        (
            "SELECT content_hash, scope_type, scope_id, GROUP_CONCAT(id) as ids
             FROM memories
             WHERE content_hash IS NOT NULL
               AND valid_to IS NULL
               AND (expires_at IS NULL OR expires_at > ?)
             GROUP BY content_hash, scope_type, scope_id, workspace
             HAVING COUNT(*) > 1",
            vec![Box::new(now.clone())],
        )
    };

    let mut hash_stmt = conn.prepare_cached(hash_sql)?;
    let hash_rows = hash_stmt.query_map(
        rusqlite::params_from_iter(hash_params.iter().map(|p| p.as_ref())),
        |row| {
            let ids_str: String = row.get(3)?;
            Ok(ids_str)
        },
    )?;

    for ids_result in hash_rows {
        let ids_str = ids_result?;
        let ids: Vec<i64> = ids_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        // Create pairs from all IDs with same hash
        // Use get_memory_internal with track_access=false to avoid inflating access stats
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let memory_a = get_memory_internal(conn, ids[i], false)?;
                let memory_b = get_memory_internal(conn, ids[j], false)?;
                duplicates.push(DuplicatePair {
                    memory_a,
                    memory_b,
                    similarity_score: 1.0, // Exact match
                    match_type: DuplicateMatchType::ExactHash,
                });
            }
        }
    }

    // Second, find high-similarity pairs from crossrefs (within same scope AND workspace)
    let (sim_sql, sim_params): (&str, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(ws) = workspace {
        (
            "SELECT DISTINCT c.from_id, c.to_id, c.score
             FROM crossrefs c
             JOIN memories m1 ON c.from_id = m1.id
             JOIN memories m2 ON c.to_id = m2.id
             WHERE c.score >= ?
               AND m1.valid_to IS NULL
               AND m2.valid_to IS NULL
               AND (m1.expires_at IS NULL OR m1.expires_at > ?)
               AND (m2.expires_at IS NULL OR m2.expires_at > ?)
               AND c.from_id < c.to_id
               AND m1.scope_type = m2.scope_type
               AND (m1.scope_id = m2.scope_id OR (m1.scope_id IS NULL AND m2.scope_id IS NULL))
               AND m1.workspace = ?
               AND m2.workspace = ?
             ORDER BY c.score DESC",
            vec![
                Box::new(threshold),
                Box::new(now.clone()),
                Box::new(now.clone()),
                Box::new(ws.to_string()),
                Box::new(ws.to_string()),
            ],
        )
    } else {
        (
            "SELECT DISTINCT c.from_id, c.to_id, c.score
             FROM crossrefs c
             JOIN memories m1 ON c.from_id = m1.id
             JOIN memories m2 ON c.to_id = m2.id
             WHERE c.score >= ?
               AND m1.valid_to IS NULL
               AND m2.valid_to IS NULL
               AND (m1.expires_at IS NULL OR m1.expires_at > ?)
               AND (m2.expires_at IS NULL OR m2.expires_at > ?)
               AND c.from_id < c.to_id
               AND m1.scope_type = m2.scope_type
               AND (m1.scope_id = m2.scope_id OR (m1.scope_id IS NULL AND m2.scope_id IS NULL))
               AND m1.workspace = m2.workspace
             ORDER BY c.score DESC",
            vec![
                Box::new(threshold),
                Box::new(now.clone()),
                Box::new(now.clone()),
            ],
        )
    };

    let mut sim_stmt = conn.prepare_cached(sim_sql)?;
    let sim_rows = sim_stmt.query_map(
        rusqlite::params_from_iter(sim_params.iter().map(|p| p.as_ref())),
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        },
    )?;

    for row_result in sim_rows {
        let (from_id, to_id, score) = row_result?;

        // Skip if this pair was already found as exact hash match
        let already_found = duplicates.iter().any(|d| {
            (d.memory_a.id == from_id && d.memory_b.id == to_id)
                || (d.memory_a.id == to_id && d.memory_b.id == from_id)
        });

        if !already_found {
            // Use get_memory_internal with track_access=false to avoid inflating access stats
            let memory_a = get_memory_internal(conn, from_id, false)?;
            let memory_b = get_memory_internal(conn, to_id, false)?;
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

/// Find semantically similar memories using embedding cosine similarity.
/// This is "LLM-powered" dedup — goes beyond hash/n-gram matching to detect
/// memories that convey the same information with different wording.
pub fn find_duplicates_by_embedding(
    conn: &Connection,
    threshold: f32,
    workspace: Option<&str>,
    limit: usize,
) -> Result<Vec<DuplicatePair>> {
    use crate::embedding::{cosine_similarity, get_embedding};

    let now = Utc::now().to_rfc3339();

    // Get all memory IDs with embeddings (scoped to workspace if provided)
    let (sql, params_vec): (&str, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(ws) = workspace {
        (
            "SELECT id FROM memories
             WHERE has_embedding = 1 AND valid_to IS NULL
               AND (expires_at IS NULL OR expires_at > ?)
               AND COALESCE(lifecycle_state, 'active') = 'active'
               AND workspace = ?
             ORDER BY id",
            vec![Box::new(now), Box::new(ws.to_string())],
        )
    } else {
        (
            "SELECT id FROM memories
             WHERE has_embedding = 1 AND valid_to IS NULL
               AND (expires_at IS NULL OR expires_at > ?)
               AND COALESCE(lifecycle_state, 'active') = 'active'
             ORDER BY id",
            vec![Box::new(now)],
        )
    };

    let mut stmt = conn.prepare(sql)?;
    let ids: Vec<i64> = stmt
        .query_map(
            rusqlite::params_from_iter(params_vec.iter().map(|p| p.as_ref())),
            |row| row.get(0),
        )?
        .filter_map(|r| r.ok())
        .collect();

    // Load all embeddings into memory for pairwise comparison
    let mut embeddings: Vec<(i64, Vec<f32>)> = Vec::with_capacity(ids.len());
    for &id in &ids {
        if let Ok(Some(emb)) = get_embedding(conn, id) {
            embeddings.push((id, emb));
        }
    }

    let mut duplicates = Vec::new();

    // Pairwise comparison (O(n^2) — bounded by limit)
    for i in 0..embeddings.len() {
        if duplicates.len() >= limit {
            break;
        }
        for j in (i + 1)..embeddings.len() {
            if duplicates.len() >= limit {
                break;
            }
            let sim = cosine_similarity(&embeddings[i].1, &embeddings[j].1);
            if sim >= threshold {
                let memory_a = get_memory_internal(conn, embeddings[i].0, false)?;
                let memory_b = get_memory_internal(conn, embeddings[j].0, false)?;
                duplicates.push(DuplicatePair {
                    memory_a,
                    memory_b,
                    similarity_score: sim as f64,
                    match_type: DuplicateMatchType::EmbeddingSimilarity,
                });
            }
        }
    }

    // Sort by similarity descending
    duplicates.sort_by(|a, b| {
        b.similarity_score
            .partial_cmp(&a.similarity_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

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

    // Normalize workspace early for dedup checking
    let workspace = match &input.workspace {
        Some(ws) => crate::types::normalize_workspace(ws)
            .map_err(|e| EngramError::InvalidInput(format!("Invalid workspace: {}", e)))?,
        None => "default".to_string(),
    };

    // Check for duplicates based on dedup_mode (scoped to same scope AND workspace)
    if input.dedup_mode != DedupMode::Allow {
        if let Some(existing) =
            find_by_content_hash(conn, &content_hash, &input.scope, Some(&workspace))?
        {
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

                        event_time: None,
                        trigger_pattern: None,
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

    // workspace was already normalized above for dedup checking

    // Determine tier and enforce tier invariants
    let tier = input.tier;

    // Calculate expires_at based on tier and ttl_seconds
    // Tier invariants:
    //   - Permanent: expires_at MUST be NULL (cannot expire)
    //   - Daily: expires_at MUST be set (default: created_at + 24h)
    let expires_at = match tier {
        MemoryTier::Permanent => {
            // Permanent memories cannot have an expiration
            if input.ttl_seconds.is_some() && input.ttl_seconds != Some(0) {
                return Err(EngramError::InvalidInput(
                    "Permanent tier memories cannot have a TTL. Use Daily tier for expiring memories.".to_string()
                ));
            }
            None
        }
        MemoryTier::Daily => {
            // Daily memories must have an expiration (default: 24 hours)
            let ttl = input.ttl_seconds.filter(|&t| t > 0).unwrap_or(86400); // 24h default
            Some((now + chrono::Duration::seconds(ttl)).to_rfc3339())
        }
    };

    let event_time = input.event_time.map(|dt| dt.to_rfc3339());

    conn.execute(
        "INSERT INTO memories (content, memory_type, importance, metadata, created_at, updated_at, valid_from, scope_type, scope_id, workspace, tier, expires_at, content_hash, event_time, event_duration_seconds, trigger_pattern, summary_of_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
            workspace,
            tier.as_str(),
            expires_at,
            content_hash,
            event_time,
            input.event_duration_seconds,
            input.trigger_pattern,
            input.summary_of_id,
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

    // Record event for sync delta tracking
    record_event(
        conn,
        MemoryEventType::Created,
        Some(id),
        None,
        serde_json::json!({
            "workspace": input.workspace.as_deref().unwrap_or("default"),
            "memory_type": input.memory_type.as_str(),
        }),
    )?;

    // Update sync state (version now tracks event count for delta sync)
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + 1, version = (SELECT MAX(id) FROM memory_events) WHERE id = 1",
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

    // Update event_time if provided (Some(None) clears)
    if let Some(event_time) = &input.event_time {
        updates.push("event_time = ?".to_string());
        let value = event_time.as_ref().map(|dt| dt.to_rfc3339());
        values.push(Box::new(value));
    }

    // Update trigger_pattern if provided (Some(None) clears)
    if let Some(trigger_pattern) = &input.trigger_pattern {
        updates.push("trigger_pattern = ?".to_string());
        values.push(Box::new(trigger_pattern.clone()));
    }

    // Handle TTL update with tier invariant enforcement
    // Normalize: ttl_seconds <= 0 means "no expiration" (consistent with create_memory)
    // Invariants:
    //   - Permanent tier: expires_at MUST be NULL
    //   - Daily tier: expires_at MUST be set
    if let Some(ttl) = input.ttl_seconds {
        if ttl <= 0 {
            // Request to remove expiration
            // Only allowed for Permanent tier; for Daily tier, this is an error
            if current.tier == MemoryTier::Daily {
                return Err(crate::error::EngramError::InvalidInput(
                    "Cannot remove expiration from a Daily tier memory. Use promote_to_permanent first.".to_string()
                ));
            }
            updates.push("expires_at = NULL".to_string());
        } else {
            // Request to set expiration
            // Only allowed for Daily tier; for Permanent tier, this is an error
            if current.tier == MemoryTier::Permanent {
                return Err(crate::error::EngramError::InvalidInput(
                    "Cannot set expiration on a Permanent tier memory. Permanent memories cannot expire.".to_string()
                ));
            }
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

    // Build list of changed fields for event data
    let mut changed_fields = Vec::new();
    if input.content.is_some() {
        changed_fields.push("content");
    }
    if input.tags.is_some() {
        changed_fields.push("tags");
    }
    if input.metadata.is_some() {
        changed_fields.push("metadata");
    }
    if input.importance.is_some() {
        changed_fields.push("importance");
    }
    if input.ttl_seconds.is_some() {
        changed_fields.push("ttl");
    }

    // Record event for sync delta tracking
    record_event(
        conn,
        MemoryEventType::Updated,
        Some(id),
        None,
        serde_json::json!({
            "changed_fields": changed_fields,
        }),
    )?;

    // Update sync state (version now tracks event count for delta sync)
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + 1, version = (SELECT MAX(id) FROM memory_events) WHERE id = 1",
        [],
    )?;

    get_memory_internal(conn, id, false)
}

/// Promote a memory from Daily tier to Permanent tier.
///
/// This operation:
/// - Changes the tier from Daily to Permanent
/// - Clears the expires_at field (permanent memories cannot expire)
/// - Updates the updated_at timestamp
///
/// # Errors
/// - Returns `NotFound` if memory doesn't exist
/// - Returns `Validation` if memory is already Permanent
pub fn promote_to_permanent(conn: &Connection, id: i64) -> Result<Memory> {
    let memory = get_memory_internal(conn, id, false)?;

    if memory.tier == MemoryTier::Permanent {
        return Err(EngramError::InvalidInput(format!(
            "Memory {} is already in the Permanent tier",
            id
        )));
    }

    let now = Utc::now().to_rfc3339();

    conn.execute(
        "UPDATE memories SET tier = 'permanent', expires_at = NULL, updated_at = ?, version = version + 1 WHERE id = ?",
        params![now, id],
    )?;

    // Record event for sync delta tracking
    record_event(
        conn,
        MemoryEventType::Updated,
        Some(id),
        None,
        serde_json::json!({
            "changed_fields": ["tier", "expires_at"],
            "action": "promote_to_permanent",
        }),
    )?;

    // Update sync state (version now tracks event count for delta sync)
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + 1, version = (SELECT MAX(id) FROM memory_events) WHERE id = 1",
        [],
    )?;

    tracing::info!(memory_id = id, "Promoted memory to permanent tier");

    get_memory_internal(conn, id, false)
}

/// Move a memory to a different workspace.
///
/// # Arguments
/// - `id`: Memory ID
/// - `workspace`: New workspace name (will be normalized)
///
/// # Errors
/// - Returns `NotFound` if memory doesn't exist
/// - Returns `Validation` if workspace name is invalid
pub fn move_to_workspace(conn: &Connection, id: i64, workspace: &str) -> Result<Memory> {
    // Validate workspace exists (by checking the memory exists first)
    let _memory = get_memory_internal(conn, id, false)?;

    // Normalize the workspace name
    let normalized = crate::types::normalize_workspace(workspace)
        .map_err(|e| EngramError::InvalidInput(format!("Invalid workspace: {}", e)))?;

    let now = Utc::now().to_rfc3339();

    conn.execute(
        "UPDATE memories SET workspace = ?, updated_at = ?, version = version + 1 WHERE id = ?",
        params![normalized, now, id],
    )?;

    // Record event for sync delta tracking
    record_event(
        conn,
        MemoryEventType::Updated,
        Some(id),
        None,
        serde_json::json!({
            "changed_fields": ["workspace"],
            "action": "move_to_workspace",
            "new_workspace": normalized,
        }),
    )?;

    // Update sync state (version now tracks event count for delta sync)
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + 1, version = (SELECT MAX(id) FROM memory_events) WHERE id = 1",
        [],
    )?;

    tracing::info!(memory_id = id, workspace = %normalized, "Moved memory to workspace");

    get_memory_internal(conn, id, false)
}

/// List all workspaces with their statistics.
///
/// Returns computed stats for each workspace that has at least one memory.
/// Stats are computed on-demand (not cached at the database level).
pub fn list_workspaces(conn: &Connection) -> Result<Vec<WorkspaceStats>> {
    let now = Utc::now().to_rfc3339();

    let mut stmt = conn.prepare(
        r#"
        SELECT
            workspace,
            COUNT(*) as memory_count,
            SUM(CASE WHEN tier = 'permanent' THEN 1 ELSE 0 END) as permanent_count,
            SUM(CASE WHEN tier = 'daily' THEN 1 ELSE 0 END) as daily_count,
            MIN(created_at) as first_memory_at,
            MAX(created_at) as last_memory_at,
            AVG(importance) as avg_importance
        FROM memories
        WHERE valid_to IS NULL AND (expires_at IS NULL OR expires_at > ?)
        GROUP BY workspace
        ORDER BY memory_count DESC
        "#,
    )?;

    let workspaces: Vec<WorkspaceStats> = stmt
        .query_map(params![now], |row| {
            let workspace: String = row.get(0)?;
            let memory_count: i64 = row.get(1)?;
            let permanent_count: i64 = row.get(2)?;
            let daily_count: i64 = row.get(3)?;
            let first_memory_at: Option<String> = row.get(4)?;
            let last_memory_at: Option<String> = row.get(5)?;
            let avg_importance: Option<f64> = row.get(6)?;

            Ok(WorkspaceStats {
                workspace,
                memory_count,
                permanent_count,
                daily_count,
                first_memory_at: first_memory_at.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
                last_memory_at: last_memory_at.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
                top_tags: vec![], // Loaded separately if needed
                avg_importance: avg_importance.map(|v| v as f32),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(workspaces)
}

/// Get statistics for a specific workspace.
pub fn get_workspace_stats(conn: &Connection, workspace: &str) -> Result<WorkspaceStats> {
    let normalized = crate::types::normalize_workspace(workspace)
        .map_err(|e| EngramError::InvalidInput(format!("Invalid workspace: {}", e)))?;

    let now = Utc::now().to_rfc3339();

    let stats = conn
        .query_row(
            r#"
        SELECT
            workspace,
            COUNT(*) as memory_count,
            SUM(CASE WHEN tier = 'permanent' THEN 1 ELSE 0 END) as permanent_count,
            SUM(CASE WHEN tier = 'daily' THEN 1 ELSE 0 END) as daily_count,
            MIN(created_at) as first_memory_at,
            MAX(created_at) as last_memory_at,
            AVG(importance) as avg_importance
        FROM memories
        WHERE workspace = ? AND valid_to IS NULL AND (expires_at IS NULL OR expires_at > ?)
        GROUP BY workspace
        "#,
            params![normalized, now],
            |row| {
                let workspace: String = row.get(0)?;
                let memory_count: i64 = row.get(1)?;
                let permanent_count: i64 = row.get(2)?;
                let daily_count: i64 = row.get(3)?;
                let first_memory_at: Option<String> = row.get(4)?;
                let last_memory_at: Option<String> = row.get(5)?;
                let avg_importance: Option<f64> = row.get(6)?;

                Ok(WorkspaceStats {
                    workspace,
                    memory_count,
                    permanent_count,
                    daily_count,
                    first_memory_at: first_memory_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    }),
                    last_memory_at: last_memory_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    }),
                    top_tags: vec![],
                    avg_importance: avg_importance.map(|v| v as f32),
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                EngramError::NotFound(0) // Workspace doesn't exist
            }
            _ => EngramError::Database(e),
        })?;

    Ok(stats)
}

/// Delete a workspace by moving all its memories to the default workspace or deleting them.
///
/// # Arguments
/// - `workspace`: Workspace to delete
/// - `move_to_default`: If true, moves memories to "default" workspace. If false, deletes them.
///
/// # Returns
/// Number of memories affected.
pub fn delete_workspace(conn: &Connection, workspace: &str, move_to_default: bool) -> Result<i64> {
    let normalized = crate::types::normalize_workspace(workspace)
        .map_err(|e| EngramError::InvalidInput(format!("Invalid workspace: {}", e)))?;

    if normalized == "default" {
        return Err(EngramError::InvalidInput(
            "Cannot delete the default workspace".to_string(),
        ));
    }

    let now = Utc::now().to_rfc3339();

    // First, get the IDs of all affected memories so we can record individual events
    let affected_ids: Vec<i64> = {
        let mut stmt =
            conn.prepare("SELECT id FROM memories WHERE workspace = ? AND valid_to IS NULL")?;
        let rows = stmt.query_map(params![&normalized], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };

    let affected = affected_ids.len() as i64;

    if affected > 0 {
        if move_to_default {
            // Move all memories to the default workspace
            conn.execute(
                "UPDATE memories SET workspace = 'default', updated_at = ?, version = version + 1 WHERE workspace = ? AND valid_to IS NULL",
                params![&now, &normalized],
            )?;
        } else {
            // Soft delete all memories in the workspace
            conn.execute(
                "UPDATE memories SET valid_to = ? WHERE workspace = ? AND valid_to IS NULL",
                params![&now, &normalized],
            )?;
        }

        // Record individual events for each affected memory (for proper sync delta tracking)
        let event_type = if move_to_default {
            MemoryEventType::Updated
        } else {
            MemoryEventType::Deleted
        };

        for memory_id in &affected_ids {
            record_event(
                conn,
                event_type.clone(),
                Some(*memory_id),
                None,
                serde_json::json!({
                    "action": "delete_workspace",
                    "workspace": normalized,
                    "move_to_default": move_to_default,
                }),
            )?;
        }
    }

    // Update sync state (version now tracks event count for delta sync)
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + ?, version = (SELECT COALESCE(MAX(id), 0) FROM memory_events) WHERE id = 1",
        params![affected],
    )?;

    tracing::info!(
        workspace = %normalized,
        move_to_default,
        affected,
        "Deleted workspace"
    );

    Ok(affected)
}

/// Delete a memory (soft delete by setting valid_to)
pub fn delete_memory(conn: &Connection, id: i64) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    // Get memory info before deletion for event data
    let memory_info: Option<(String, String)> = conn
        .query_row(
            "SELECT workspace, memory_type FROM memories WHERE id = ? AND valid_to IS NULL",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

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

    // Record event for sync delta tracking
    let (workspace, memory_type) =
        memory_info.unwrap_or(("default".to_string(), "unknown".to_string()));
    record_event(
        conn,
        MemoryEventType::Deleted,
        Some(id),
        None,
        serde_json::json!({
            "workspace": workspace,
            "memory_type": memory_type,
        }),
    )?;

    // Update sync state (version now tracks event count for delta sync)
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + 1, version = (SELECT MAX(id) FROM memory_events) WHERE id = 1",
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
                m.scope_type, m.scope_id, m.workspace, m.tier, m.expires_at, m.content_hash
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

    // Workspace filter
    if let Some(ref workspace) = options.workspace {
        conditions.push("m.workspace = ?".to_string());
        params.push(Box::new(workspace.clone()));
    }

    // Tier filter
    if let Some(ref tier) = options.tier {
        conditions.push("m.tier = ?".to_string());
        params.push(Box::new(tier.as_str().to_string()));
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

/// Query episodic memories ordered by event_time within a time range.
pub fn get_episodic_timeline(
    conn: &Connection,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
    workspace: Option<&str>,
    tags: Option<&[String]>,
    limit: i64,
) -> Result<Vec<Memory>> {
    let now = Utc::now().to_rfc3339();

    let mut sql = String::from(
        "SELECT DISTINCT m.id, m.content, m.memory_type, m.importance, m.access_count,
                m.created_at, m.updated_at, m.last_accessed_at, m.owner_id,
                m.visibility, m.version, m.has_embedding, m.metadata,
                m.scope_type, m.scope_id, m.workspace, m.tier, m.expires_at, m.content_hash,
                m.event_time, m.event_duration_seconds, m.trigger_pattern,
                m.procedure_success_count, m.procedure_failure_count, m.summary_of_id,
                m.lifecycle_state
         FROM memories m",
    );

    let mut conditions = vec![
        "m.valid_to IS NULL".to_string(),
        "(m.expires_at IS NULL OR m.expires_at > ?)".to_string(),
        "m.memory_type = 'episodic'".to_string(),
        "m.event_time IS NOT NULL".to_string(),
    ];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

    if let Some(start) = start_time {
        conditions.push("m.event_time >= ?".to_string());
        params.push(Box::new(start.to_rfc3339()));
    }

    if let Some(end) = end_time {
        conditions.push("m.event_time <= ?".to_string());
        params.push(Box::new(end.to_rfc3339()));
    }

    if let Some(ws) = workspace {
        conditions.push("m.workspace = ?".to_string());
        params.push(Box::new(ws.to_string()));
    }

    if let Some(tag_list) = tags {
        if !tag_list.is_empty() {
            sql.push_str(
                " JOIN memory_tags mt ON m.id = mt.memory_id
                  JOIN tags t ON mt.tag_id = t.id",
            );
            let placeholders: Vec<String> = tag_list.iter().map(|_| "?".to_string()).collect();
            conditions.push(format!("t.name IN ({})", placeholders.join(", ")));
            for tag in tag_list {
                params.push(Box::new(tag.clone()));
            }
        }
    }

    sql.push_str(" WHERE ");
    sql.push_str(&conditions.join(" AND "));
    sql.push_str(" ORDER BY m.event_time ASC");
    sql.push_str(&format!(" LIMIT {}", limit));

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

/// Query procedural memories, optionally filtered by trigger pattern and success rate.
pub fn get_procedural_memories(
    conn: &Connection,
    trigger_pattern: Option<&str>,
    workspace: Option<&str>,
    min_success_rate: Option<f32>,
    limit: i64,
) -> Result<Vec<Memory>> {
    let now = Utc::now().to_rfc3339();

    let sql_base = "SELECT m.id, m.content, m.memory_type, m.importance, m.access_count,
                m.created_at, m.updated_at, m.last_accessed_at, m.owner_id,
                m.visibility, m.version, m.has_embedding, m.metadata,
                m.scope_type, m.scope_id, m.workspace, m.tier, m.expires_at, m.content_hash,
                m.event_time, m.event_duration_seconds, m.trigger_pattern,
                m.procedure_success_count, m.procedure_failure_count, m.summary_of_id,
                m.lifecycle_state
         FROM memories m";

    let mut conditions = vec![
        "m.valid_to IS NULL".to_string(),
        "(m.expires_at IS NULL OR m.expires_at > ?)".to_string(),
        "m.memory_type = 'procedural'".to_string(),
    ];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

    if let Some(pattern) = trigger_pattern {
        conditions.push("m.trigger_pattern LIKE ?".to_string());
        params.push(Box::new(format!("%{}%", pattern)));
    }

    if let Some(ws) = workspace {
        conditions.push("m.workspace = ?".to_string());
        params.push(Box::new(ws.to_string()));
    }

    if let Some(min_rate) = min_success_rate {
        // Filter: success / (success + failure) >= min_rate
        // Only apply when there's at least one execution
        conditions.push("(m.procedure_success_count + m.procedure_failure_count) > 0".to_string());
        conditions.push(
            "CAST(m.procedure_success_count AS REAL) / (m.procedure_success_count + m.procedure_failure_count) >= ?"
                .to_string(),
        );
        params.push(Box::new(min_rate as f64));
    }

    let sql = format!(
        "{} WHERE {} ORDER BY m.procedure_success_count DESC LIMIT {}",
        sql_base,
        conditions.join(" AND "),
        limit
    );

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

/// Record a success or failure outcome for a procedural memory.
pub fn record_procedure_outcome(
    conn: &Connection,
    memory_id: i64,
    success: bool,
) -> Result<Memory> {
    let column = if success {
        "procedure_success_count"
    } else {
        "procedure_failure_count"
    };

    let now = Utc::now().to_rfc3339();

    // Verify the memory exists and is procedural
    let memory_type: String = conn
        .query_row(
            "SELECT memory_type FROM memories WHERE id = ? AND valid_to IS NULL",
            params![memory_id],
            |row| row.get(0),
        )
        .map_err(|_| EngramError::NotFound(memory_id))?;

    if memory_type != "procedural" {
        return Err(EngramError::InvalidInput(format!(
            "Memory {} is type '{}', not 'procedural'",
            memory_id, memory_type
        )));
    }

    conn.execute(
        &format!(
            "UPDATE memories SET {} = {} + 1, updated_at = ? WHERE id = ?",
            column, column
        ),
        params![now, memory_id],
    )?;

    get_memory(conn, memory_id)
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
            // No change - don't record event or update sync state
            return get_memory_internal(conn, id, false);
        }
    }

    // Record event for sync delta tracking
    record_event(
        conn,
        MemoryEventType::Updated,
        Some(id),
        None,
        serde_json::json!({
            "changed_fields": ["expires_at"],
            "action": "set_expiration",
        }),
    )?;

    // Update sync state (version now tracks event count for delta sync)
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + 1, version = (SELECT MAX(id) FROM memory_events) WHERE id = 1",
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

        // Record batch event for sync delta tracking
        record_event(
            conn,
            MemoryEventType::Deleted,
            None, // Batch operation
            None,
            serde_json::json!({
                "action": "cleanup_expired",
                "affected_count": affected,
            }),
        )?;

        // Update sync state (version now tracks event count for delta sync)
        conn.execute(
            "UPDATE sync_state SET pending_changes = pending_changes + ?, version = (SELECT COALESCE(MAX(id), 0) FROM memory_events) WHERE id = 1",
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

/// A per-workspace retention policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub id: i64,
    pub workspace: String,
    pub max_age_days: Option<i64>,
    pub max_memories: Option<i64>,
    pub compress_after_days: Option<i64>,
    pub compress_max_importance: f32,
    pub compress_min_access: i32,
    pub auto_delete_after_days: Option<i64>,
    pub exclude_types: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Get retention policy for a workspace.
pub fn get_retention_policy(conn: &Connection, workspace: &str) -> Result<Option<RetentionPolicy>> {
    conn.query_row(
        "SELECT id, workspace, max_age_days, max_memories, compress_after_days,
                compress_max_importance, compress_min_access, auto_delete_after_days,
                exclude_types, created_at, updated_at
         FROM retention_policies WHERE workspace = ?",
        params![workspace],
        |row| {
            let exclude_str: Option<String> = row.get(8)?;
            Ok(RetentionPolicy {
                id: row.get(0)?,
                workspace: row.get(1)?,
                max_age_days: row.get(2)?,
                max_memories: row.get(3)?,
                compress_after_days: row.get(4)?,
                compress_max_importance: row.get::<_, f32>(5).unwrap_or(0.3),
                compress_min_access: row.get::<_, i32>(6).unwrap_or(3),
                auto_delete_after_days: row.get(7)?,
                exclude_types: exclude_str
                    .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
                    .unwrap_or_default(),
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        },
    )
    .optional()
    .map_err(EngramError::from)
}

/// List all retention policies.
pub fn list_retention_policies(conn: &Connection) -> Result<Vec<RetentionPolicy>> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace, max_age_days, max_memories, compress_after_days,
                compress_max_importance, compress_min_access, auto_delete_after_days,
                exclude_types, created_at, updated_at
         FROM retention_policies ORDER BY workspace",
    )?;

    let policies = stmt
        .query_map([], |row| {
            let exclude_str: Option<String> = row.get(8)?;
            Ok(RetentionPolicy {
                id: row.get(0)?,
                workspace: row.get(1)?,
                max_age_days: row.get(2)?,
                max_memories: row.get(3)?,
                compress_after_days: row.get(4)?,
                compress_max_importance: row.get::<_, f32>(5).unwrap_or(0.3),
                compress_min_access: row.get::<_, i32>(6).unwrap_or(3),
                auto_delete_after_days: row.get(7)?,
                exclude_types: exclude_str
                    .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
                    .unwrap_or_default(),
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(policies)
}

/// Upsert a retention policy for a workspace.
pub fn set_retention_policy(
    conn: &Connection,
    workspace: &str,
    max_age_days: Option<i64>,
    max_memories: Option<i64>,
    compress_after_days: Option<i64>,
    compress_max_importance: Option<f32>,
    compress_min_access: Option<i32>,
    auto_delete_after_days: Option<i64>,
    exclude_types: Option<Vec<String>>,
) -> Result<RetentionPolicy> {
    let now = Utc::now().to_rfc3339();
    let exclude_str = exclude_types.map(|v| v.join(","));

    conn.execute(
        "INSERT INTO retention_policies (workspace, max_age_days, max_memories, compress_after_days,
            compress_max_importance, compress_min_access, auto_delete_after_days, exclude_types,
            created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
         ON CONFLICT(workspace) DO UPDATE SET
            max_age_days = COALESCE(?2, max_age_days),
            max_memories = COALESCE(?3, max_memories),
            compress_after_days = COALESCE(?4, compress_after_days),
            compress_max_importance = COALESCE(?5, compress_max_importance),
            compress_min_access = COALESCE(?6, compress_min_access),
            auto_delete_after_days = COALESCE(?7, auto_delete_after_days),
            exclude_types = COALESCE(?8, exclude_types),
            updated_at = ?9",
        params![
            workspace,
            max_age_days,
            max_memories,
            compress_after_days,
            compress_max_importance.unwrap_or(0.3),
            compress_min_access.unwrap_or(3),
            auto_delete_after_days,
            exclude_str,
            now,
        ],
    )?;

    get_retention_policy(conn, workspace)?.ok_or_else(|| EngramError::NotFound(0))
}

/// Delete a retention policy for a workspace.
pub fn delete_retention_policy(conn: &Connection, workspace: &str) -> Result<bool> {
    let affected = conn.execute(
        "DELETE FROM retention_policies WHERE workspace = ?",
        params![workspace],
    )?;
    Ok(affected > 0)
}

/// Apply all retention policies. Returns total memories affected across all workspaces.
pub fn apply_retention_policies(conn: &Connection) -> Result<i64> {
    let policies = list_retention_policies(conn)?;
    let mut total_affected = 0i64;

    for policy in &policies {
        // 1. Auto-compress based on compress_after_days
        if let Some(compress_days) = policy.compress_after_days {
            let compressed = compress_old_memories(
                conn,
                compress_days,
                policy.compress_max_importance,
                policy.compress_min_access,
                100,
            )?;
            total_affected += compressed;
        }

        // 2. Enforce max_memories limit per workspace
        if let Some(max_mem) = policy.max_memories {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE workspace = ? AND valid_to IS NULL
                     AND COALESCE(lifecycle_state, 'active') = 'active'",
                    params![policy.workspace],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            if count > max_mem {
                // Archive excess (oldest, lowest importance first)
                let excess = count - max_mem;
                let archived = conn.execute(
                    "UPDATE memories SET lifecycle_state = 'archived'
                     WHERE id IN (
                        SELECT id FROM memories
                        WHERE workspace = ? AND valid_to IS NULL
                          AND COALESCE(lifecycle_state, 'active') = 'active'
                          AND memory_type NOT IN ('summary', 'checkpoint')
                        ORDER BY importance ASC, access_count ASC, created_at ASC
                        LIMIT ?
                     )",
                    params![policy.workspace, excess],
                )?;
                total_affected += archived as i64;
            }
        }

        // 3. Auto-delete very old archived memories
        if let Some(delete_days) = policy.auto_delete_after_days {
            let cutoff = (Utc::now() - chrono::Duration::days(delete_days)).to_rfc3339();
            let now = Utc::now().to_rfc3339();
            let deleted = conn.execute(
                "UPDATE memories SET valid_to = ?
                 WHERE workspace = ? AND valid_to IS NULL
                   AND lifecycle_state = 'archived'
                   AND created_at < ?",
                params![now, policy.workspace, cutoff],
            )?;
            total_affected += deleted as i64;
        }
    }

    Ok(total_affected)
}

/// Auto-compress old, rarely-accessed memories by creating summaries and archiving originals.
/// Returns the number of memories archived.
pub fn compress_old_memories(
    conn: &Connection,
    max_age_days: i64,
    max_importance: f32,
    min_access_count: i32,
    batch_limit: usize,
) -> Result<i64> {
    let cutoff = (Utc::now() - chrono::Duration::days(max_age_days)).to_rfc3339();
    let now = Utc::now().to_rfc3339();

    // Find candidates: old, low-importance, rarely-accessed, not already archived/summary
    let mut stmt = conn.prepare(
        "SELECT id, content, memory_type, importance, tags, workspace
         FROM (
            SELECT m.id, m.content, m.memory_type, m.importance, m.access_count, m.workspace,
                   COALESCE(m.lifecycle_state, 'active') as lifecycle_state,
                   (SELECT GROUP_CONCAT(t.name, ',') FROM memory_tags mt JOIN tags t ON mt.tag_id = t.id WHERE mt.memory_id = m.id) as tags
            FROM memories m
            WHERE m.valid_to IS NULL
              AND (m.expires_at IS NULL OR m.expires_at > ?1)
              AND m.created_at < ?2
              AND m.importance <= ?3
              AND m.access_count < ?4
              AND m.memory_type NOT IN ('summary', 'checkpoint')
              AND COALESCE(m.lifecycle_state, 'active') = 'active'
            ORDER BY m.created_at ASC
            LIMIT ?5
         )",
    )?;

    let candidates: Vec<(i64, String, String, f32, Option<String>, String)> = stmt
        .query_map(
            params![
                now,
                cutoff,
                max_importance,
                min_access_count,
                batch_limit as i64
            ],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get::<_, String>(5)
                        .unwrap_or_else(|_| "default".to_string()),
                ))
            },
        )?
        .filter_map(|r| r.ok())
        .collect();

    let mut archived = 0i64;

    for (id, content, memory_type, importance, tags_csv, workspace) in &candidates {
        // Create compressed summary
        let summary_text = if content.len() > 200 {
            let head: String = content.chars().take(120).collect();
            let tail: String = content
                .chars()
                .rev()
                .take(60)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            format!("{}...{}", head, tail)
        } else {
            content.clone()
        };

        let tags: Vec<String> = tags_csv
            .as_deref()
            .unwrap_or("")
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        let input = CreateMemoryInput {
            content: format!("[Archived {}] {}", memory_type, summary_text),
            memory_type: MemoryType::Summary,
            importance: Some(*importance),
            tags,
            workspace: Some(workspace.clone()),
            tier: MemoryTier::Permanent,
            summary_of_id: Some(*id),
            ..Default::default()
        };

        if create_memory(conn, &input).is_ok()
            && conn
                .execute(
                    "UPDATE memories SET lifecycle_state = 'archived' WHERE id = ? AND valid_to IS NULL",
                    params![id],
                )
                .is_ok()
        {
            archived += 1;
        }
    }

    Ok(archived)
}

/// A compact memory representation for efficient list views.
/// Contains only essential fields and a truncated content preview.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompactMemoryRow {
    /// Memory ID
    pub id: i64,
    /// Content preview (first line or N chars)
    pub preview: String,
    /// Whether content was truncated
    pub truncated: bool,
    /// Memory type
    pub memory_type: MemoryType,
    /// Tags
    pub tags: Vec<String>,
    /// Importance score
    pub importance: f32,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
    /// Workspace name
    pub workspace: String,
    /// Memory tier
    pub tier: MemoryTier,
    /// Original content length in chars
    pub content_length: usize,
    /// Number of lines in original content
    pub line_count: usize,
}

/// List memories in compact format with preview only.
///
/// This is more efficient than `list_memories` when you don't need full content,
/// such as for browsing/listing UIs.
///
/// # Arguments
/// * `conn` - Database connection
/// * `options` - List filtering/pagination options
/// * `preview_chars` - Max chars for preview (default: 100)
pub fn list_memories_compact(
    conn: &Connection,
    options: &ListOptions,
    preview_chars: Option<usize>,
) -> Result<Vec<CompactMemoryRow>> {
    use crate::intelligence::compact_preview;

    let now = Utc::now().to_rfc3339();
    let max_preview = preview_chars.unwrap_or(100);

    let mut sql = String::from(
        "SELECT DISTINCT m.id, m.content, m.memory_type, m.importance,
                m.created_at, m.updated_at, m.workspace, m.tier
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

    // Workspace filter
    if let Some(ref workspace) = options.workspace {
        conditions.push("m.workspace = ?".to_string());
        params.push(Box::new(workspace.clone()));
    }

    // Tier filter
    if let Some(ref tier) = options.tier {
        conditions.push("m.tier = ?".to_string());
        params.push(Box::new(tier.as_str().to_string()));
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

    let memories: Vec<CompactMemoryRow> = stmt
        .query_map(param_refs.as_slice(), |row| {
            let id: i64 = row.get("id")?;
            let content: String = row.get("content")?;
            let memory_type_str: String = row.get("memory_type")?;
            let importance: f32 = row.get("importance")?;
            let created_at_str: String = row.get("created_at")?;
            let updated_at_str: String = row.get("updated_at")?;
            let workspace: String = row.get("workspace")?;
            let tier_str: String = row.get("tier")?;

            let memory_type = memory_type_str.parse().unwrap_or(MemoryType::Note);
            let tier = tier_str.parse().unwrap_or_default();

            // Generate compact preview
            let (preview, truncated) = compact_preview(&content, max_preview);
            let content_length = content.len();
            let line_count = content.lines().count();

            Ok(CompactMemoryRow {
                id,
                preview,
                truncated,
                memory_type,
                tags: vec![], // Will be loaded separately
                importance,
                created_at: DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                workspace,
                tier,
                content_length,
                line_count,
            })
        })?
        .filter_map(|r| r.ok())
        .map(|mut m| {
            m.tags = load_tags(conn, m.id).unwrap_or_default();
            m
        })
        .collect();

    Ok(memories)
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

    let _total_identities: i64 =
        conn.query_row("SELECT COUNT(*) FROM identities", [], |row| row.get(0))?;

    let _total_entities: i64 =
        conn.query_row("SELECT COUNT(*) FROM entities", [], |row| row.get(0))?;

    let db_size_bytes: i64 = conn.query_row(
        "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
        [],
        |row| row.get(0),
    )?;

    let _schema_version: i32 = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .unwrap_or(0);

    let mut workspace_stmt = conn.prepare(
        "SELECT workspace, COUNT(*) FROM memories WHERE valid_to IS NULL GROUP BY workspace",
    )?;
    let workspaces: HashMap<String, i64> = workspace_stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut type_stmt = conn.prepare(
        "SELECT memory_type, COUNT(*) FROM memories WHERE valid_to IS NULL GROUP BY memory_type",
    )?;
    let type_counts: HashMap<String, i64> = type_stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut tier_stmt = conn.prepare(
        "SELECT COALESCE(tier, 'permanent'), COUNT(*) FROM memories GROUP BY COALESCE(tier, 'permanent')",
    )?;
    let tier_counts: HashMap<String, i64> = tier_stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

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
        total_identities: 0,
        total_entities: 0,
        db_size_bytes,
        memories_with_embeddings,
        memories_pending_embedding,
        last_sync: last_sync.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        }),
        sync_pending: sync_pending > 0,
        storage_mode: "sqlite".to_string(),
        schema_version: 0,
        workspaces,
        type_counts,
        tier_counts,
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

// ============================================================================
// Batch Operations
// ============================================================================

/// Result of a batch create operation
#[derive(Debug, Clone, serde::Serialize)]
pub struct BatchCreateResult {
    pub created: Vec<Memory>,
    pub failed: Vec<BatchError>,
    pub total_created: usize,
    pub total_failed: usize,
}

/// Result of a batch delete operation
#[derive(Debug, Clone, serde::Serialize)]
pub struct BatchDeleteResult {
    pub deleted: Vec<i64>,
    pub failed: Vec<BatchError>,
    pub total_deleted: usize,
    pub total_failed: usize,
}

/// Error information for batch operations
#[derive(Debug, Clone, serde::Serialize)]
pub struct BatchError {
    pub index: usize,
    pub id: Option<i64>,
    pub error: String,
}

/// Create multiple memories in a single transaction
pub fn create_memory_batch(
    conn: &Connection,
    inputs: &[CreateMemoryInput],
) -> Result<BatchCreateResult> {
    let mut created = Vec::new();
    let mut failed = Vec::new();

    for (index, input) in inputs.iter().enumerate() {
        match create_memory(conn, input) {
            Ok(memory) => created.push(memory),
            Err(e) => failed.push(BatchError {
                index,
                id: None,
                error: e.to_string(),
            }),
        }
    }

    Ok(BatchCreateResult {
        total_created: created.len(),
        total_failed: failed.len(),
        created,
        failed,
    })
}

/// Delete multiple memories in a single transaction
pub fn delete_memory_batch(conn: &Connection, ids: &[i64]) -> Result<BatchDeleteResult> {
    let mut deleted = Vec::new();
    let mut failed = Vec::new();

    for (index, &id) in ids.iter().enumerate() {
        match delete_memory(conn, id) {
            Ok(()) => deleted.push(id),
            Err(e) => failed.push(BatchError {
                index,
                id: Some(id),
                error: e.to_string(),
            }),
        }
    }

    Ok(BatchDeleteResult {
        total_deleted: deleted.len(),
        total_failed: failed.len(),
        deleted,
        failed,
    })
}

// ============================================================================
// Tag Utilities
// ============================================================================

/// Tag with usage count
#[derive(Debug, Clone, serde::Serialize)]
pub struct TagInfo {
    pub name: String,
    pub count: i64,
    pub last_used: Option<DateTime<Utc>>,
}

/// Get all tags with their usage counts
pub fn list_tags(conn: &Connection) -> Result<Vec<TagInfo>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT t.name, COUNT(mt.memory_id) as count,
               MAX(m.updated_at) as last_used
        FROM tags t
        LEFT JOIN memory_tags mt ON t.id = mt.tag_id
        LEFT JOIN memories m ON mt.memory_id = m.id AND m.valid_to IS NULL
        GROUP BY t.id, t.name
        ORDER BY count DESC, t.name ASC
        "#,
    )?;

    let tags: Vec<TagInfo> = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            let last_used: Option<String> = row.get(2)?;

            Ok(TagInfo {
                name,
                count,
                last_used: last_used.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(tags)
}

/// Tag hierarchy node
#[derive(Debug, Clone, serde::Serialize)]
pub struct TagHierarchyNode {
    pub name: String,
    pub full_path: String,
    pub count: i64,
    pub children: Vec<TagHierarchyNode>,
}

/// Build tag hierarchy from slash-separated tags (e.g., "project/engram/core")
pub fn get_tag_hierarchy(conn: &Connection) -> Result<Vec<TagHierarchyNode>> {
    let tags = list_tags(conn)?;

    // Build hierarchy from slash-separated paths
    let mut root_nodes: HashMap<String, TagHierarchyNode> = HashMap::new();

    for tag in tags {
        let parts: Vec<&str> = tag.name.split('/').collect();
        if parts.is_empty() {
            continue;
        }

        let root_name = parts[0].to_string();
        if !root_nodes.contains_key(&root_name) {
            root_nodes.insert(
                root_name.clone(),
                TagHierarchyNode {
                    name: root_name.clone(),
                    full_path: root_name.clone(),
                    count: 0,
                    children: Vec::new(),
                },
            );
        }

        // Add count to appropriate level
        if parts.len() == 1 {
            if let Some(node) = root_nodes.get_mut(&root_name) {
                node.count += tag.count;
            }
        } else {
            // For nested tags, we'd need recursive building
            // For now, just add to root count
            if let Some(node) = root_nodes.get_mut(&root_name) {
                node.count += tag.count;
            }
        }
    }

    Ok(root_nodes.into_values().collect())
}

/// Tag validation result
#[derive(Debug, Clone, serde::Serialize)]
pub struct TagValidationResult {
    pub valid: bool,
    pub orphaned_tags: Vec<String>,
    pub empty_tags: Vec<String>,
    pub duplicate_assignments: Vec<(i64, String)>,
    pub total_tags: i64,
    pub total_assignments: i64,
}

/// Validate tag consistency
pub fn validate_tags(conn: &Connection) -> Result<TagValidationResult> {
    // Find orphaned tags (tags with no memories)
    let orphaned: Vec<String> = conn
        .prepare(
            "SELECT t.name FROM tags t
             LEFT JOIN memory_tags mt ON t.id = mt.tag_id
             WHERE mt.tag_id IS NULL",
        )?
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    // Find empty tag names
    let empty: Vec<String> = conn
        .prepare("SELECT name FROM tags WHERE name = '' OR name IS NULL")?
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    // Count totals
    let total_tags: i64 = conn.query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))?;
    let total_assignments: i64 =
        conn.query_row("SELECT COUNT(*) FROM memory_tags", [], |row| row.get(0))?;

    Ok(TagValidationResult {
        valid: orphaned.is_empty() && empty.is_empty(),
        orphaned_tags: orphaned,
        empty_tags: empty,
        duplicate_assignments: vec![], // Would need more complex query
        total_tags,
        total_assignments,
    })
}

// ============================================================================
// Import/Export
// ============================================================================

/// Exported memory format
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportedMemory {
    pub id: i64,
    pub content: String,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub importance: f32,
    pub workspace: String,
    pub tier: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Export format
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExportData {
    pub version: String,
    pub exported_at: String,
    pub memory_count: usize,
    pub memories: Vec<ExportedMemory>,
}

/// Export all memories to JSON-serializable format
pub fn export_memories(conn: &Connection) -> Result<ExportData> {
    let memories = list_memories(
        conn,
        &ListOptions {
            limit: Some(100000),
            ..Default::default()
        },
    )?;

    let exported: Vec<ExportedMemory> = memories
        .into_iter()
        .map(|m| ExportedMemory {
            id: m.id,
            content: m.content,
            memory_type: m.memory_type.as_str().to_string(),
            tags: m.tags,
            metadata: m.metadata,
            importance: m.importance,
            workspace: m.workspace,
            tier: m.tier.as_str().to_string(),
            created_at: m.created_at.to_rfc3339(),
            updated_at: m.updated_at.to_rfc3339(),
        })
        .collect();

    Ok(ExportData {
        version: "1.0".to_string(),
        exported_at: Utc::now().to_rfc3339(),
        memory_count: exported.len(),
        memories: exported,
    })
}

/// Import result
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

/// Import memories from exported format
pub fn import_memories(
    conn: &Connection,
    data: &ExportData,
    skip_duplicates: bool,
) -> Result<ImportResult> {
    let mut imported = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut errors = Vec::new();

    for mem in &data.memories {
        let memory_type = mem.memory_type.parse().unwrap_or(MemoryType::Note);
        let tier = mem.tier.parse().unwrap_or(MemoryTier::Permanent);

        let input = CreateMemoryInput {
            content: mem.content.clone(),
            memory_type,
            tags: mem.tags.clone(),
            metadata: mem.metadata.clone(),
            importance: Some(mem.importance),
            scope: MemoryScope::Global,
            workspace: Some(mem.workspace.clone()),
            tier,
            defer_embedding: false,
            ttl_seconds: None,
            dedup_mode: if skip_duplicates {
                DedupMode::Skip
            } else {
                DedupMode::Allow
            },
            dedup_threshold: None,
            event_time: None,
            event_duration_seconds: None,
            trigger_pattern: None,
            summary_of_id: None,
        };

        match create_memory(conn, &input) {
            Ok(_) => imported += 1,
            Err(EngramError::Duplicate { .. }) => skipped += 1,
            Err(e) => {
                failed += 1;
                errors.push(format!("Failed to import memory {}: {}", mem.id, e));
            }
        }
    }

    Ok(ImportResult {
        imported,
        skipped,
        failed,
        errors,
    })
}

// ============================================================================
// Maintenance Operations
// ============================================================================

/// Queue all memories for re-embedding
pub fn rebuild_embeddings(conn: &Connection) -> Result<i64> {
    let now = Utc::now().to_rfc3339();

    // Clear existing queue
    conn.execute("DELETE FROM embedding_queue", [])?;

    // Queue all memories
    let count = conn.execute(
        "INSERT INTO embedding_queue (memory_id, status, queued_at)
         SELECT id, 'pending', ? FROM memories WHERE valid_to IS NULL",
        params![now],
    )?;

    // Reset has_embedding flag
    conn.execute(
        "UPDATE memories SET has_embedding = 0 WHERE valid_to IS NULL",
        [],
    )?;

    Ok(count as i64)
}

/// Rebuild all cross-references based on embeddings
pub fn rebuild_crossrefs(conn: &Connection) -> Result<i64> {
    let now = Utc::now().to_rfc3339();

    // Clear existing auto-generated crossrefs (keep manual ones)
    let deleted = conn.execute(
        "UPDATE crossrefs SET valid_to = ? WHERE source = 'auto' AND valid_to IS NULL",
        params![now],
    )?;

    // Note: Actual crossref generation requires embeddings and is done by the embedding worker
    // This just clears the old ones so they can be regenerated

    Ok(deleted as i64)
}

// ============================================================================
// Special Memory Types
// ============================================================================

/// Create a section memory (for document structure)
pub fn create_section_memory(
    conn: &Connection,
    title: &str,
    content: &str,
    parent_id: Option<i64>,
    level: i32,
    workspace: Option<&str>,
) -> Result<Memory> {
    let mut metadata = HashMap::new();
    metadata.insert("section_title".to_string(), serde_json::json!(title));
    metadata.insert("section_level".to_string(), serde_json::json!(level));
    if let Some(pid) = parent_id {
        metadata.insert("parent_memory_id".to_string(), serde_json::json!(pid));
    }

    let input = CreateMemoryInput {
        content: format!("# {}\n\n{}", title, content),
        memory_type: MemoryType::Context,
        tags: vec!["section".to_string()],
        metadata,
        importance: Some(0.6),
        scope: MemoryScope::Global,
        workspace: workspace.map(String::from),
        tier: MemoryTier::Permanent,
        defer_embedding: false,
        ttl_seconds: None,
        dedup_mode: DedupMode::Skip,
        dedup_threshold: None,
        event_time: None,
        event_duration_seconds: None,
        trigger_pattern: None,
        summary_of_id: None,
    };

    create_memory(conn, &input)
}

/// Create a checkpoint memory for session state
pub fn create_checkpoint(
    conn: &Connection,
    session_id: &str,
    summary: &str,
    context: &HashMap<String, serde_json::Value>,
    workspace: Option<&str>,
) -> Result<Memory> {
    let mut metadata = context.clone();
    metadata.insert(
        "checkpoint_session".to_string(),
        serde_json::json!(session_id),
    );
    metadata.insert(
        "checkpoint_time".to_string(),
        serde_json::json!(Utc::now().to_rfc3339()),
    );

    let input = CreateMemoryInput {
        content: format!("Session Checkpoint: {}\n\n{}", session_id, summary),
        memory_type: MemoryType::Context,
        tags: vec!["checkpoint".to_string(), format!("session:{}", session_id)],
        metadata,
        importance: Some(0.7),
        scope: MemoryScope::Global,
        workspace: workspace.map(String::from),
        tier: MemoryTier::Permanent,
        defer_embedding: false,
        ttl_seconds: None,
        dedup_mode: DedupMode::Allow,
        dedup_threshold: None,
        event_time: None,
        event_duration_seconds: None,
        trigger_pattern: None,
        summary_of_id: None,
    };

    create_memory(conn, &input)
}

/// Temporarily boost a memory's importance
pub fn boost_memory(
    conn: &Connection,
    id: i64,
    boost_amount: f32,
    duration_seconds: Option<i64>,
) -> Result<Memory> {
    let memory = get_memory(conn, id)?;
    let new_importance = (memory.importance + boost_amount).min(1.0);
    let now = Utc::now();

    // Update importance
    conn.execute(
        "UPDATE memories SET importance = ?, updated_at = ? WHERE id = ?",
        params![new_importance, now.to_rfc3339(), id],
    )?;

    // If duration specified, store boost info in metadata for later decay
    if let Some(duration) = duration_seconds {
        let expires = now + chrono::Duration::seconds(duration);
        let mut metadata = memory.metadata.clone();
        metadata.insert(
            "boost_expires".to_string(),
            serde_json::json!(expires.to_rfc3339()),
        );
        metadata.insert(
            "boost_original_importance".to_string(),
            serde_json::json!(memory.importance),
        );

        let metadata_json = serde_json::to_string(&metadata)?;
        conn.execute(
            "UPDATE memories SET metadata = ? WHERE id = ?",
            params![metadata_json, id],
        )?;
    }

    get_memory(conn, id)
}

// =============================================================================
// Event System
// =============================================================================

/// Event types for the memory system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryEventType {
    Created,
    Updated,
    Deleted,
    Linked,
    Unlinked,
    Shared,
    Synced,
}

impl std::fmt::Display for MemoryEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryEventType::Created => write!(f, "created"),
            MemoryEventType::Updated => write!(f, "updated"),
            MemoryEventType::Deleted => write!(f, "deleted"),
            MemoryEventType::Linked => write!(f, "linked"),
            MemoryEventType::Unlinked => write!(f, "unlinked"),
            MemoryEventType::Shared => write!(f, "shared"),
            MemoryEventType::Synced => write!(f, "synced"),
        }
    }
}

impl std::str::FromStr for MemoryEventType {
    type Err = EngramError;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "created" => Ok(MemoryEventType::Created),
            "updated" => Ok(MemoryEventType::Updated),
            "deleted" => Ok(MemoryEventType::Deleted),
            "linked" => Ok(MemoryEventType::Linked),
            "unlinked" => Ok(MemoryEventType::Unlinked),
            "shared" => Ok(MemoryEventType::Shared),
            "synced" => Ok(MemoryEventType::Synced),
            _ => Err(EngramError::InvalidInput(format!(
                "Invalid event type: {}",
                s
            ))),
        }
    }
}

/// A memory event for tracking changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub id: i64,
    pub event_type: String,
    pub memory_id: Option<i64>,
    pub agent_id: Option<String>,
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Record an event in the event system
pub fn record_event(
    conn: &Connection,
    event_type: MemoryEventType,
    memory_id: Option<i64>,
    agent_id: Option<&str>,
    data: serde_json::Value,
) -> Result<i64> {
    let now = Utc::now();
    let data_json = serde_json::to_string(&data)?;

    conn.execute(
        "INSERT INTO memory_events (event_type, memory_id, agent_id, data, created_at)
         VALUES (?, ?, ?, ?, ?)",
        params![
            event_type.to_string(),
            memory_id,
            agent_id,
            data_json,
            now.to_rfc3339()
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

/// Poll for events since a given timestamp or event ID
pub fn poll_events(
    conn: &Connection,
    since_id: Option<i64>,
    since_time: Option<DateTime<Utc>>,
    agent_id: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<MemoryEvent>> {
    let limit = limit.unwrap_or(100);

    let (query, params): (&str, Vec<Box<dyn rusqlite::ToSql>>) =
        match (since_id, since_time, agent_id) {
            (Some(id), _, Some(agent)) => (
                "SELECT id, event_type, memory_id, agent_id, data, created_at
             FROM memory_events WHERE id > ? AND (agent_id = ? OR agent_id IS NULL)
             ORDER BY id ASC LIMIT ?",
                vec![
                    Box::new(id),
                    Box::new(agent.to_string()),
                    Box::new(limit as i64),
                ],
            ),
            (Some(id), _, None) => (
                "SELECT id, event_type, memory_id, agent_id, data, created_at
             FROM memory_events WHERE id > ?
             ORDER BY id ASC LIMIT ?",
                vec![Box::new(id), Box::new(limit as i64)],
            ),
            (None, Some(time), Some(agent)) => (
                "SELECT id, event_type, memory_id, agent_id, data, created_at
             FROM memory_events WHERE created_at > ? AND (agent_id = ? OR agent_id IS NULL)
             ORDER BY id ASC LIMIT ?",
                vec![
                    Box::new(time.to_rfc3339()),
                    Box::new(agent.to_string()),
                    Box::new(limit as i64),
                ],
            ),
            (None, Some(time), None) => (
                "SELECT id, event_type, memory_id, agent_id, data, created_at
             FROM memory_events WHERE created_at > ?
             ORDER BY id ASC LIMIT ?",
                vec![Box::new(time.to_rfc3339()), Box::new(limit as i64)],
            ),
            (None, None, Some(agent)) => (
                "SELECT id, event_type, memory_id, agent_id, data, created_at
             FROM memory_events WHERE agent_id = ? OR agent_id IS NULL
             ORDER BY id DESC LIMIT ?",
                vec![Box::new(agent.to_string()), Box::new(limit as i64)],
            ),
            (None, None, None) => (
                "SELECT id, event_type, memory_id, agent_id, data, created_at
             FROM memory_events ORDER BY id DESC LIMIT ?",
                vec![Box::new(limit as i64)],
            ),
        };

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(query)?;
    let events = stmt
        .query_map(params_refs.as_slice(), |row| {
            let data_str: String = row.get(4)?;
            let created_str: String = row.get(5)?;
            Ok(MemoryEvent {
                id: row.get(0)?,
                event_type: row.get(1)?,
                memory_id: row.get(2)?,
                agent_id: row.get(3)?,
                data: serde_json::from_str(&data_str).unwrap_or(serde_json::json!({})),
                created_at: DateTime::parse_from_rfc3339(&created_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(events)
}

/// Clear old events (cleanup)
pub fn clear_events(
    conn: &Connection,
    before_id: Option<i64>,
    before_time: Option<DateTime<Utc>>,
    keep_recent: Option<usize>,
) -> Result<i64> {
    let deleted = if let Some(id) = before_id {
        conn.execute("DELETE FROM memory_events WHERE id < ?", params![id])?
    } else if let Some(time) = before_time {
        conn.execute(
            "DELETE FROM memory_events WHERE created_at < ?",
            params![time.to_rfc3339()],
        )?
    } else if let Some(keep) = keep_recent {
        // Keep only the most recent N events
        conn.execute(
            "DELETE FROM memory_events WHERE id NOT IN (
                SELECT id FROM memory_events ORDER BY id DESC LIMIT ?
            )",
            params![keep as i64],
        )?
    } else {
        // Clear all events
        conn.execute("DELETE FROM memory_events", [])?
    };

    Ok(deleted as i64)
}

// =============================================================================
// Advanced Sync
// =============================================================================

/// Sync version info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncVersion {
    pub version: i64,
    pub last_modified: DateTime<Utc>,
    pub memory_count: i64,
    pub checksum: String,
}

/// Sync task status record (Phase 3 - Langfuse integration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTask {
    pub task_id: String,
    pub task_type: String,
    pub status: String,
    pub progress_percent: i32,
    pub traces_processed: i64,
    pub memories_created: i64,
    pub error_message: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

/// Get the current sync version
pub fn get_sync_version(conn: &Connection) -> Result<SyncVersion> {
    let memory_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;

    let last_modified: Option<String> = conn
        .query_row("SELECT MAX(updated_at) FROM memories", [], |row| row.get(0))
        .ok();

    let version: i64 = conn
        .query_row("SELECT MAX(version) FROM sync_state", [], |row| row.get(0))
        .unwrap_or(0);

    // Simple checksum based on count and last modified
    let checksum = format!(
        "{}-{}-{}",
        memory_count,
        version,
        last_modified.as_deref().unwrap_or("none")
    );

    Ok(SyncVersion {
        version,
        last_modified: last_modified
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now),
        memory_count,
        checksum,
    })
}

/// Insert or update a sync task record
pub fn upsert_sync_task(conn: &Connection, task: &SyncTask) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO sync_tasks (
            task_id, task_type, status, progress_percent, traces_processed, memories_created,
            error_message, started_at, completed_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            task_type = excluded.task_type,
            status = excluded.status,
            progress_percent = excluded.progress_percent,
            traces_processed = excluded.traces_processed,
            memories_created = excluded.memories_created,
            error_message = excluded.error_message,
            started_at = excluded.started_at,
            completed_at = excluded.completed_at
        "#,
        params![
            task.task_id,
            task.task_type,
            task.status,
            task.progress_percent,
            task.traces_processed,
            task.memories_created,
            task.error_message,
            task.started_at,
            task.completed_at
        ],
    )?;

    Ok(())
}

/// Get a sync task by ID
pub fn get_sync_task(conn: &Connection, task_id: &str) -> Result<Option<SyncTask>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT task_id, task_type, status, progress_percent, traces_processed, memories_created,
               error_message, started_at, completed_at
        FROM sync_tasks
        WHERE task_id = ?
        "#,
    )?;

    let mut rows = stmt.query(params![task_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(SyncTask {
            task_id: row.get("task_id")?,
            task_type: row.get("task_type")?,
            status: row.get("status")?,
            progress_percent: row.get("progress_percent")?,
            traces_processed: row.get("traces_processed")?,
            memories_created: row.get("memories_created")?,
            error_message: row.get("error_message")?,
            started_at: row.get("started_at")?,
            completed_at: row.get("completed_at")?,
        }))
    } else {
        Ok(None)
    }
}

/// Delta entry for sync
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDelta {
    pub created: Vec<Memory>,
    pub updated: Vec<Memory>,
    pub deleted: Vec<i64>,
    pub from_version: i64,
    pub to_version: i64,
}

/// Get changes since a specific version
pub fn get_sync_delta(conn: &Connection, since_version: i64) -> Result<SyncDelta> {
    let current_version = get_sync_version(conn)?.version;

    // Get events since that version to determine what changed
    let events = poll_events(conn, Some(since_version), None, None, Some(10000))?;

    let mut created_ids = std::collections::HashSet::new();
    let mut updated_ids = std::collections::HashSet::new();
    let mut deleted_ids = std::collections::HashSet::new();

    for event in events {
        if let Some(memory_id) = event.memory_id {
            match event.event_type.as_str() {
                "created" => {
                    created_ids.insert(memory_id);
                }
                "updated" => {
                    if !created_ids.contains(&memory_id) {
                        updated_ids.insert(memory_id);
                    }
                }
                "deleted" => {
                    created_ids.remove(&memory_id);
                    updated_ids.remove(&memory_id);
                    deleted_ids.insert(memory_id);
                }
                _ => {}
            }
        }
    }

    let created: Vec<Memory> = created_ids
        .iter()
        .filter_map(|id| get_memory(conn, *id).ok())
        .collect();

    let updated: Vec<Memory> = updated_ids
        .iter()
        .filter_map(|id| get_memory(conn, *id).ok())
        .collect();

    Ok(SyncDelta {
        created,
        updated,
        deleted: deleted_ids.into_iter().collect(),
        from_version: since_version,
        to_version: current_version,
    })
}

/// Agent sync state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSyncState {
    pub agent_id: String,
    pub last_sync_version: i64,
    pub last_sync_time: DateTime<Utc>,
    pub pending_changes: i64,
}

/// Get sync state for a specific agent
pub fn get_agent_sync_state(conn: &Connection, agent_id: &str) -> Result<AgentSyncState> {
    let result: std::result::Result<(i64, String), rusqlite::Error> = conn.query_row(
        "SELECT last_sync_version, last_sync_time FROM agent_sync_state WHERE agent_id = ?",
        params![agent_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );

    match result {
        Ok((version, time_str)) => {
            let current_version = get_sync_version(conn)?.version;
            let pending = (current_version - version).max(0);

            Ok(AgentSyncState {
                agent_id: agent_id.to_string(),
                last_sync_version: version,
                last_sync_time: DateTime::parse_from_rfc3339(&time_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                pending_changes: pending,
            })
        }
        Err(_) => {
            // No sync state yet for this agent
            Ok(AgentSyncState {
                agent_id: agent_id.to_string(),
                last_sync_version: 0,
                last_sync_time: Utc::now(),
                pending_changes: get_sync_version(conn)?.version,
            })
        }
    }
}

/// Update sync state for an agent
pub fn update_agent_sync_state(conn: &Connection, agent_id: &str, version: i64) -> Result<()> {
    let now = Utc::now();
    conn.execute(
        "INSERT INTO agent_sync_state (agent_id, last_sync_version, last_sync_time)
         VALUES (?, ?, ?)
         ON CONFLICT(agent_id) DO UPDATE SET
            last_sync_version = excluded.last_sync_version,
            last_sync_time = excluded.last_sync_time",
        params![agent_id, version, now.to_rfc3339()],
    )?;
    Ok(())
}

/// Cleanup old sync data
pub fn cleanup_sync_data(conn: &Connection, older_than_days: i64) -> Result<i64> {
    let cutoff = Utc::now() - chrono::Duration::days(older_than_days);
    let deleted = conn.execute(
        "DELETE FROM memory_events WHERE created_at < ?",
        params![cutoff.to_rfc3339()],
    )?;
    Ok(deleted as i64)
}

// =============================================================================
// Multi-Agent Sharing
// =============================================================================

/// A shared memory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedMemory {
    pub id: i64,
    pub memory_id: i64,
    pub from_agent: String,
    pub to_agent: String,
    pub message: Option<String>,
    pub acknowledged: bool,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Share a memory with another agent
pub fn share_memory(
    conn: &Connection,
    memory_id: i64,
    from_agent: &str,
    to_agent: &str,
    message: Option<&str>,
) -> Result<i64> {
    let now = Utc::now();

    // Verify memory exists
    let _ = get_memory(conn, memory_id)?;

    conn.execute(
        "INSERT INTO shared_memories (memory_id, from_agent, to_agent, message, acknowledged, created_at)
         VALUES (?, ?, ?, ?, 0, ?)",
        params![memory_id, from_agent, to_agent, message, now.to_rfc3339()],
    )?;

    let share_id = conn.last_insert_rowid();

    // Record event
    record_event(
        conn,
        MemoryEventType::Shared,
        Some(memory_id),
        Some(from_agent),
        serde_json::json!({
            "to_agent": to_agent,
            "share_id": share_id,
            "message": message
        }),
    )?;

    Ok(share_id)
}

/// Poll for shared memories sent to this agent
pub fn poll_shared_memories(
    conn: &Connection,
    to_agent: &str,
    include_acknowledged: bool,
) -> Result<Vec<SharedMemory>> {
    let query = if include_acknowledged {
        "SELECT id, memory_id, from_agent, to_agent, message, acknowledged, acknowledged_at, created_at
         FROM shared_memories WHERE to_agent = ?
         ORDER BY created_at DESC"
    } else {
        "SELECT id, memory_id, from_agent, to_agent, message, acknowledged, acknowledged_at, created_at
         FROM shared_memories WHERE to_agent = ? AND acknowledged = 0
         ORDER BY created_at DESC"
    };

    let mut stmt = conn.prepare(query)?;
    let shares = stmt
        .query_map(params![to_agent], |row| {
            let created_str: String = row.get(7)?;
            let ack_str: Option<String> = row.get(6)?;
            Ok(SharedMemory {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                from_agent: row.get(2)?,
                to_agent: row.get(3)?,
                message: row.get(4)?,
                acknowledged: row.get(5)?,
                acknowledged_at: ack_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                }),
                created_at: DateTime::parse_from_rfc3339(&created_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(shares)
}

/// Acknowledge a shared memory
pub fn acknowledge_share(conn: &Connection, share_id: i64, agent_id: &str) -> Result<()> {
    let now = Utc::now();

    let affected = conn.execute(
        "UPDATE shared_memories SET acknowledged = 1, acknowledged_at = ?
         WHERE id = ? AND to_agent = ?",
        params![now.to_rfc3339(), share_id, agent_id],
    )?;

    if affected == 0 {
        return Err(EngramError::NotFound(share_id));
    }

    Ok(())
}

// =============================================================================
// Search Variants
// =============================================================================

/// Search memories by identity (canonical ID or alias)
pub fn search_by_identity(
    conn: &Connection,
    identity: &str,
    workspace: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<Memory>> {
    let limit = limit.unwrap_or(50);
    let now = Utc::now().to_rfc3339();

    // Search in content and tags for the identity
    // Tags are in a junction table, so we need to use a subquery or JOIN
    let pattern = format!("%{}%", identity);

    let query = if workspace.is_some() {
        "SELECT DISTINCT m.id, m.content, m.memory_type, m.importance, m.access_count,
                m.created_at, m.updated_at, m.last_accessed_at, m.owner_id,
                m.visibility, m.version, m.has_embedding, m.metadata,
                m.scope_type, m.scope_id, m.workspace, m.tier, m.expires_at, m.content_hash
         FROM memories m
         LEFT JOIN memory_tags mt ON m.id = mt.memory_id
         LEFT JOIN tags t ON mt.tag_id = t.id
         WHERE m.workspace = ? AND (m.content LIKE ? OR t.name LIKE ?)
           AND m.valid_to IS NULL
           AND (m.expires_at IS NULL OR m.expires_at > ?)
         ORDER BY m.importance DESC, m.created_at DESC
         LIMIT ?"
    } else {
        "SELECT DISTINCT m.id, m.content, m.memory_type, m.importance, m.access_count,
                m.created_at, m.updated_at, m.last_accessed_at, m.owner_id,
                m.visibility, m.version, m.has_embedding, m.metadata,
                m.scope_type, m.scope_id, m.workspace, m.tier, m.expires_at, m.content_hash
         FROM memories m
         LEFT JOIN memory_tags mt ON m.id = mt.memory_id
         LEFT JOIN tags t ON mt.tag_id = t.id
         WHERE (m.content LIKE ? OR t.name LIKE ?)
           AND m.valid_to IS NULL
           AND (m.expires_at IS NULL OR m.expires_at > ?)
         ORDER BY m.importance DESC, m.created_at DESC
         LIMIT ?"
    };

    let mut stmt = conn.prepare(query)?;

    let memories = if let Some(ws) = workspace {
        stmt.query_map(
            params![ws, &pattern, &pattern, &now, limit as i64],
            memory_from_row,
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(
            params![&pattern, &pattern, &now, limit as i64],
            memory_from_row,
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };

    Ok(memories)
}

/// Search within session transcript chunks
pub fn search_sessions(
    conn: &Connection,
    query_text: &str,
    session_id: Option<&str>,
    workspace: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<Memory>> {
    let limit = limit.unwrap_or(20);
    let now = Utc::now().to_rfc3339();
    let pattern = format!("%{}%", query_text);

    // Build query based on filters
    // Session chunks are stored as TranscriptChunk type (not Context)
    let mut conditions = vec![
        "m.memory_type = 'transcript_chunk'",
        "m.valid_to IS NULL",
        "(m.expires_at IS NULL OR m.expires_at > ?)",
    ];
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

    // Add session filter via tags (tags are in junction table)
    let use_tag_join = session_id.is_some();
    if let Some(sid) = session_id {
        let tag_name = format!("session:{}", sid);
        conditions.push("t.name = ?");
        params_vec.push(Box::new(tag_name));
    }

    // Add workspace filter
    if let Some(ws) = workspace {
        conditions.push("m.workspace = ?");
        params_vec.push(Box::new(ws.to_string()));
    }

    // Add content search
    conditions.push("m.content LIKE ?");
    params_vec.push(Box::new(pattern));

    // Add limit
    params_vec.push(Box::new(limit as i64));

    // Build query with optional tag join
    let join_clause = if use_tag_join {
        "JOIN memory_tags mt ON m.id = mt.memory_id JOIN tags t ON mt.tag_id = t.id"
    } else {
        ""
    };

    let query = format!(
        "SELECT DISTINCT m.id, m.content, m.memory_type, m.importance, m.access_count,
                m.created_at, m.updated_at, m.last_accessed_at, m.owner_id,
                m.visibility, m.version, m.has_embedding, m.metadata,
                m.scope_type, m.scope_id, m.workspace, m.tier, m.expires_at, m.content_hash
         FROM memories m {} WHERE {} ORDER BY m.created_at DESC LIMIT ?",
        join_clause,
        conditions.join(" AND ")
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&query)?;
    let memories = stmt
        .query_map(params_refs.as_slice(), memory_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(memories)
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
                let memory2 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Second".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: metadata2,
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
                // Create daily memory with TTL of 1 hour
                let memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Temporary memory".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        workspace: None,
                        tier: MemoryTier::Daily, // Daily tier for expiring memories
                        defer_embedding: true,
                        ttl_seconds: Some(3600), // 1 hour
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
                    },
                )?;

                // Verify expires_at is set and tier is daily
                assert!(memory.expires_at.is_some());
                assert_eq!(memory.tier, MemoryTier::Daily);
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
                // Create a daily memory with TTL (will expire)
                let memory1 = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Memory to expire".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["test".to_string()],
                        metadata: HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        workspace: None,
                        tier: MemoryTier::Daily, // Daily tier for expiring memories
                        defer_embedding: true,
                        ttl_seconds: Some(3600), // 1 hour TTL
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                // Create 3 daily memories that we'll expire manually
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
                            workspace: None,
                            tier: MemoryTier::Daily, // Daily tier for expiring memories
                            defer_embedding: true,
                            ttl_seconds: Some(3600), // 1 hour TTL
                            dedup_mode: Default::default(),
                            dedup_threshold: None,
                            event_time: None,
                            event_duration_seconds: None,
                            trigger_pattern: None,
                            summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow, // First one allows
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Reject,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Skip,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Merge,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        event_time: None,
                        trigger_pattern: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Reject, // Should not reject - different scope
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Reject, // Should reject - same scope
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: false,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: false,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        summary_of_id: None,
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
                    None, // default workspace
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
                    None,
                    0.5,
                )?;
                assert!(result_low_threshold.is_some());

                // Test 3: Query with embedding not similar to anything (threshold too high)
                let query_orthogonal = vec![0.0, 0.0, 0.0, 1.0]; // Different direction
                let result_no_match = find_similar_by_embedding(
                    conn,
                    &query_orthogonal,
                    &MemoryScope::Global,
                    None,
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
                    None,
                    0.5,
                )?;
                assert!(result_wrong_scope.is_none());

                Ok(())
            })
            .unwrap();
    }
}
