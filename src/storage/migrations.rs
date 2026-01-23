//! Database migrations for Engram

use rusqlite::Connection;

use crate::error::Result;

/// Current schema version
pub const SCHEMA_VERSION: i32 = 1;

/// Run all migrations
pub fn run_migrations(conn: &Connection) -> Result<()> {
    // Create migrations table if not exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    let current_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current_version < SCHEMA_VERSION {
        migrate_v1(conn)?;
    }

    Ok(())
}

/// Initial schema (v1)
fn migrate_v1(conn: &Connection) -> Result<()> {
    // Main memories table
    conn.execute_batch(
        r#"
        -- Memories table with full features
        CREATE TABLE IF NOT EXISTS memories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            content TEXT NOT NULL,
            memory_type TEXT NOT NULL DEFAULT 'note',
            importance REAL NOT NULL DEFAULT 0.5,
            access_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            last_accessed_at TEXT,
            owner_id TEXT,
            visibility TEXT NOT NULL DEFAULT 'private',
            version INTEGER NOT NULL DEFAULT 1,
            has_embedding INTEGER NOT NULL DEFAULT 0,
            embedding_queued_at TEXT,
            valid_from TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            valid_to TEXT,
            metadata TEXT NOT NULL DEFAULT '{}'
        );

        -- Tags table (normalized)
        CREATE TABLE IF NOT EXISTS tags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE COLLATE NOCASE
        );

        -- Memory-tag relationship
        CREATE TABLE IF NOT EXISTS memory_tags (
            memory_id INTEGER NOT NULL,
            tag_id INTEGER NOT NULL,
            PRIMARY KEY (memory_id, tag_id),
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE,
            FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
        );

        -- Cross-references with rich metadata (RML-901)
        CREATE TABLE IF NOT EXISTS crossrefs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id INTEGER NOT NULL,
            to_id INTEGER NOT NULL,
            edge_type TEXT NOT NULL DEFAULT 'related_to',
            score REAL NOT NULL,
            confidence REAL NOT NULL DEFAULT 1.0,
            strength REAL NOT NULL DEFAULT 1.0,
            source TEXT NOT NULL DEFAULT 'auto',
            source_context TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            valid_from TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            valid_to TEXT,
            pinned INTEGER NOT NULL DEFAULT 0,
            metadata TEXT NOT NULL DEFAULT '{}',
            UNIQUE(from_id, to_id, edge_type),
            FOREIGN KEY (from_id) REFERENCES memories(id) ON DELETE CASCADE,
            FOREIGN KEY (to_id) REFERENCES memories(id) ON DELETE CASCADE
        );

        -- Memory versions for history (RML-889)
        CREATE TABLE IF NOT EXISTS memory_versions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            memory_id INTEGER NOT NULL,
            version INTEGER NOT NULL,
            content TEXT NOT NULL,
            tags TEXT NOT NULL DEFAULT '[]',
            metadata TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            created_by TEXT,
            change_summary TEXT,
            UNIQUE(memory_id, version),
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
        );

        -- Embeddings table (separate for flexibility)
        CREATE TABLE IF NOT EXISTS embeddings (
            memory_id INTEGER PRIMARY KEY,
            embedding BLOB NOT NULL,
            model TEXT NOT NULL,
            dimensions INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
        );

        -- Embedding queue for async processing (RML-873)
        CREATE TABLE IF NOT EXISTS embedding_queue (
            memory_id INTEGER PRIMARY KEY,
            status TEXT NOT NULL DEFAULT 'pending',
            queued_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            started_at TEXT,
            completed_at TEXT,
            error TEXT,
            retry_count INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
        );

        -- Audit log (RML-884)
        CREATE TABLE IF NOT EXISTS audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            user_id TEXT,
            action TEXT NOT NULL,
            memory_id INTEGER,
            changes TEXT,
            ip_address TEXT
        );

        -- Sync tracking
        CREATE TABLE IF NOT EXISTS sync_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            last_sync TEXT,
            pending_changes INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            is_syncing INTEGER NOT NULL DEFAULT 0
        );

        -- Initialize sync state
        INSERT OR IGNORE INTO sync_state (id, pending_changes) VALUES (1, 0);

        -- Full-text search with BM25 (RML-876)
        CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
            content,
            tags,
            metadata,
            content='memories',
            content_rowid='id',
            tokenize='porter unicode61'
        );

        -- Triggers to keep FTS in sync
        CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
            INSERT INTO memories_fts(rowid, content, tags, metadata)
            SELECT NEW.id, NEW.content, '', NEW.metadata;
        END;

        CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, content, tags, metadata)
            VALUES('delete', OLD.id, OLD.content, '', OLD.metadata);
        END;

        CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, content, tags, metadata)
            VALUES('delete', OLD.id, OLD.content, '', OLD.metadata);
            INSERT INTO memories_fts(rowid, content, tags, metadata)
            SELECT NEW.id, NEW.content, '', NEW.metadata;
        END;

        -- Indexes for performance
        CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
        CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_memories_updated ON memories(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance DESC);
        CREATE INDEX IF NOT EXISTS idx_memories_owner ON memories(owner_id);
        CREATE INDEX IF NOT EXISTS idx_memories_visibility ON memories(visibility);
        CREATE INDEX IF NOT EXISTS idx_memories_valid ON memories(valid_from, valid_to);

        CREATE INDEX IF NOT EXISTS idx_memory_tags_memory ON memory_tags(memory_id);
        CREATE INDEX IF NOT EXISTS idx_memory_tags_tag ON memory_tags(tag_id);

        CREATE INDEX IF NOT EXISTS idx_crossrefs_from ON crossrefs(from_id);
        CREATE INDEX IF NOT EXISTS idx_crossrefs_to ON crossrefs(to_id);
        CREATE INDEX IF NOT EXISTS idx_crossrefs_type ON crossrefs(edge_type);
        CREATE INDEX IF NOT EXISTS idx_crossrefs_valid ON crossrefs(valid_from, valid_to);

        CREATE INDEX IF NOT EXISTS idx_versions_memory ON memory_versions(memory_id);
        CREATE INDEX IF NOT EXISTS idx_audit_memory ON audit_log(memory_id);
        CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_audit_user ON audit_log(user_id);

        CREATE INDEX IF NOT EXISTS idx_embedding_queue_status ON embedding_queue(status);

        -- Record migration
        INSERT INTO schema_version (version) VALUES (1);
        "#,
    )?;

    Ok(())
}
