//! Session transcript indexing with dual-limiter chunking
//!
//! Implements conversation indexing with:
//! - Dual-limiter chunking (messages + characters)
//! - Overlap preservation for context continuity
//! - Delta updates for incremental indexing
//! - TranscriptChunk memory type with 7-day default TTL
//!
//! Based on Fix 6 from the design plan:
//! > Dual-limiter chunking algorithm with max_messages AND max_chars

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{EngramError, Result};
use crate::storage::queries::create_memory;
use crate::types::{CreateMemoryInput, MemoryTier, MemoryType};

/// Configuration for conversation chunking
#[derive(Debug, Clone)]
pub struct ChunkingConfig {
    /// Maximum messages per chunk (default: 10)
    pub max_messages: usize,
    /// Overlap in messages between chunks (default: 2)
    pub overlap_messages: usize,
    /// Maximum characters per chunk (default: 8000, ~2000 tokens)
    pub max_chars: usize,
    /// Default TTL for transcript chunks in seconds (default: 7 days)
    pub default_ttl_seconds: i64,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            max_messages: 10,
            overlap_messages: 2,
            max_chars: 8000,
            default_ttl_seconds: 7 * 24 * 60 * 60, // 7 days
        }
    }
}

/// A message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message role (user, assistant, system)
    pub role: String,
    /// Message content
    pub content: String,
    /// Message timestamp
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
    /// Optional message ID
    pub id: Option<String>,
}

/// A chunk of conversation messages
#[derive(Debug, Clone)]
pub struct ConversationChunk {
    /// Index of this chunk in the conversation
    pub chunk_index: usize,
    /// Start message index (inclusive)
    pub start_index: usize,
    /// End message index (exclusive)
    pub end_index: usize,
    /// The messages in this chunk
    pub messages: Vec<Message>,
    /// Combined content for embedding
    pub content: String,
    /// Character count
    pub char_count: usize,
}

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier
    pub session_id: String,
    /// Optional title
    pub title: Option<String>,
    /// Agent ID if applicable
    pub agent_id: Option<String>,
    /// When the session started
    pub started_at: DateTime<Utc>,
    /// Last time the session was indexed
    pub last_indexed_at: Option<DateTime<Utc>>,
    /// Number of messages indexed
    pub message_count: i64,
    /// Number of chunks created
    pub chunk_count: i64,
    /// Workspace for the session
    pub workspace: String,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Chunk a conversation using the dual-limiter algorithm.
///
/// The algorithm ensures chunks respect both message count AND character limits:
/// 1. Start a new chunk
/// 2. Add messages until either limit is reached
/// 3. If a single message exceeds max_chars, truncate it with marker
/// 4. Close the chunk and start a new one with overlap
///
/// # Arguments
/// - `messages`: The conversation messages to chunk
/// - `config`: Chunking configuration
///
/// # Returns
/// Vector of conversation chunks
pub fn chunk_conversation(messages: &[Message], config: &ChunkingConfig) -> Vec<ConversationChunk> {
    if messages.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::new();
    let mut chunk_start = 0;

    while chunk_start < messages.len() {
        let mut current_messages = Vec::new();
        let mut current_chars = 0;
        let mut i = chunk_start;

        // Build chunk until we hit a limit
        while i < messages.len() {
            let msg = &messages[i];
            let msg_chars = msg.content.len();

            // Handle very long messages - truncate with marker
            let (content, chars) = if msg_chars > config.max_chars {
                let truncated = truncate_with_marker(&msg.content, config.max_chars);
                (truncated.clone(), truncated.len())
            } else {
                (msg.content.clone(), msg_chars)
            };

            // Check if adding this message would exceed limits
            let would_exceed_chars =
                current_chars + chars > config.max_chars && !current_messages.is_empty();
            let would_exceed_messages = current_messages.len() >= config.max_messages;

            if would_exceed_chars || would_exceed_messages {
                break;
            }

            // Add message to chunk
            current_messages.push(Message {
                role: msg.role.clone(),
                content,
                timestamp: msg.timestamp,
                id: msg.id.clone(),
            });
            current_chars += chars;
            i += 1;
        }

        // Create chunk if we have messages
        if !current_messages.is_empty() {
            let chunk_content = format_chunk_content(&current_messages);
            chunks.push(ConversationChunk {
                chunk_index: chunks.len(),
                start_index: chunk_start,
                end_index: i,
                messages: current_messages,
                content: chunk_content.clone(),
                char_count: chunk_content.len(),
            });
        }

        // Move to next chunk with overlap
        let overlap = config.overlap_messages.min(i - chunk_start);
        chunk_start = if i >= messages.len() {
            messages.len() // Done
        } else if i > chunk_start + overlap {
            i - overlap
        } else {
            i // Can't overlap, just continue
        };
    }

    chunks
}

/// Truncate content with a marker preserving head and tail
fn truncate_with_marker(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }

    // Preserve 60% head, 30% tail, 10% for marker
    let marker = "\n[...truncated...]\n";
    let available = max_chars - marker.len();
    let head_len = (available * 60) / 100;
    let tail_len = available - head_len;

    let head: String = content.chars().take(head_len).collect();
    let tail: String = content
        .chars()
        .rev()
        .take(tail_len)
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    format!("{}{}{}", head, marker, tail)
}

/// Format chunk messages into a single content string
fn format_chunk_content(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| format!("[{}]: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Index a full conversation into memory chunks.
///
/// Creates TranscriptChunk memories with 7-day TTL by default.
/// Stores session metadata and chunk mappings.
///
/// # Arguments
/// - `conn`: Database connection
/// - `session_id`: Unique session identifier
/// - `messages`: The conversation messages
/// - `config`: Chunking configuration
/// - `workspace`: Optional workspace (default: "default")
/// - `title`: Optional session title
/// - `agent_id`: Optional agent identifier
///
/// # Returns
/// The created session with chunk information
pub fn index_conversation(
    conn: &Connection,
    session_id: &str,
    messages: &[Message],
    config: &ChunkingConfig,
    workspace: Option<&str>,
    title: Option<&str>,
    agent_id: Option<&str>,
) -> Result<Session> {
    let now = Utc::now();
    let workspace = workspace.unwrap_or("default");

    // Chunk the conversation
    let chunks = chunk_conversation(messages, config);

    if chunks.is_empty() {
        return Err(EngramError::InvalidInput(
            "No messages to index".to_string(),
        ));
    }

    // Store the last N messages as overlap for future delta updates
    let overlap_messages: Vec<&Message> = messages
        .iter()
        .rev()
        .take(config.overlap_messages)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    let mut session_metadata = HashMap::new();
    session_metadata.insert(
        "overlap_messages".to_string(),
        serde_json::to_value(&overlap_messages).unwrap_or_default(),
    );
    let metadata_json = serde_json::to_string(&session_metadata)?;

    // Create or update session record
    let started_at = messages.first().map(|m| m.timestamp).unwrap_or(now);

    conn.execute(
        r#"
        INSERT INTO sessions (session_id, title, agent_id, started_at, last_indexed_at,
                             message_count, chunk_count, workspace, metadata)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(session_id) DO UPDATE SET
            title = COALESCE(excluded.title, sessions.title),
            last_indexed_at = excluded.last_indexed_at,
            message_count = excluded.message_count,
            chunk_count = excluded.chunk_count,
            metadata = excluded.metadata
        "#,
        params![
            session_id,
            title,
            agent_id,
            started_at.to_rfc3339(),
            now.to_rfc3339(),
            messages.len() as i64,
            chunks.len() as i64,
            workspace,
            metadata_json,
        ],
    )?;

    // Delete existing chunks for this session (full reindex)
    conn.execute(
        "DELETE FROM session_chunks WHERE session_id = ?",
        params![session_id],
    )?;

    // Create memory for each chunk
    for chunk in &chunks {
        let mut metadata = HashMap::new();
        metadata.insert("session_id".to_string(), serde_json::json!(session_id));
        metadata.insert(
            "chunk_index".to_string(),
            serde_json::json!(chunk.chunk_index),
        );
        metadata.insert(
            "start_message".to_string(),
            serde_json::json!(chunk.start_index),
        );
        metadata.insert(
            "end_message".to_string(),
            serde_json::json!(chunk.end_index),
        );
        metadata.insert(
            "message_count".to_string(),
            serde_json::json!(chunk.messages.len()),
        );

        let input = CreateMemoryInput {
            content: chunk.content.clone(),
            memory_type: MemoryType::TranscriptChunk,
            tags: vec!["transcript".to_string(), format!("session:{}", session_id)],
            metadata,
            importance: Some(0.3), // Lower importance for transcript chunks
            scope: Default::default(),
            workspace: Some(workspace.to_string()),
            tier: MemoryTier::Daily, // Transcript chunks are ephemeral by default
            defer_embedding: false,
            ttl_seconds: Some(config.default_ttl_seconds),
            dedup_mode: Default::default(),
            dedup_threshold: None,
            event_time: None,
            event_duration_seconds: None,
            trigger_pattern: None,
            summary_of_id: None,
        };

        let memory = create_memory(conn, &input)?;

        // Record chunk mapping
        conn.execute(
            r#"
            INSERT INTO session_chunks (session_id, memory_id, chunk_index,
                                       start_message_index, end_message_index)
            VALUES (?, ?, ?, ?, ?)
            "#,
            params![
                session_id,
                memory.id,
                chunk.chunk_index as i64,
                chunk.start_index as i64,
                chunk.end_index as i64,
            ],
        )?;
    }

    tracing::info!(
        session_id = session_id,
        message_count = messages.len(),
        chunk_count = chunks.len(),
        "Indexed conversation"
    );

    Ok(Session {
        session_id: session_id.to_string(),
        title: title.map(String::from),
        agent_id: agent_id.map(String::from),
        started_at,
        last_indexed_at: Some(now),
        message_count: messages.len() as i64,
        chunk_count: chunks.len() as i64,
        workspace: workspace.to_string(),
        metadata: HashMap::new(),
    })
}

/// Index new messages incrementally (delta update).
///
/// Only indexes messages that haven't been indexed yet.
///
/// # Arguments
/// - `conn`: Database connection
/// - `session_id`: Session to update
/// - `new_messages`: New messages to add
/// - `config`: Chunking configuration
///
/// # Returns
/// Updated session information
pub fn index_conversation_delta(
    conn: &Connection,
    session_id: &str,
    new_messages: &[Message],
    config: &ChunkingConfig,
) -> Result<Session> {
    // Get existing session
    let session: Option<Session> = conn
        .query_row(
            "SELECT session_id, title, agent_id, started_at, last_indexed_at,
                    message_count, chunk_count, workspace, metadata
             FROM sessions WHERE session_id = ?",
            params![session_id],
            |row| {
                let started_at: String = row.get(3)?;
                let last_indexed_at: Option<String> = row.get(4)?;
                let metadata_str: String = row.get(8)?;
                Ok(Session {
                    session_id: row.get(0)?,
                    title: row.get(1)?,
                    agent_id: row.get(2)?,
                    started_at: DateTime::parse_from_rfc3339(&started_at)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    last_indexed_at: last_indexed_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    }),
                    message_count: row.get(5)?,
                    chunk_count: row.get(6)?,
                    workspace: row.get(7)?,
                    metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
                })
            },
        )
        .ok();

    match session {
        Some(existing) => {
            // Get the last chunk's end index to determine overlap
            let last_chunk_end: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(end_message_index), 0) FROM session_chunks WHERE session_id = ?",
                    params![session_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            // Retrieve overlap messages from session metadata (stored from previous indexing)
            let overlap_messages: Vec<Message> = existing
                .metadata
                .get("overlap_messages")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            // Track overlap count before moving for offset calculation later
            let overlap_count = overlap_messages.len();

            // Combine overlap messages with new messages for proper context continuity
            let mut all_messages = overlap_messages;
            all_messages.extend(new_messages.iter().cloned());

            // Chunk the combined messages (overlap + new)
            let chunks = chunk_conversation(&all_messages, config);

            if chunks.is_empty() {
                return Ok(existing);
            }

            let now = Utc::now();
            let new_message_count = existing.message_count + new_messages.len() as i64;
            let starting_chunk_index = existing.chunk_count;

            // Store the last N messages as overlap for next delta update
            // Take from the end of all_messages (which includes both old overlap + new messages)
            let new_overlap: Vec<&Message> = all_messages
                .iter()
                .rev()
                .take(config.overlap_messages)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();

            // Update session metadata with new overlap messages
            let mut updated_metadata = existing.metadata.clone();
            updated_metadata.insert(
                "overlap_messages".to_string(),
                serde_json::to_value(&new_overlap).unwrap_or_default(),
            );
            let metadata_json = serde_json::to_string(&updated_metadata)?;

            // Update session
            conn.execute(
                "UPDATE sessions SET last_indexed_at = ?, message_count = ?, chunk_count = ?, metadata = ? WHERE session_id = ?",
                params![
                    now.to_rfc3339(),
                    new_message_count,
                    existing.chunk_count + chunks.len() as i64,
                    metadata_json,
                    session_id,
                ],
            )?;

            // Calculate the offset for global message indices
            // The chunk indices are relative to all_messages (overlap + new_messages).
            // Since overlap messages were already indexed in previous chunks,
            // we need to subtract the overlap count to avoid double-counting.
            // global_index = chunk_local_index + last_chunk_end - overlap_count
            let base_offset = (last_chunk_end as usize).saturating_sub(overlap_count);

            // Create memory for each new chunk
            for (i, chunk) in chunks.iter().enumerate() {
                let chunk_index = starting_chunk_index as usize + i;

                // Calculate global message indices
                let global_start = chunk.start_index + base_offset;
                let global_end = chunk.end_index + base_offset;

                let mut metadata = HashMap::new();
                metadata.insert("session_id".to_string(), serde_json::json!(session_id));
                metadata.insert("chunk_index".to_string(), serde_json::json!(chunk_index));
                metadata.insert("start_message".to_string(), serde_json::json!(global_start));
                metadata.insert("end_message".to_string(), serde_json::json!(global_end));
                metadata.insert(
                    "message_count".to_string(),
                    serde_json::json!(chunk.messages.len()),
                );

                let input = CreateMemoryInput {
                    content: chunk.content.clone(),
                    memory_type: MemoryType::TranscriptChunk,
                    tags: vec!["transcript".to_string(), format!("session:{}", session_id)],
                    metadata,
                    importance: Some(0.3),
                    scope: Default::default(),
                    workspace: Some(existing.workspace.clone()),
                    tier: MemoryTier::Daily,
                    defer_embedding: false,
                    ttl_seconds: Some(config.default_ttl_seconds),
                    dedup_mode: Default::default(),
                    dedup_threshold: None,
                    event_time: None,
                    event_duration_seconds: None,
                    trigger_pattern: None,
                    summary_of_id: None,
                };

                let memory = create_memory(conn, &input)?;

                conn.execute(
                    r#"
                    INSERT INTO session_chunks (session_id, memory_id, chunk_index,
                                               start_message_index, end_message_index)
                    VALUES (?, ?, ?, ?, ?)
                    "#,
                    params![
                        session_id,
                        memory.id,
                        chunk_index as i64,
                        global_start as i64,
                        global_end as i64,
                    ],
                )?;
            }

            Ok(Session {
                message_count: new_message_count,
                chunk_count: existing.chunk_count + chunks.len() as i64,
                last_indexed_at: Some(now),
                ..existing
            })
        }
        None => {
            // No existing session, create new one
            index_conversation(conn, session_id, new_messages, config, None, None, None)
        }
    }
}

/// Get a session by ID
pub fn get_session(conn: &Connection, session_id: &str) -> Result<Session> {
    conn.query_row(
        "SELECT session_id, title, agent_id, started_at, last_indexed_at,
                message_count, chunk_count, workspace, metadata
         FROM sessions WHERE session_id = ?",
        params![session_id],
        |row| {
            let started_at: String = row.get(3)?;
            let last_indexed_at: Option<String> = row.get(4)?;
            let metadata_str: String = row.get(8)?;
            Ok(Session {
                session_id: row.get(0)?,
                title: row.get(1)?,
                agent_id: row.get(2)?,
                started_at: DateTime::parse_from_rfc3339(&started_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                last_indexed_at: last_indexed_at.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
                message_count: row.get(5)?,
                chunk_count: row.get(6)?,
                workspace: row.get(7)?,
                metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
            })
        },
    )
    .map_err(|_| EngramError::NotFound(0))
}

/// List all sessions with optional workspace filter
pub fn list_sessions(
    conn: &Connection,
    workspace: Option<&str>,
    limit: i64,
) -> Result<Vec<Session>> {
    let mut sql = String::from(
        "SELECT session_id, title, agent_id, started_at, last_indexed_at,
                message_count, chunk_count, workspace, metadata
         FROM sessions",
    );

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if let Some(ws) = workspace {
        sql.push_str(" WHERE workspace = ?");
        params.push(Box::new(ws.to_string()));
    }

    sql.push_str(" ORDER BY started_at DESC LIMIT ?");
    params.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;

    let sessions = stmt
        .query_map(param_refs.as_slice(), |row| {
            let started_at: String = row.get(3)?;
            let last_indexed_at: Option<String> = row.get(4)?;
            let metadata_str: String = row.get(8)?;
            Ok(Session {
                session_id: row.get(0)?,
                title: row.get(1)?,
                agent_id: row.get(2)?,
                started_at: DateTime::parse_from_rfc3339(&started_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                last_indexed_at: last_indexed_at.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
                message_count: row.get(5)?,
                chunk_count: row.get(6)?,
                workspace: row.get(7)?,
                metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(sessions)
}

/// Delete a session and all its chunks
pub fn delete_session(conn: &Connection, session_id: &str) -> Result<()> {
    // Delete memories associated with chunks (soft delete)
    conn.execute(
        r#"
        UPDATE memories SET valid_to = datetime('now')
        WHERE id IN (SELECT memory_id FROM session_chunks WHERE session_id = ?)
        "#,
        params![session_id],
    )?;

    // Delete chunk mappings
    conn.execute(
        "DELETE FROM session_chunks WHERE session_id = ?",
        params![session_id],
    )?;

    // Delete session
    conn.execute(
        "DELETE FROM sessions WHERE session_id = ?",
        params![session_id],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(count: usize, char_len: usize) -> Vec<Message> {
        (0..count)
            .map(|i| Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
                content: format!("Message {} {}", i, "x".repeat(char_len)),
                timestamp: Utc::now(),
                id: Some(format!("msg-{}", i)),
            })
            .collect()
    }

    #[test]
    fn test_chunk_by_message_count() {
        let config = ChunkingConfig {
            max_messages: 3,
            overlap_messages: 1,
            max_chars: 100000, // High limit, won't trigger
            ..Default::default()
        };

        let messages = make_messages(7, 10);
        let chunks = chunk_conversation(&messages, &config);

        // Expected: [0,1,2], [2,3,4], [4,5,6]
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].start_index, 0);
        assert_eq!(chunks[0].end_index, 3);
        assert_eq!(chunks[1].start_index, 2); // Overlap
        assert_eq!(chunks[1].end_index, 5);
        assert_eq!(chunks[2].start_index, 4); // Overlap
        assert_eq!(chunks[2].end_index, 7);
    }

    #[test]
    fn test_chunk_by_char_count() {
        let config = ChunkingConfig {
            max_messages: 100, // High limit, won't trigger
            overlap_messages: 1,
            max_chars: 100, // Low limit, will trigger
            ..Default::default()
        };

        // Each message ~30 chars, so ~3 per chunk
        let messages = make_messages(9, 20);
        let chunks = chunk_conversation(&messages, &config);

        // Should create multiple chunks based on char limit
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.char_count <= config.max_chars + 50); // Some tolerance for formatting
        }
    }

    #[test]
    fn test_truncate_long_message() {
        let config = ChunkingConfig {
            max_messages: 10,
            overlap_messages: 1,
            max_chars: 100,
            ..Default::default()
        };

        let long_content = "x".repeat(200);
        let messages = vec![Message {
            role: "user".to_string(),
            content: long_content,
            timestamp: Utc::now(),
            id: None,
        }];

        let chunks = chunk_conversation(&messages, &config);

        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].messages[0].content.contains("[...truncated...]"));
        assert!(chunks[0].messages[0].content.len() <= config.max_chars);
    }

    #[test]
    fn test_empty_conversation() {
        let config = ChunkingConfig::default();
        let messages: Vec<Message> = vec![];
        let chunks = chunk_conversation(&messages, &config);

        assert!(chunks.is_empty());
    }

    #[test]
    fn test_format_chunk_content() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: "Hello".to_string(),
                timestamp: Utc::now(),
                id: None,
            },
            Message {
                role: "assistant".to_string(),
                content: "Hi there!".to_string(),
                timestamp: Utc::now(),
                id: None,
            },
        ];

        let content = format_chunk_content(&messages);
        assert!(content.contains("[user]: Hello"));
        assert!(content.contains("[assistant]: Hi there!"));
    }
}
