//! Letta/MemGPT-inspired self-editing memory blocks.
//!
//! Memory blocks are named, versioned text slots that an AI agent can read and
//! overwrite during a session.  Each write increments the version counter and
//! appends a row to `block_edit_log` so the full rewrite history is preserved.
//!
//! # Design
//! - One row per block name (PRIMARY KEY on `name`).
//! - Overflow detection uses a rough 4 chars/token heuristic; if the content
//!   exceeds `max_tokens * 4` bytes the excess is returned and the block is
//!   truncated to the allowed length.
//! - All timestamps follow the project-wide convention: RFC 3339 UTC strings.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{EngramError, Result};

// ─── DDL ─────────────────────────────────────────────────────────────────────

/// SQL that creates the `memory_blocks` table.
///
/// Embed this in a migration when integrating with the main schema.
pub const CREATE_MEMORY_BLOCKS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS memory_blocks (
    name       TEXT    PRIMARY KEY,
    content    TEXT    NOT NULL DEFAULT '',
    version    INTEGER NOT NULL DEFAULT 1,
    max_tokens INTEGER NOT NULL DEFAULT 4096,
    created_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
"#;

/// SQL that creates the `block_edit_log` table.
pub const CREATE_BLOCK_EDIT_LOG_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS block_edit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    block_name  TEXT    NOT NULL,
    old_content TEXT    NOT NULL,
    new_content TEXT    NOT NULL,
    edit_reason TEXT    NOT NULL DEFAULT '',
    timestamp   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (block_name) REFERENCES memory_blocks(name) ON DELETE CASCADE
);
"#;

// ─── Types ───────────────────────────────────────────────────────────────────

/// A named, versioned text slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryBlock {
    pub name: String,
    pub content: String,
    pub version: i64,
    pub max_tokens: usize,
    pub created_at: String,
    pub updated_at: String,
}

/// A single entry in the edit history for a block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockEditLog {
    pub id: i64,
    pub block_name: String,
    pub old_content: String,
    pub new_content: String,
    pub edit_reason: String,
    pub timestamp: String,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn row_to_block(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryBlock> {
    Ok(MemoryBlock {
        name: row.get(0)?,
        content: row.get(1)?,
        version: row.get(2)?,
        max_tokens: row.get::<_, i64>(3)? as usize,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn row_to_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<BlockEditLog> {
    Ok(BlockEditLog {
        id: row.get(0)?,
        block_name: row.get(1)?,
        old_content: row.get(2)?,
        new_content: row.get(3)?,
        edit_reason: row.get(4)?,
        timestamp: row.get(5)?,
    })
}

// ─── Storage functions ───────────────────────────────────────────────────────

/// Create a new memory block.
///
/// Returns an error if a block with the same `name` already exists.
pub fn create_block(
    conn: &Connection,
    name: &str,
    content: &str,
    max_tokens: usize,
) -> Result<MemoryBlock> {
    if name.is_empty() {
        return Err(EngramError::InvalidInput(
            "block name must not be empty".into(),
        ));
    }

    conn.execute(
        r#"
        INSERT INTO memory_blocks (name, content, version, max_tokens, created_at, updated_at)
        VALUES (?, ?, 1, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        "#,
        params![name, content, max_tokens as i64],
    )?;

    get_block_required(conn, name)
}

/// Retrieve a block by name, returning `None` when it does not exist.
pub fn get_block(conn: &Connection, name: &str) -> Result<Option<MemoryBlock>> {
    let result = conn
        .query_row(
            r#"
            SELECT name, content, version, max_tokens, created_at, updated_at
            FROM memory_blocks WHERE name = ?
            "#,
            params![name],
            row_to_block,
        )
        .optional()?;
    Ok(result)
}

/// Update a block's content, increment its version, and record the edit.
///
/// Returns an error when the block does not exist.
pub fn update_block(conn: &Connection, name: &str, new_content: &str, reason: &str) -> Result<MemoryBlock> {
    let old = get_block(conn, name)?.ok_or_else(|| {
        EngramError::Storage(format!("memory block '{}' not found", name))
    })?;

    conn.execute(
        r#"
        UPDATE memory_blocks
        SET content    = ?,
            version    = version + 1,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
        WHERE name = ?
        "#,
        params![new_content, name],
    )?;

    conn.execute(
        r#"
        INSERT INTO block_edit_log (block_name, old_content, new_content, edit_reason, timestamp)
        VALUES (?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        "#,
        params![name, old.content, new_content, reason],
    )?;

    get_block_required(conn, name)
}

/// Return all memory blocks ordered by name.
pub fn list_blocks(conn: &Connection) -> Result<Vec<MemoryBlock>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT name, content, version, max_tokens, created_at, updated_at
        FROM memory_blocks ORDER BY name
        "#,
    )?;
    let blocks: rusqlite::Result<Vec<MemoryBlock>> =
        stmt.query_map([], row_to_block)?.collect();
    Ok(blocks?)
}

/// Delete a block and its edit history.
pub fn delete_block(conn: &Connection, name: &str) -> Result<()> {
    let rows = conn.execute("DELETE FROM memory_blocks WHERE name = ?", params![name])?;
    if rows == 0 {
        return Err(EngramError::Storage(format!(
            "memory block '{}' not found",
            name
        )));
    }
    Ok(())
}

/// Retrieve the edit history for a block, most-recent first.
///
/// `limit` caps the number of entries returned; pass `0` for all entries.
pub fn get_block_history(
    conn: &Connection,
    name: &str,
    limit: usize,
) -> Result<Vec<BlockEditLog>> {
    let sql = if limit > 0 {
        format!(
            r#"
            SELECT id, block_name, old_content, new_content, edit_reason, timestamp
            FROM block_edit_log WHERE block_name = ?
            ORDER BY id DESC LIMIT {}
            "#,
            limit
        )
    } else {
        r#"
        SELECT id, block_name, old_content, new_content, edit_reason, timestamp
        FROM block_edit_log WHERE block_name = ?
        ORDER BY id DESC
        "#
        .to_string()
    };

    let mut stmt = conn.prepare(&sql)?;
    let entries: rusqlite::Result<Vec<BlockEditLog>> =
        stmt.query_map(params![name], row_to_log)?.collect();
    Ok(entries?)
}

/// If the block's content exceeds its `max_tokens` budget (estimated at
/// 4 chars/token), truncate the block to the allowed length, persist the
/// truncated version with a log entry, and return the overflow text.
///
/// Returns `None` when the block is within budget (no write is performed).
pub fn archive_overflow(conn: &Connection, name: &str) -> Result<Option<String>> {
    let block = get_block(conn, name)?.ok_or_else(|| {
        EngramError::Storage(format!("memory block '{}' not found", name))
    })?;

    let max_chars = block.max_tokens * 4;
    if block.content.len() <= max_chars {
        return Ok(None);
    }

    // Truncate at a char boundary.
    let keep = &block.content[..max_chars];
    let overflow = block.content[max_chars..].to_string();

    update_block(conn, name, keep, "overflow archived")?;

    Ok(Some(overflow))
}

// ─── Private helpers ─────────────────────────────────────────────────────────

fn get_block_required(conn: &Connection, name: &str) -> Result<MemoryBlock> {
    get_block(conn, name)?.ok_or_else(|| {
        EngramError::Storage(format!("memory block '{}' unexpectedly missing", name))
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(CREATE_MEMORY_BLOCKS_TABLE).unwrap();
        conn.execute_batch(CREATE_BLOCK_EDIT_LOG_TABLE).unwrap();
        conn
    }

    // 1. Create and get block
    #[test]
    fn test_create_and_get_block() {
        let conn = setup();
        let block = create_block(&conn, "persona", "I am a helpful assistant.", 512).unwrap();

        assert_eq!(block.name, "persona");
        assert_eq!(block.content, "I am a helpful assistant.");
        assert_eq!(block.version, 1);
        assert_eq!(block.max_tokens, 512);

        let fetched = get_block(&conn, "persona").unwrap().unwrap();
        assert_eq!(fetched.name, block.name);
        assert_eq!(fetched.content, block.content);
    }

    // 2. Update increments version
    #[test]
    fn test_update_increments_version() {
        let conn = setup();
        create_block(&conn, "notes", "initial", 256).unwrap();

        let v2 = update_block(&conn, "notes", "updated once", "first edit").unwrap();
        assert_eq!(v2.version, 2);

        let v3 = update_block(&conn, "notes", "updated twice", "second edit").unwrap();
        assert_eq!(v3.version, 3);
    }

    // 3. Update logs to edit history
    #[test]
    fn test_update_logs_edit_history() {
        let conn = setup();
        create_block(&conn, "context", "old text", 256).unwrap();
        update_block(&conn, "context", "new text", "test reason").unwrap();

        let history = get_block_history(&conn, "context", 0).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].old_content, "old text");
        assert_eq!(history[0].new_content, "new text");
        assert_eq!(history[0].edit_reason, "test reason");
    }

    // 4. List blocks returns all
    #[test]
    fn test_list_blocks_returns_all() {
        let conn = setup();
        create_block(&conn, "alpha", "a", 128).unwrap();
        create_block(&conn, "beta", "b", 128).unwrap();
        create_block(&conn, "gamma", "c", 128).unwrap();

        let blocks = list_blocks(&conn).unwrap();
        assert_eq!(blocks.len(), 3);
        // Ordered by name
        assert_eq!(blocks[0].name, "alpha");
        assert_eq!(blocks[1].name, "beta");
        assert_eq!(blocks[2].name, "gamma");
    }

    // 5. Delete block
    #[test]
    fn test_delete_block() {
        let conn = setup();
        create_block(&conn, "temp", "to be deleted", 64).unwrap();
        delete_block(&conn, "temp").unwrap();

        let result = get_block(&conn, "temp").unwrap();
        assert!(result.is_none());
    }

    // 6. Get history with limit
    #[test]
    fn test_get_history_with_limit() {
        let conn = setup();
        create_block(&conn, "log", "v1", 256).unwrap();
        update_block(&conn, "log", "v2", "edit 1").unwrap();
        update_block(&conn, "log", "v3", "edit 2").unwrap();
        update_block(&conn, "log", "v4", "edit 3").unwrap();

        let limited = get_block_history(&conn, "log", 2).unwrap();
        assert_eq!(limited.len(), 2);
        // Most-recent first
        assert_eq!(limited[0].new_content, "v4");
        assert_eq!(limited[1].new_content, "v3");
    }

    // 7. Archive overflow truncates and returns excess
    #[test]
    fn test_archive_overflow_truncates_and_returns_excess() {
        let conn = setup();
        // max_tokens = 2  →  max_chars = 8
        let long_content = "12345678overflow_part";
        create_block(&conn, "small", long_content, 2).unwrap();

        let overflow = archive_overflow(&conn, "small").unwrap();
        assert!(overflow.is_some());
        assert_eq!(overflow.unwrap(), "overflow_part");

        let block = get_block(&conn, "small").unwrap().unwrap();
        assert_eq!(block.content, "12345678");
    }

    // 8. Archive non-overflowing block returns None
    #[test]
    fn test_archive_non_overflowing_returns_none() {
        let conn = setup();
        create_block(&conn, "roomy", "short", 1024).unwrap();

        let result = archive_overflow(&conn, "roomy").unwrap();
        assert!(result.is_none());

        // Block content and version must not have changed
        let block = get_block(&conn, "roomy").unwrap().unwrap();
        assert_eq!(block.content, "short");
        assert_eq!(block.version, 1);
    }

    // 9. Get nonexistent returns None
    #[test]
    fn test_get_nonexistent_returns_none() {
        let conn = setup();
        let result = get_block(&conn, "does_not_exist").unwrap();
        assert!(result.is_none());
    }

    // Bonus: delete nonexistent returns error
    #[test]
    fn test_delete_nonexistent_returns_error() {
        let conn = setup();
        let result = delete_block(&conn, "ghost");
        assert!(result.is_err());
    }

    // Bonus: update nonexistent returns error
    #[test]
    fn test_update_nonexistent_returns_error() {
        let conn = setup();
        let result = update_block(&conn, "ghost", "content", "reason");
        assert!(result.is_err());
    }

    // Bonus: create with empty name returns error
    #[test]
    fn test_create_empty_name_returns_error() {
        let conn = setup();
        let result = create_block(&conn, "", "content", 256);
        assert!(result.is_err());
    }
}
