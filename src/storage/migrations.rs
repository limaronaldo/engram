//! Database migrations for Engram

use rusqlite::Connection;

use crate::error::Result;

/// Current schema version
pub const SCHEMA_VERSION: i32 = 11;

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

    if current_version < 1 {
        migrate_v1(conn)?;
    }

    if current_version < 2 {
        migrate_v2(conn)?;
    }

    if current_version < 3 {
        migrate_v3(conn)?;
    }

    if current_version < 4 {
        migrate_v4(conn)?;
    }

    if current_version < 5 {
        migrate_v5(conn)?;
    }

    if current_version < 6 {
        migrate_v6(conn)?;
    }

    if current_version < 7 {
        migrate_v7(conn)?;
    }

    if current_version < 8 {
        migrate_v8(conn)?;
    }

    if current_version < 9 {
        migrate_v9(conn)?;
    }

    if current_version < 10 {
        migrate_v10(conn)?;
    }

    if current_version < SCHEMA_VERSION {
        migrate_v11(conn)?;
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

/// Memory scoping migration (v2) - RML-924
/// Adds scope_type and scope_id columns for user/session/agent/global isolation
fn migrate_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Add scope columns to memories table
        -- scope_type: 'user', 'session', 'agent', 'global'
        -- scope_id: the actual ID (user_id, session_id, agent_id) or NULL for global
        ALTER TABLE memories ADD COLUMN scope_type TEXT NOT NULL DEFAULT 'global';
        ALTER TABLE memories ADD COLUMN scope_id TEXT;

        -- Index for efficient scope filtering
        CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope_type, scope_id);

        -- Composite index for common query patterns (scope + type + created)
        CREATE INDEX IF NOT EXISTS idx_memories_scope_type_created
            ON memories(scope_type, scope_id, memory_type, created_at DESC);

        -- Record migration
        INSERT INTO schema_version (version) VALUES (2);
        "#,
    )?;

    Ok(())
}

/// Entity extraction migration (v3) - RML-925
/// Adds tables for storing extracted entities and their relationships to memories
fn migrate_v3(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Entities table for storing extracted named entities
        CREATE TABLE IF NOT EXISTS entities (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            normalized_name TEXT NOT NULL,
            entity_type TEXT NOT NULL,  -- person, organization, project, concept, location, datetime, reference, other
            aliases TEXT NOT NULL DEFAULT '[]',  -- JSON array of alternative names
            metadata TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            mention_count INTEGER NOT NULL DEFAULT 1,
            UNIQUE(normalized_name, entity_type)
        );

        -- Memory-entity relationships
        CREATE TABLE IF NOT EXISTS memory_entities (
            memory_id INTEGER NOT NULL,
            entity_id INTEGER NOT NULL,
            relation TEXT NOT NULL DEFAULT 'mentions',  -- mentions, defines, references, about, created_by
            confidence REAL NOT NULL DEFAULT 1.0,
            char_offset INTEGER,  -- position in memory content
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (memory_id, entity_id, relation),
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE,
            FOREIGN KEY (entity_id) REFERENCES entities(id) ON DELETE CASCADE
        );

        -- Indexes for entity queries
        CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);
        CREATE INDEX IF NOT EXISTS idx_entities_normalized ON entities(normalized_name);
        CREATE INDEX IF NOT EXISTS idx_entities_mention_count ON entities(mention_count DESC);

        CREATE INDEX IF NOT EXISTS idx_memory_entities_memory ON memory_entities(memory_id);
        CREATE INDEX IF NOT EXISTS idx_memory_entities_entity ON memory_entities(entity_id);
        CREATE INDEX IF NOT EXISTS idx_memory_entities_relation ON memory_entities(relation);

        -- Record migration
        INSERT INTO schema_version (version) VALUES (3);
        "#,
    )?;

    Ok(())
}

/// Recompute entity mention counts (v4)
fn migrate_v4(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Recalculate mention_count from existing links
        UPDATE entities
        SET mention_count = (
            SELECT COUNT(DISTINCT memory_id)
            FROM memory_entities
            WHERE memory_entities.entity_id = entities.id
        );

        -- Record migration
        INSERT INTO schema_version (version) VALUES (4);
        "#,
    )?;

    Ok(())
}

/// Memory expiration (TTL) migration (v5) - RML-930
/// Adds expires_at column for automatic memory expiration
fn migrate_v5(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Add expires_at column to memories table
        -- NULL = never expires (default)
        ALTER TABLE memories ADD COLUMN expires_at TEXT;

        -- Index for efficient expired memory queries and cleanup
        CREATE INDEX IF NOT EXISTS idx_memories_expires_at ON memories(expires_at)
            WHERE expires_at IS NOT NULL;

        -- Record migration
        INSERT INTO schema_version (version) VALUES (5);
        "#,
    )?;

    Ok(())
}

/// Memory deduplication migration (v6) - RML-931
/// Adds content_hash column for duplicate detection and backfills existing memories
fn migrate_v6(conn: &Connection) -> Result<()> {
    use sha2::{Digest, Sha256};

    // Step 1: Add the column and index
    conn.execute_batch(
        r#"
        -- Add content_hash column to memories table for deduplication
        -- SHA256 hash of normalized content
        ALTER TABLE memories ADD COLUMN content_hash TEXT;

        -- Index for fast exact duplicate detection (scoped)
        -- Not UNIQUE because we want to allow duplicates with 'allow' mode
        CREATE INDEX IF NOT EXISTS idx_memories_content_hash ON memories(content_hash, scope_type, scope_id)
            WHERE content_hash IS NOT NULL;

        -- Record migration
        INSERT INTO schema_version (version) VALUES (6);
        "#,
    )?;

    // Step 2: Backfill content_hash for existing memories
    // We compute the hash in Rust since SQLite doesn't have SHA256 built-in
    let mut stmt = conn.prepare("SELECT id, content FROM memories WHERE content_hash IS NULL")?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut update_stmt = conn.prepare("UPDATE memories SET content_hash = ? WHERE id = ?")?;

    for (id, content) in rows {
        // Normalize: lowercase + collapse whitespace
        let normalized = content
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let mut hasher = Sha256::new();
        hasher.update(normalized.as_bytes());
        let hash = format!("sha256:{}", hex::encode(hasher.finalize()));

        update_stmt.execute(rusqlite::params![hash, id])?;
    }

    tracing::info!("Migration v6: Backfilled content_hash for existing memories");

    Ok(())
}

/// Workspace and tier migration (v7) - RML-950
/// Adds workspace column for project isolation and tier column for memory tiering
fn migrate_v7(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v7: Adding workspace + tier columns...");

    conn.execute_batch(
        r#"
        -- Add workspace column for project-based memory isolation
        -- Normalized at application level: lowercase, [a-z0-9_-], max 64 chars
        -- Default 'default' for backward compatibility
        ALTER TABLE memories ADD COLUMN workspace TEXT NOT NULL DEFAULT 'default';

        -- Add tier column for memory tiering (permanent vs daily)
        -- 'permanent' = never expires (default)
        -- 'daily' = auto-expires, requires expires_at to be set
        ALTER TABLE memories ADD COLUMN tier TEXT NOT NULL DEFAULT 'permanent';

        -- Composite index for workspace filtering (most common query pattern)
        -- Covers: list by workspace, search by workspace + time
        CREATE INDEX IF NOT EXISTS idx_memories_workspace_created
            ON memories(workspace, created_at DESC);

        -- Composite index for workspace + scope (multi-tenant queries)
        CREATE INDEX IF NOT EXISTS idx_memories_workspace_scope
            ON memories(workspace, scope_type, scope_id);

        -- Record migration
        INSERT INTO schema_version (version) VALUES (7);
        "#,
    )?;

    tracing::info!("Migration v7 complete: workspace + tier columns added");

    Ok(())
}

/// Session transcript indexing migration (v8)
/// Adds tables for storing conversation sessions and their indexed chunks
fn migrate_v8(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v8: Adding session transcript indexing tables...");

    conn.execute_batch(
        r#"
        -- Sessions table for tracking indexed conversations
        CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL UNIQUE,
            title TEXT,
            agent_id TEXT,
            started_at TEXT NOT NULL,
            last_indexed_at TEXT,
            message_count INTEGER NOT NULL DEFAULT 0,
            chunk_count INTEGER NOT NULL DEFAULT 0,
            workspace TEXT NOT NULL DEFAULT 'default',
            metadata TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        -- Session chunks table linking sessions to memory chunks
        CREATE TABLE IF NOT EXISTS session_chunks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL,
            memory_id INTEGER NOT NULL,
            chunk_index INTEGER NOT NULL,
            start_message_index INTEGER NOT NULL,
            end_message_index INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE,
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE,
            UNIQUE(session_id, chunk_index)
        );

        -- Index for finding chunks by session
        CREATE INDEX IF NOT EXISTS idx_session_chunks_session
            ON session_chunks(session_id, chunk_index);

        -- Index for finding chunks by memory
        CREATE INDEX IF NOT EXISTS idx_session_chunks_memory
            ON session_chunks(memory_id);

        -- Index for sessions by workspace
        CREATE INDEX IF NOT EXISTS idx_sessions_workspace
            ON sessions(workspace, started_at DESC);

        -- Record migration
        INSERT INTO schema_version (version) VALUES (8);
        "#,
    )?;

    tracing::info!("Migration v8 complete: session transcript indexing tables added");

    Ok(())
}

/// Identity links migration (v9)
/// Adds tables for entity unification through canonical identities and aliases
fn migrate_v9(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v9: Adding identity links tables...");

    conn.execute_batch(
        r#"
        -- Identities table for canonical identity management
        -- Each identity represents a unique entity that may have multiple aliases
        CREATE TABLE IF NOT EXISTS identities (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            canonical_id TEXT NOT NULL UNIQUE,
            display_name TEXT NOT NULL,
            entity_type TEXT NOT NULL DEFAULT 'person',
            description TEXT,
            metadata TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        -- Identity aliases table for mapping various names to canonical identities
        -- Aliases are normalized at the application level (lowercase, trimmed, collapsed whitespace)
        CREATE TABLE IF NOT EXISTS identity_aliases (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            canonical_id TEXT NOT NULL,
            alias TEXT NOT NULL,
            alias_normalized TEXT NOT NULL,
            source TEXT,
            confidence REAL NOT NULL DEFAULT 1.0,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (canonical_id) REFERENCES identities(canonical_id) ON DELETE CASCADE,
            UNIQUE(alias_normalized)
        );

        -- Memory-identity links for tracking which identities are mentioned in memories
        CREATE TABLE IF NOT EXISTS memory_identity_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            memory_id INTEGER NOT NULL,
            canonical_id TEXT NOT NULL,
            mention_text TEXT,
            mention_count INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE,
            FOREIGN KEY (canonical_id) REFERENCES identities(canonical_id) ON DELETE CASCADE,
            UNIQUE(memory_id, canonical_id)
        );

        -- Index for finding aliases by normalized form (primary lookup)
        CREATE INDEX IF NOT EXISTS idx_identity_aliases_normalized
            ON identity_aliases(alias_normalized);

        -- Index for finding all aliases for an identity
        CREATE INDEX IF NOT EXISTS idx_identity_aliases_canonical
            ON identity_aliases(canonical_id);

        -- Index for finding identities by type
        CREATE INDEX IF NOT EXISTS idx_identities_type
            ON identities(entity_type);

        -- Index for finding all memories mentioning an identity
        CREATE INDEX IF NOT EXISTS idx_memory_identity_links_canonical
            ON memory_identity_links(canonical_id);

        -- Index for finding all identities in a memory
        CREATE INDEX IF NOT EXISTS idx_memory_identity_links_memory
            ON memory_identity_links(memory_id);

        -- Record migration
        INSERT INTO schema_version (version) VALUES (9);
        "#,
    )?;

    tracing::info!("Migration v9 complete: identity links tables added");

    Ok(())
}

/// Events and sharing migration (v10)
/// Adds tables for real-time events and multi-agent memory sharing
fn migrate_v10(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v10: Adding events and sharing tables...");

    conn.execute_batch(
        r#"
        -- Memory events table for real-time change notifications
        -- Enables event-driven sync between agents/clients
        CREATE TABLE IF NOT EXISTS memory_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_type TEXT NOT NULL,
            memory_id INTEGER,
            agent_id TEXT,
            data TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            processed INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE SET NULL
        );

        -- Index for polling unprocessed events
        CREATE INDEX IF NOT EXISTS idx_memory_events_unprocessed
            ON memory_events(processed, created_at);

        -- Index for filtering events by type
        CREATE INDEX IF NOT EXISTS idx_memory_events_type
            ON memory_events(event_type, created_at DESC);

        -- Index for filtering events by agent
        CREATE INDEX IF NOT EXISTS idx_memory_events_agent
            ON memory_events(agent_id, created_at DESC);

        -- Shared memories table for multi-agent communication
        -- Allows one agent to share memories with others
        CREATE TABLE IF NOT EXISTS shared_memories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            memory_id INTEGER NOT NULL,
            from_agent TEXT NOT NULL,
            to_agent TEXT,
            message TEXT,
            acknowledged INTEGER NOT NULL DEFAULT 0,
            acknowledged_at TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            expires_at TEXT,
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
        );

        -- Index for polling shared memories by recipient
        CREATE INDEX IF NOT EXISTS idx_shared_memories_recipient
            ON shared_memories(to_agent, acknowledged, created_at DESC);

        -- Index for finding shares from a specific agent
        CREATE INDEX IF NOT EXISTS idx_shared_memories_sender
            ON shared_memories(from_agent, created_at DESC);

        -- Sync state per agent for delta sync
        CREATE TABLE IF NOT EXISTS agent_sync_state (
            agent_id TEXT PRIMARY KEY,
            last_sync_version INTEGER NOT NULL DEFAULT 0,
            last_sync_at TEXT,
            sync_metadata TEXT NOT NULL DEFAULT '{}'
        );

        -- Add sync version to memories for delta sync
        -- Each write increments a global counter stored in sync_state
        ALTER TABLE sync_state ADD COLUMN version INTEGER NOT NULL DEFAULT 0;

        -- Record migration
        INSERT INTO schema_version (version) VALUES (10);
        "#,
    )?;

    tracing::info!("Migration v10 complete: events and sharing tables added");

    Ok(())
}

/// Migration v11: Add agent_id column to memory_events for existing v10 databases
///
/// This migration handles the case where v10 was applied before agent_id was added
/// to the memory_events table schema. It adds the column and index if missing.
fn migrate_v11(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v11: Adding agent_id to memory_events if missing...");

    // Check if agent_id column already exists
    let has_agent_id: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('memory_events') WHERE name = 'agent_id'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !has_agent_id {
        // Add agent_id column to memory_events
        conn.execute("ALTER TABLE memory_events ADD COLUMN agent_id TEXT", [])?;

        tracing::info!("  ✓ Added agent_id column to memory_events");

        // Add index for agent_id queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_events_agent ON memory_events(agent_id, created_at DESC)",
            [],
        )?;

        tracing::info!("  ✓ Created idx_memory_events_agent index");
    } else {
        tracing::info!("  ✓ agent_id column already exists, skipping ALTER");
    }

    // Record migration
    conn.execute("INSERT INTO schema_version (version) VALUES (11)", [])?;

    tracing::info!("Migration v11 complete: memory_events.agent_id ensured");

    Ok(())
}
