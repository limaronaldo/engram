//! Session Context Tracking (Phase 8 - ENG-70, ENG-71)
//!
//! Extends session indexing with:
//! - Named session creation and management
//! - Memory-to-session linking
//! - Session-scoped memory search
//! - Session summarization
//! - Context role tracking (referenced, created, updated)
//!
//! This enables agents to:
//! - Track which memories were used in a session
//! - Search within session context
//! - Generate session summaries
//! - Export session data for analysis

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{EngramError, Result};
use crate::types::{Memory, MemoryId};

/// Role of a memory in a session context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextRole {
    /// Memory was referenced/read during session
    Referenced,
    /// Memory was created during session
    Created,
    /// Memory was updated during session
    Updated,
    /// Memory was explicitly added to context
    Pinned,
}

impl Default for ContextRole {
    fn default() -> Self {
        Self::Referenced
    }
}

impl std::fmt::Display for ContextRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextRole::Referenced => write!(f, "referenced"),
            ContextRole::Created => write!(f, "created"),
            ContextRole::Updated => write!(f, "updated"),
            ContextRole::Pinned => write!(f, "pinned"),
        }
    }
}

impl std::str::FromStr for ContextRole {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "referenced" => Ok(ContextRole::Referenced),
            "created" => Ok(ContextRole::Created),
            "updated" => Ok(ContextRole::Updated),
            "pinned" => Ok(ContextRole::Pinned),
            _ => Err(format!("Unknown context role: {}", s)),
        }
    }
}

/// A memory linked to a session with context information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemoryLink {
    /// Session ID
    pub session_id: String,
    /// Memory ID
    pub memory_id: MemoryId,
    /// When the memory was added to session
    pub added_at: DateTime<Utc>,
    /// Relevance score (0.0 - 1.0)
    pub relevance_score: f32,
    /// Role of the memory in the session
    pub context_role: ContextRole,
}

/// Extended session information with context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    /// Session ID
    pub session_id: String,
    /// Session title (optional)
    pub title: Option<String>,
    /// When the session started
    pub created_at: DateTime<Utc>,
    /// When the session ended (None if active)
    pub ended_at: Option<DateTime<Utc>>,
    /// Number of messages in the session
    pub message_count: i32,
    /// Workspace for the session
    pub workspace: String,
    /// Summary of the session (auto-generated or manual)
    pub summary: Option<String>,
    /// Active context (JSON-encoded working memory)
    pub context: Option<String>,
    /// Session metadata
    pub metadata: HashMap<String, serde_json::Value>,
    /// Linked memories with context info
    #[serde(default)]
    pub memories: Vec<SessionMemoryLink>,
}

/// Input for creating a new session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionInput {
    /// Optional custom session ID (auto-generated if not provided)
    pub session_id: Option<String>,
    /// Optional session title
    pub title: Option<String>,
    /// Initial context (JSON string)
    pub initial_context: Option<String>,
    /// Optional workspace (defaults to "default")
    pub workspace: Option<String>,
    /// Session metadata
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Result of session search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSearchResult {
    /// The memory
    pub memory: Memory,
    /// Search relevance score
    pub relevance_score: f32,
    /// Context role in the session
    pub context_role: ContextRole,
    /// When added to session
    pub added_at: DateTime<Utc>,
}

/// Session export format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExport {
    /// Session information
    pub session: SessionContext,
    /// All linked memories
    pub memories: Vec<Memory>,
    /// Export timestamp
    pub exported_at: DateTime<Utc>,
    /// Export format version
    pub format_version: String,
}

/// Create a new named session
pub fn create_session(conn: &Connection, input: CreateSessionInput) -> Result<SessionContext> {
    let session_id = input
        .session_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let metadata_json = serde_json::to_string(&input.metadata).unwrap_or_else(|_| "{}".to_string());
    let workspace = input.workspace.unwrap_or_else(|| "default".to_string());

    conn.execute(
        "INSERT INTO sessions (session_id, title, started_at, message_count, workspace, metadata, summary, context)
         VALUES (?, ?, ?, 0, ?, ?, NULL, ?)",
        params![
            session_id,
            input.title,
            now_str,
            workspace,
            metadata_json,
            input.initial_context
        ],
    )?;

    Ok(SessionContext {
        session_id,
        title: input.title,
        created_at: now,
        ended_at: None,
        message_count: 0,
        workspace,
        summary: None,
        context: input.initial_context,
        metadata: input.metadata,
        memories: vec![],
    })
}

/// Add a memory to a session's context
pub fn add_memory_to_session(
    conn: &Connection,
    session_id: &str,
    memory_id: MemoryId,
    relevance_score: f32,
    context_role: ContextRole,
) -> Result<SessionMemoryLink> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let role_str = context_role.to_string();

    // Check if session exists
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id = ?)",
        params![session_id],
        |row| row.get(0),
    )?;

    if !exists {
        return Err(EngramError::InvalidInput(format!(
            "Session not found: {}",
            session_id
        )));
    }

    // Insert or update the link
    conn.execute(
        "INSERT INTO session_memories (session_id, memory_id, added_at, relevance_score, context_role)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(session_id, memory_id) DO UPDATE SET
             relevance_score = MAX(relevance_score, excluded.relevance_score),
             context_role = excluded.context_role",
        params![session_id, memory_id, now_str, relevance_score, role_str],
    )?;

    Ok(SessionMemoryLink {
        session_id: session_id.to_string(),
        memory_id,
        added_at: now,
        relevance_score,
        context_role,
    })
}

/// Remove a memory from a session's context
pub fn remove_memory_from_session(
    conn: &Connection,
    session_id: &str,
    memory_id: MemoryId,
) -> Result<bool> {
    let rows = conn.execute(
        "DELETE FROM session_memories WHERE session_id = ? AND memory_id = ?",
        params![session_id, memory_id],
    )?;

    Ok(rows > 0)
}

/// Get all memories linked to a session
pub fn get_session_memories(
    conn: &Connection,
    session_id: &str,
    role_filter: Option<ContextRole>,
) -> Result<Vec<SessionMemoryLink>> {
    let base_query = "SELECT session_id, memory_id, added_at, relevance_score, context_role
                      FROM session_memories WHERE session_id = ?";

    let query = if role_filter.is_some() {
        format!("{} AND context_role = ?", base_query)
    } else {
        format!("{} ORDER BY relevance_score DESC", base_query)
    };

    let mut stmt = conn.prepare(&query)?;

    let links = if let Some(role) = role_filter {
        stmt.query_map(params![session_id, role.to_string()], parse_link)?
    } else {
        stmt.query_map(params![session_id], parse_link)?
    };

    Ok(links.filter_map(|r| r.ok()).collect::<Vec<_>>())
}

fn parse_link(row: &rusqlite::Row) -> rusqlite::Result<SessionMemoryLink> {
    let session_id: String = row.get(0)?;
    let memory_id: MemoryId = row.get(1)?;
    let added_at_str: String = row.get(2)?;
    let relevance_score: f32 = row.get(3)?;
    let role_str: String = row.get(4)?;

    let added_at = DateTime::parse_from_rfc3339(&added_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let context_role = role_str.parse().unwrap_or(ContextRole::Referenced);

    Ok(SessionMemoryLink {
        session_id,
        memory_id,
        added_at,
        relevance_score,
        context_role,
    })
}

/// Get a session with all its linked memories
pub fn get_session_context(conn: &Connection, session_id: &str) -> Result<Option<SessionContext>> {
    let row = conn.query_row(
        "SELECT session_id, title, started_at, ended_at, message_count, workspace, metadata, summary, context
         FROM sessions WHERE session_id = ?",
        params![session_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        },
    );

    match row {
        Ok((
            id,
            title,
            started_at_str,
            ended_at_str,
            message_count,
            workspace,
            metadata_str,
            summary,
            context,
        )) => {
            let created_at = DateTime::parse_from_rfc3339(&started_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            let ended_at = ended_at_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            });

            let metadata: HashMap<String, serde_json::Value> = metadata_str
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            let title = title.or_else(|| {
                metadata
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            });

            let memories = get_session_memories(conn, session_id, None)?;

            Ok(Some(SessionContext {
                session_id: id,
                title,
                created_at,
                ended_at,
                message_count,
                workspace,
                summary,
                context,
                metadata,
                memories,
            }))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Update session summary
pub fn update_session_summary(conn: &Connection, session_id: &str, summary: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    let rows = conn.execute(
        "UPDATE sessions SET summary = ?, ended_at = COALESCE(ended_at, ?) WHERE session_id = ?",
        params![summary, now, session_id],
    )?;

    if rows == 0 {
        return Err(EngramError::InvalidInput(format!(
            "Session not found: {}",
            session_id
        )));
    }

    Ok(())
}

/// Update session context (working memory)
pub fn update_session_context(conn: &Connection, session_id: &str, context: &str) -> Result<()> {
    let rows = conn.execute(
        "UPDATE sessions SET context = ? WHERE session_id = ?",
        params![context, session_id],
    )?;

    if rows == 0 {
        return Err(EngramError::InvalidInput(format!(
            "Session not found: {}",
            session_id
        )));
    }

    Ok(())
}

/// End a session
pub fn end_session(conn: &Connection, session_id: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    let rows = conn.execute(
        "UPDATE sessions SET ended_at = ? WHERE session_id = ? AND ended_at IS NULL",
        params![now, session_id],
    )?;

    if rows == 0 {
        // Check if session exists but was already ended
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE session_id = ?)",
            params![session_id],
            |row| row.get(0),
        )?;

        if !exists {
            return Err(EngramError::InvalidInput(format!(
                "Session not found: {}",
                session_id
            )));
        }
        // Session exists but already ended - that's OK
    }

    Ok(())
}

/// Search memories within a session's context
pub fn search_session_memories(
    conn: &Connection,
    session_id: &str,
    query: &str,
    limit: i64,
) -> Result<Vec<SessionSearchResult>> {
    // First get memory IDs linked to this session
    let memory_ids: Vec<MemoryId> = conn
        .prepare(
            "SELECT memory_id FROM session_memories WHERE session_id = ? ORDER BY relevance_score DESC",
        )?
        .query_map(params![session_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    if memory_ids.is_empty() {
        return Ok(vec![]);
    }

    // Build IN clause
    let placeholders: Vec<String> = memory_ids.iter().map(|_| "?".to_string()).collect();
    let in_clause = placeholders.join(", ");

    // Search within those memories using FTS
    let sql = format!(
        "SELECT m.id, m.content, m.memory_type, m.importance, m.access_count,
                m.created_at, m.updated_at, m.last_accessed_at, m.tags,
                m.workspace, m.tier, m.lifecycle_state,
                sm.relevance_score, sm.context_role, sm.added_at,
                bm25(memories_fts) as search_score
         FROM memories m
         JOIN session_memories sm ON m.id = sm.memory_id
         JOIN memories_fts ON memories_fts.rowid = m.id
         WHERE sm.session_id = ?
           AND m.id IN ({})
           AND memories_fts MATCH ?
         ORDER BY search_score * sm.relevance_score DESC
         LIMIT ?",
        in_clause
    );

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(session_id.to_string())];
    for id in &memory_ids {
        params_vec.push(Box::new(*id));
    }
    params_vec.push(Box::new(query.to_string()));
    params_vec.push(Box::new(limit));

    let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let results = stmt
        .query_map(params_refs.as_slice(), |row| {
            // Parse memory fields
            let id: MemoryId = row.get(0)?;
            let content: String = row.get(1)?;
            let memory_type_str: String = row.get(2)?;
            let importance: f32 = row.get(3)?;
            let access_count: i32 = row.get(4)?;
            let created_at_str: String = row.get(5)?;
            let updated_at_str: String = row.get(6)?;
            let last_accessed_str: Option<String> = row.get(7)?;
            let tags_str: Option<String> = row.get(8)?;
            let workspace: String = row.get(9)?;
            let tier_str: String = row.get(10)?;
            let lifecycle_str: String = row.get(11)?;
            let relevance_score: f32 = row.get(12)?;
            let context_role_str: String = row.get(13)?;
            let added_at_str: String = row.get(14)?;

            Ok((
                id,
                content,
                memory_type_str,
                importance,
                access_count,
                created_at_str,
                updated_at_str,
                last_accessed_str,
                tags_str,
                workspace,
                tier_str,
                lifecycle_str,
                relevance_score,
                context_role_str,
                added_at_str,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(
            |(
                id,
                content,
                memory_type_str,
                importance,
                access_count,
                created_at_str,
                updated_at_str,
                last_accessed_str,
                tags_str,
                workspace,
                tier_str,
                lifecycle_str,
                relevance_score,
                context_role_str,
                added_at_str,
            )| {
                let now = Utc::now();

                let memory = Memory {
                    id,
                    content,
                    memory_type: memory_type_str
                        .parse()
                        .unwrap_or(crate::types::MemoryType::Note),
                    tags: tags_str
                        .map(|s| serde_json::from_str(&s).unwrap_or_default())
                        .unwrap_or_default(),
                    metadata: HashMap::new(),
                    importance,
                    access_count,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or(now),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or(now),
                    last_accessed_at: last_accessed_str.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    }),
                    owner_id: None,
                    visibility: crate::types::Visibility::Private,
                    scope: crate::types::MemoryScope::Global,
                    workspace,
                    tier: tier_str
                        .parse()
                        .unwrap_or(crate::types::MemoryTier::Permanent),
                    version: 1,
                    has_embedding: false,
                    expires_at: None,
                    content_hash: None,
                    event_time: None,
                    event_duration_seconds: None,
                    trigger_pattern: None,
                    procedure_success_count: 0,
                    procedure_failure_count: 0,
                    summary_of_id: None,
                    lifecycle_state: lifecycle_str
                        .parse()
                        .unwrap_or(crate::types::LifecycleState::Active),
                };

                SessionSearchResult {
                    memory,
                    relevance_score,
                    context_role: context_role_str.parse().unwrap_or(ContextRole::Referenced),
                    added_at: DateTime::parse_from_rfc3339(&added_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or(now),
                }
            },
        )
        .collect();

    Ok(results)
}

/// Export a session with all its data
pub fn export_session(
    conn: &Connection,
    session_id: &str,
    include_content: bool,
) -> Result<SessionExport> {
    let session = get_session_context(conn, session_id)?
        .ok_or_else(|| EngramError::InvalidInput(format!("Session not found: {}", session_id)))?;

    // Get all linked memories
    let memory_ids: Vec<MemoryId> = session.memories.iter().map(|m| m.memory_id).collect();

    let mut memories = Vec::new();
    if !memory_ids.is_empty() {
        for id in memory_ids {
            match crate::storage::queries::get_memory(conn, id) {
                Ok(mut memory) => {
                    if !include_content {
                        memory.content.clear();
                    }
                    memories.push(memory);
                }
                Err(EngramError::NotFound(_)) => continue,
                Err(e) => return Err(e),
            }
        }
    }

    Ok(SessionExport {
        session,
        memories,
        exported_at: Utc::now(),
        format_version: "1.0".to_string(),
    })
}

/// List sessions with optional filters
pub fn list_sessions_extended(
    conn: &Connection,
    workspace: Option<&str>,
    active_only: bool,
    limit: i64,
    offset: i64,
) -> Result<Vec<SessionContext>> {
    let mut query = String::from(
        "SELECT session_id, title, started_at, ended_at, message_count, workspace, metadata, summary, context
         FROM sessions",
    );

    let mut filters = Vec::new();
    if active_only {
        filters.push("ended_at IS NULL");
    }
    if workspace.is_some() {
        filters.push("workspace = ?");
    }
    if !filters.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&filters.join(" AND "));
    }

    query.push_str(" ORDER BY started_at DESC LIMIT ? OFFSET ?");

    let mut stmt = conn.prepare(&query)?;
    let rows: Vec<(
        String,
        Option<String>,
        String,
        Option<String>,
        i32,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = if let Some(workspace) = workspace {
        let rows = stmt.query_map(params![workspace, limit, offset], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        let rows = stmt.query_map(params![limit, offset], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i32>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };

    let sessions = rows
        .into_iter()
        .map(
            |(
                id,
                title,
                started_at_str,
                ended_at_str,
                message_count,
                workspace,
                metadata_str,
                summary,
                context,
            )| {
                let now = Utc::now();
                let created_at = DateTime::parse_from_rfc3339(&started_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or(now);

                let ended_at = ended_at_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                });

                let metadata: HashMap<String, serde_json::Value> = metadata_str
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();
                let title = title.or_else(|| {
                    metadata
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                });

                SessionContext {
                    session_id: id,
                    title,
                    created_at,
                    ended_at,
                    message_count,
                    workspace,
                    summary,
                    context,
                    metadata,
                    memories: vec![], // Don't load memories for list view
                }
            },
        )
        .collect();

    Ok(sessions)
}

/// Get sessions that reference a specific memory
pub fn get_sessions_for_memory(
    conn: &Connection,
    memory_id: MemoryId,
) -> Result<Vec<SessionMemoryLink>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, memory_id, added_at, relevance_score, context_role
         FROM session_memories
         WHERE memory_id = ?
         ORDER BY added_at DESC",
    )?;

    let links: Vec<SessionMemoryLink> = stmt
        .query_map(params![memory_id], parse_link)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();

        // Create minimal schema for testing
        conn.execute_batch(
            r#"
            CREATE TABLE sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL UNIQUE,
                title TEXT,
                started_at TEXT NOT NULL,
                last_indexed_at TEXT,
                message_count INTEGER NOT NULL DEFAULT 0,
                chunk_count INTEGER NOT NULL DEFAULT 0,
                workspace TEXT NOT NULL DEFAULT 'default',
                metadata TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                summary TEXT,
                context TEXT,
                ended_at TEXT
            );

            CREATE TABLE memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                memory_type TEXT DEFAULT 'note',
                importance REAL DEFAULT 0.5,
                access_count INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_accessed_at TEXT,
                workspace TEXT DEFAULT 'default',
                tier TEXT DEFAULT 'permanent',
                lifecycle_state TEXT DEFAULT 'active',
                tags TEXT
            );

            CREATE TABLE session_memories (
                session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
                memory_id INTEGER NOT NULL,
                added_at TEXT NOT NULL,
                relevance_score REAL DEFAULT 1.0,
                context_role TEXT DEFAULT 'referenced',
                PRIMARY KEY (session_id, memory_id)
            );

            CREATE VIRTUAL TABLE memories_fts USING fts5(content);
            "#,
        )
        .unwrap();

        conn
    }

    #[test]
    fn test_create_session() {
        let conn = setup_test_db();

        let input = CreateSessionInput {
            session_id: Some("test-session-1".to_string()),
            title: Some("Test Session".to_string()),
            initial_context: Some(r#"{"topic": "testing"}"#.to_string()),
            workspace: None,
            metadata: HashMap::new(),
        };

        let session = create_session(&conn, input).unwrap();
        assert_eq!(session.session_id, "test-session-1");
        assert!(session.context.is_some());
    }

    #[test]
    fn test_add_memory_to_session() {
        let conn = setup_test_db();

        // Create session
        let input = CreateSessionInput {
            session_id: Some("test-session".to_string()),
            title: None,
            initial_context: None,
            workspace: None,
            metadata: HashMap::new(),
        };
        create_session(&conn, input).unwrap();

        // Create a memory
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO memories (content, created_at, updated_at) VALUES (?, ?, ?)",
            params!["Test memory", now, now],
        )
        .unwrap();

        // Add memory to session
        let link =
            add_memory_to_session(&conn, "test-session", 1, 0.9, ContextRole::Created).unwrap();

        assert_eq!(link.session_id, "test-session");
        assert_eq!(link.memory_id, 1);
        assert_eq!(link.context_role, ContextRole::Created);
    }

    #[test]
    fn test_get_session_context() {
        let conn = setup_test_db();

        // Create session
        let input = CreateSessionInput {
            session_id: Some("context-test".to_string()),
            title: None,
            initial_context: None,
            workspace: None,
            metadata: HashMap::new(),
        };
        create_session(&conn, input).unwrap();

        // Get context
        let context = get_session_context(&conn, "context-test").unwrap();
        assert!(context.is_some());
        assert_eq!(context.unwrap().session_id, "context-test");
    }

    #[test]
    fn test_context_role_parsing() {
        assert_eq!(
            "referenced".parse::<ContextRole>().unwrap(),
            ContextRole::Referenced
        );
        assert_eq!(
            "created".parse::<ContextRole>().unwrap(),
            ContextRole::Created
        );
        assert_eq!(
            "updated".parse::<ContextRole>().unwrap(),
            ContextRole::Updated
        );
        assert_eq!(
            "pinned".parse::<ContextRole>().unwrap(),
            ContextRole::Pinned
        );
    }

    #[test]
    fn test_end_session() {
        let conn = setup_test_db();

        let input = CreateSessionInput {
            session_id: Some("end-test".to_string()),
            title: None,
            initial_context: None,
            workspace: None,
            metadata: HashMap::new(),
        };
        create_session(&conn, input).unwrap();

        end_session(&conn, "end-test").unwrap();

        let session = get_session_context(&conn, "end-test").unwrap().unwrap();
        assert!(session.ended_at.is_some());
    }
}
