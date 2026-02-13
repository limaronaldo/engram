//! Point-in-Time Graph Queries (RML-899)
//!
//! Provides:
//! - Query memories as they existed at a specific timestamp
//! - Query cross-references valid at a specific time
//! - Historical graph traversal
//! - Time-range queries

use crate::error::{EngramError, Result};
use crate::types::{CrossReference, EdgeType, Memory, MemoryScope, MemoryTier, Visibility};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Options for point-in-time queries
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemporalQueryOptions {
    /// The point in time to query (None = current)
    pub as_of: Option<DateTime<Utc>>,
    /// Include memories created after this time
    pub created_after: Option<DateTime<Utc>>,
    /// Include memories created before this time
    pub created_before: Option<DateTime<Utc>>,
    /// Include memories updated after this time
    pub updated_after: Option<DateTime<Utc>>,
    /// Include memories updated before this time
    pub updated_before: Option<DateTime<Utc>>,
    /// Include deleted memories (if tracking soft deletes)
    #[serde(default)]
    pub include_deleted: bool,
}

impl TemporalQueryOptions {
    /// Create options for querying at a specific point in time
    pub fn as_of(timestamp: DateTime<Utc>) -> Self {
        Self {
            as_of: Some(timestamp),
            ..Default::default()
        }
    }

    /// Create options for a time range
    pub fn time_range(start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            created_after: Some(start),
            created_before: Some(end),
            ..Default::default()
        }
    }
}

/// Result of a temporal query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalMemory {
    /// The memory at the queried point in time
    pub memory: Memory,
    /// Version number at the queried time
    pub version_at_time: i32,
    /// Whether this is the current version
    pub is_current: bool,
    /// The queried timestamp
    pub queried_at: DateTime<Utc>,
}

/// Historical snapshot of a memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    /// Memory ID
    pub memory_id: i64,
    /// Version number
    pub version: i32,
    /// Content at this version
    pub content: String,
    /// Tags at this version
    pub tags: Vec<String>,
    /// Metadata at this version
    pub metadata: HashMap<String, serde_json::Value>,
    /// When this version was created
    pub created_at: DateTime<Utc>,
    /// Who created this version
    pub created_by: Option<String>,
    /// Summary of changes from previous version
    pub change_summary: Option<String>,
}

/// Temporal query engine
pub struct TemporalQueryEngine<'a> {
    conn: &'a Connection,
}

impl<'a> TemporalQueryEngine<'a> {
    /// Create a new temporal query engine
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Get a memory as it existed at a specific point in time
    pub fn get_memory_at(
        &self,
        memory_id: i64,
        as_of: DateTime<Utc>,
    ) -> Result<Option<TemporalMemory>> {
        // First, check if the memory existed at that time
        let memory_existed: Option<(String, String)> = self
            .conn
            .query_row(
                r#"
                SELECT created_at, content
                FROM memories
                WHERE id = ?1 AND created_at <= ?2
                "#,
                params![memory_id, as_of.to_rfc3339()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        if memory_existed.is_none() {
            return Ok(None);
        }

        // Find the version that was current at that time
        let version_result: Option<(i32, String, String)> = self
            .conn
            .query_row(
                r#"
                SELECT version, content, tags
                FROM memory_versions
                WHERE memory_id = ?1 AND created_at <= ?2
                ORDER BY version DESC
                LIMIT 1
                "#,
                params![memory_id, as_of.to_rfc3339()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        // Get current memory for comparison
        let current: Option<Memory> = self.get_current_memory(memory_id)?;

        if let Some((version, content, tags_json)) = version_result {
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

            // Build the memory as it was at that time
            if let Some(mut memory) = current.clone() {
                memory.content = content;
                memory.tags = tags;
                memory.version = version;

                let is_current = current.map(|c| c.version == version).unwrap_or(false);

                return Ok(Some(TemporalMemory {
                    memory,
                    version_at_time: version,
                    is_current,
                    queried_at: as_of,
                }));
            }
        }

        // If no version history, return current if it existed at that time
        if let Some(memory) = current {
            if memory.created_at <= as_of {
                return Ok(Some(TemporalMemory {
                    memory: memory.clone(),
                    version_at_time: memory.version,
                    is_current: true,
                    queried_at: as_of,
                }));
            }
        }

        Ok(None)
    }

    /// Get current memory by ID
    fn get_current_memory(&self, memory_id: i64) -> Result<Option<Memory>> {
        self.conn
            .query_row(
                r#"
                SELECT id, content, type, importance, access_count, created_at, updated_at,
                       last_accessed_at, owner_id, visibility, version, has_embedding
                FROM memories
                WHERE id = ?1
                "#,
                params![memory_id],
                |row| {
                    let memory_type_str: String = row.get(2)?;
                    let visibility_str: String = row.get(9)?;

                    Ok(Memory {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        memory_type: memory_type_str.parse().unwrap_or_default(),
                        tags: vec![], // Will be filled separately
                        metadata: HashMap::new(),
                        importance: row.get(3)?,
                        access_count: row.get(4)?,
                        created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                        updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                        last_accessed_at: row
                            .get::<_, Option<String>>(7)?
                            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                            .map(|dt| dt.with_timezone(&Utc)),
                        owner_id: row.get(8)?,
                        visibility: match visibility_str.as_str() {
                            "shared" => Visibility::Shared,
                            "public" => Visibility::Public,
                            _ => Visibility::Private,
                        },
                        scope: MemoryScope::Global,
                        workspace: "default".to_string(),
                        tier: MemoryTier::Permanent,
                        version: row.get(10)?,
                        has_embedding: row.get(11)?,
                        expires_at: None,
                        content_hash: None,
                        event_time: None,
                        event_duration_seconds: None,
                        trigger_pattern: None,
                        procedure_success_count: 0,
                        procedure_failure_count: 0,
                        summary_of_id: None,
                        lifecycle_state: crate::types::LifecycleState::Active,
                    })
                },
            )
            .optional()
            .map_err(EngramError::from)
    }

    /// Query memories within a time range
    pub fn query_time_range(
        &self,
        options: &TemporalQueryOptions,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        let mut conditions = vec!["1=1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

        if let Some(ref after) = options.created_after {
            conditions.push(format!("created_at >= ?{}", params.len() + 1));
            params.push(Box::new(after.to_rfc3339()));
        }

        if let Some(ref before) = options.created_before {
            conditions.push(format!("created_at <= ?{}", params.len() + 1));
            params.push(Box::new(before.to_rfc3339()));
        }

        if let Some(ref after) = options.updated_after {
            conditions.push(format!("updated_at >= ?{}", params.len() + 1));
            params.push(Box::new(after.to_rfc3339()));
        }

        if let Some(ref before) = options.updated_before {
            conditions.push(format!("updated_at <= ?{}", params.len() + 1));
            params.push(Box::new(before.to_rfc3339()));
        }

        let sql = format!(
            r#"
            SELECT id, content, type, importance, access_count, created_at, updated_at,
                   last_accessed_at, owner_id, visibility, version, has_embedding
            FROM memories
            WHERE {}
            ORDER BY created_at DESC
            LIMIT ?{}
            "#,
            conditions.join(" AND "),
            params.len() + 1
        );

        params.push(Box::new(limit));

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(&sql)?;
        let memories = stmt
            .query_map(params_refs.as_slice(), |row| {
                let memory_type_str: String = row.get(2)?;
                let visibility_str: String = row.get(9)?;

                Ok(Memory {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    memory_type: memory_type_str.parse().unwrap_or_default(),
                    tags: vec![],
                    metadata: HashMap::new(),
                    importance: row.get(3)?,
                    access_count: row.get(4)?,
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    last_accessed_at: row
                        .get::<_, Option<String>>(7)?
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    owner_id: row.get(8)?,
                    visibility: match visibility_str.as_str() {
                        "shared" => Visibility::Shared,
                        "public" => Visibility::Public,
                        _ => Visibility::Private,
                    },
                    scope: MemoryScope::Global, // Temporal queries default to global
                    workspace: "default".to_string(),
                    tier: MemoryTier::Permanent,
                    version: row.get(10)?,
                    has_embedding: row.get(11)?,
                    expires_at: None,   // Temporal queries don't track expiration
                    content_hash: None, // Temporal queries don't track content hash
                    event_time: None,
                    event_duration_seconds: None,
                    trigger_pattern: None,
                    procedure_success_count: 0,
                    procedure_failure_count: 0,
                    summary_of_id: None,
                    lifecycle_state: crate::types::LifecycleState::Active,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(memories)
    }

    /// Get cross-references valid at a specific point in time
    pub fn get_crossrefs_at(
        &self,
        memory_id: i64,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<CrossReference>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT from_id, to_id, edge_type, score, confidence, strength, source,
                   source_context, created_at, valid_from, valid_to, pinned
            FROM crossrefs
            WHERE (from_id = ?1 OR to_id = ?1)
              AND valid_from <= ?2
              AND (valid_to IS NULL OR valid_to > ?2)
            ORDER BY score DESC
            "#,
        )?;

        let crossrefs = stmt
            .query_map(params![memory_id, as_of.to_rfc3339()], |row| {
                let edge_type_str: String = row.get(2)?;
                let source_str: String = row.get(6)?;

                Ok(CrossReference {
                    from_id: row.get(0)?,
                    to_id: row.get(1)?,
                    edge_type: edge_type_str.parse().unwrap_or_default(),
                    score: row.get(3)?,
                    confidence: row.get(4)?,
                    strength: row.get(5)?,
                    source: match source_str.as_str() {
                        "manual" => crate::types::RelationSource::Manual,
                        "llm" => crate::types::RelationSource::Llm,
                        _ => crate::types::RelationSource::Auto,
                    },
                    source_context: row.get(7)?,
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    valid_from: DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    valid_to: row
                        .get::<_, Option<String>>(10)?
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    pinned: row.get(11)?,
                    metadata: HashMap::new(),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(crossrefs)
    }

    /// Get version history for a memory
    pub fn get_version_history(&self, memory_id: i64) -> Result<Vec<MemorySnapshot>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT memory_id, version, content, tags, metadata, created_at, created_by, change_summary
            FROM memory_versions
            WHERE memory_id = ?1
            ORDER BY version DESC
            "#,
        )?;

        let snapshots = stmt
            .query_map(params![memory_id], |row| {
                let tags_json: String = row.get(3)?;
                let metadata_json: String = row.get(4)?;

                Ok(MemorySnapshot {
                    memory_id: row.get(0)?,
                    version: row.get(1)?,
                    content: row.get(2)?,
                    tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                    metadata: serde_json::from_str(&metadata_json).unwrap_or_default(),
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    created_by: row.get(6)?,
                    change_summary: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(snapshots)
    }

    /// Get a specific version of a memory
    pub fn get_memory_version(
        &self,
        memory_id: i64,
        version: i32,
    ) -> Result<Option<MemorySnapshot>> {
        self.conn
            .query_row(
                r#"
                SELECT memory_id, version, content, tags, metadata, created_at, created_by, change_summary
                FROM memory_versions
                WHERE memory_id = ?1 AND version = ?2
                "#,
                params![memory_id, version],
                |row| {
                    let tags_json: String = row.get(3)?;
                    let metadata_json: String = row.get(4)?;

                    Ok(MemorySnapshot {
                        memory_id: row.get(0)?,
                        version: row.get(1)?,
                        content: row.get(2)?,
                        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                        metadata: serde_json::from_str(&metadata_json).unwrap_or_default(),
                        created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                        created_by: row.get(6)?,
                        change_summary: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(EngramError::from)
    }

    /// Traverse the graph as it existed at a point in time
    pub fn traverse_graph_at(
        &self,
        start_id: i64,
        as_of: DateTime<Utc>,
        depth: usize,
        edge_types: Option<Vec<EdgeType>>,
    ) -> Result<Vec<(Memory, CrossReference)>> {
        let mut visited = std::collections::HashSet::new();
        let mut results = Vec::new();
        let mut to_visit = vec![(start_id, 0usize)];

        while let Some((current_id, current_depth)) = to_visit.pop() {
            if current_depth >= depth || visited.contains(&current_id) {
                continue;
            }
            visited.insert(current_id);

            // Get cross-references valid at that time
            let crossrefs = self.get_crossrefs_at(current_id, as_of)?;

            for crossref in crossrefs {
                // Filter by edge type if specified
                if let Some(ref types) = edge_types {
                    if !types.contains(&crossref.edge_type) {
                        continue;
                    }
                }

                // Get the connected memory
                let other_id = if crossref.from_id == current_id {
                    crossref.to_id
                } else {
                    crossref.from_id
                };

                if let Some(temporal_memory) = self.get_memory_at(other_id, as_of)? {
                    results.push((temporal_memory.memory, crossref.clone()));
                    to_visit.push((other_id, current_depth + 1));
                }
            }
        }

        Ok(results)
    }

    /// Compare two points in time
    pub fn compare_states(
        &self,
        memory_id: i64,
        time1: DateTime<Utc>,
        time2: DateTime<Utc>,
    ) -> Result<StateDiff> {
        let state1 = self.get_memory_at(memory_id, time1)?;
        let state2 = self.get_memory_at(memory_id, time2)?;

        let crossrefs1 = self.get_crossrefs_at(memory_id, time1)?;
        let crossrefs2 = self.get_crossrefs_at(memory_id, time2)?;

        Ok(StateDiff {
            memory_id,
            time1,
            time2,
            memory_state1: state1.map(|t| t.memory),
            memory_state2: state2.map(|t| t.memory),
            crossrefs_added: crossrefs2
                .iter()
                .filter(|c| {
                    !crossrefs1
                        .iter()
                        .any(|c1| c1.to_id == c.to_id && c1.from_id == c.from_id)
                })
                .cloned()
                .collect(),
            crossrefs_removed: crossrefs1
                .iter()
                .filter(|c| {
                    !crossrefs2
                        .iter()
                        .any(|c2| c2.to_id == c.to_id && c2.from_id == c.from_id)
                })
                .cloned()
                .collect(),
        })
    }
}

/// Difference between two points in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDiff {
    pub memory_id: i64,
    pub time1: DateTime<Utc>,
    pub time2: DateTime<Utc>,
    pub memory_state1: Option<Memory>,
    pub memory_state2: Option<Memory>,
    pub crossrefs_added: Vec<CrossReference>,
    pub crossrefs_removed: Vec<CrossReference>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temporal_query_options_default() {
        let options = TemporalQueryOptions::default();
        assert!(options.as_of.is_none());
        assert!(options.created_after.is_none());
        assert!(!options.include_deleted);
    }

    #[test]
    fn test_temporal_query_options_as_of() {
        let now = Utc::now();
        let options = TemporalQueryOptions::as_of(now);
        assert_eq!(options.as_of, Some(now));
    }

    #[test]
    fn test_temporal_query_options_time_range() {
        let start = Utc::now() - chrono::Duration::days(7);
        let end = Utc::now();
        let options = TemporalQueryOptions::time_range(start, end);
        assert_eq!(options.created_after, Some(start));
        assert_eq!(options.created_before, Some(end));
    }
}
