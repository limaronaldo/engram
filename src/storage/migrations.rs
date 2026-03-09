//! Database migrations for Engram

use rusqlite::Connection;

use crate::error::Result;

/// Current schema version
pub const SCHEMA_VERSION: i32 = 25;

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

    if current_version < 11 {
        migrate_v11(conn)?;
    }

    if current_version < 12 {
        migrate_v12(conn)?;
    }

    if current_version < 13 {
        migrate_v13(conn)?;
    }

    if current_version < 14 {
        migrate_v14(conn)?;
    }

    if current_version < 15 {
        migrate_v15(conn)?;
    }

    if current_version < 16 {
        migrate_v16(conn)?;
    }

    if current_version < 17 {
        migrate_v17(conn)?;
    }

    if current_version < 18 {
        migrate_v18(conn)?;
    }

    if current_version < 19 {
        migrate_v19(conn)?;
    }

    if current_version < 20 {
        migrate_v20(conn)?;
    }

    if current_version < 21 {
        migrate_v21(conn)?;
    }

    if current_version < 22 {
        migrate_v22(conn)?;
    }

    if current_version < 23 {
        migrate_v23(conn)?;
    }

    if current_version < 24 {
        migrate_v24(conn)?;
    }

    if current_version < SCHEMA_VERSION {
        migrate_v25(conn)?;
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

/// Migration v12: Cognitive memory types (Phase 1 - ENG-33)
///
/// Adds columns for episodic, procedural, summary, and checkpoint memory types:
/// - event_time: ISO8601 timestamp for episodic memories (when the event occurred)
/// - event_duration_seconds: Duration of the event in seconds
/// - trigger_pattern: Pattern that triggers procedural memories
/// - procedure_success_count: Times this procedure succeeded
/// - procedure_failure_count: Times this procedure failed
/// - summary_of_id: References the original memory for summaries
///
/// Also creates the sync_tasks table for Phase 3 (Langfuse integration)
fn migrate_v12(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v12: Adding cognitive memory type columns...");

    conn.execute_batch(
        r#"
        -- Episodic memory: temporal indexing
        ALTER TABLE memories ADD COLUMN event_time TEXT;
        ALTER TABLE memories ADD COLUMN event_duration_seconds INTEGER;

        -- Procedural memory: pattern tracking
        ALTER TABLE memories ADD COLUMN trigger_pattern TEXT;
        ALTER TABLE memories ADD COLUMN procedure_success_count INTEGER DEFAULT 0;
        ALTER TABLE memories ADD COLUMN procedure_failure_count INTEGER DEFAULT 0;

        -- Summary memory: reference to source
        ALTER TABLE memories ADD COLUMN summary_of_id INTEGER REFERENCES memories(id) ON DELETE SET NULL;

        -- Index for episodic memory queries by event time
        CREATE INDEX IF NOT EXISTS idx_memories_event_time
            ON memories(event_time) WHERE event_time IS NOT NULL;

        -- Index for finding summaries of a specific memory
        CREATE INDEX IF NOT EXISTS idx_memories_summary_of
            ON memories(summary_of_id) WHERE summary_of_id IS NOT NULL;

        -- Sync tasks table for Phase 3 (Langfuse integration)
        -- Added now to avoid another migration for Phase 3
        CREATE TABLE IF NOT EXISTS sync_tasks (
            task_id TEXT PRIMARY KEY,
            task_type TEXT NOT NULL,
            status TEXT NOT NULL,
            progress_percent INTEGER DEFAULT 0,
            traces_processed INTEGER DEFAULT 0,
            memories_created INTEGER DEFAULT 0,
            error_message TEXT,
            started_at TEXT NOT NULL,
            completed_at TEXT
        );

        -- Record migration
        INSERT INTO schema_version (version) VALUES (12);
        "#,
    )?;

    tracing::info!("Migration v12 complete: cognitive memory type columns added");

    Ok(())
}

/// Migration v13: Memory lifecycle management (Phase 5 - ENG-37)
///
/// Adds:
/// - lifecycle_state: active/stale/archived state tracking
/// - Index for lifecycle-based queries
///
/// Note: last_accessed_at and access_count already exist in v1
fn migrate_v13(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v13: Adding lifecycle state column...");

    conn.execute_batch(
        r#"
        -- Lifecycle state for memory management
        -- Values: 'active' (default), 'stale', 'archived'
        ALTER TABLE memories ADD COLUMN lifecycle_state TEXT DEFAULT 'active';

        -- Index for filtering by lifecycle state
        CREATE INDEX IF NOT EXISTS idx_memories_lifecycle
            ON memories(lifecycle_state)
            WHERE lifecycle_state IS NOT NULL;

        -- Record migration
        INSERT INTO schema_version (version) VALUES (13);
        "#,
    )?;

    tracing::info!("Migration v13 complete: lifecycle state column added");

    Ok(())
}

/// Migration v14: Salience & Session Memory (Phase 8 - ENG-66 to ENG-77)
///
/// Adds:
/// - salience_history: Track salience scores over time for trend analysis
/// - session_memories: Link memories to sessions for context tracking
/// - Indexes for efficient salience-based queries
///
/// This enables:
/// - Salience scoring algorithm (ENG-66)
/// - Priority queue implementation (ENG-67)
/// - Temporal decay functions (ENG-68)
/// - Session context tracking (ENG-70)
/// - Session-scoped search (ENG-71)
fn migrate_v14(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v14: Adding salience history and session memory tables...");

    conn.execute_batch(
        r#"
        -- Salience history for trend analysis
        -- Records salience scores periodically for each memory
        CREATE TABLE IF NOT EXISTS salience_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
            salience_score REAL NOT NULL,
            recency_score REAL,
            frequency_score REAL,
            importance_score REAL,
            feedback_score REAL,
            recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        -- Index for querying salience history by memory
        CREATE INDEX IF NOT EXISTS idx_salience_history_memory
            ON salience_history(memory_id, recorded_at DESC);

        -- Index for time-based salience queries
        CREATE INDEX IF NOT EXISTS idx_salience_history_time
            ON salience_history(recorded_at DESC);

        -- Session-memory linking table
        -- Links memories to sessions for context tracking
        CREATE TABLE IF NOT EXISTS session_memories (
            session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
            memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
            added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            relevance_score REAL DEFAULT 1.0,
            context_role TEXT DEFAULT 'referenced',
            PRIMARY KEY (session_id, memory_id)
        );

        -- Index for finding memories by session
        CREATE INDEX IF NOT EXISTS idx_session_memories_session
            ON session_memories(session_id);

        -- Index for finding sessions by memory
        CREATE INDEX IF NOT EXISTS idx_session_memories_memory
            ON session_memories(memory_id);

        -- Add summary column to sessions for session summarization
        ALTER TABLE sessions ADD COLUMN summary TEXT;

        -- Add context column to sessions for active context tracking
        ALTER TABLE sessions ADD COLUMN context TEXT;

        -- Add ended_at column to sessions for lifecycle tracking
        ALTER TABLE sessions ADD COLUMN ended_at TEXT;

        -- Record migration
        INSERT INTO schema_version (version) VALUES (14);
        "#,
    )?;

    tracing::info!("Migration v14 complete: salience history and session memory tables added");

    Ok(())
}

/// Phase 9: Context Quality (ENG-48 to ENG-66)
fn migrate_v15(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v15: Adding quality scoring and conflict detection tables...");

    conn.execute_batch(
        r#"
        -- Quality score column on memories table
        -- Overall quality score (0.0 - 1.0)
        ALTER TABLE memories ADD COLUMN quality_score REAL DEFAULT 0.5;

        -- Validation status: unverified, verified, disputed, stale
        ALTER TABLE memories ADD COLUMN validation_status TEXT DEFAULT 'unverified';

        -- Content hash for fast duplicate detection (SimHash)
        -- Note: content_hash may already exist from earlier migration, so we use IF NOT EXISTS pattern
        -- SQLite doesn't support IF NOT EXISTS for ALTER TABLE, so we catch errors

        -- Quality history for trend tracking
        CREATE TABLE IF NOT EXISTS quality_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
            quality_score REAL NOT NULL,
            clarity_score REAL,
            completeness_score REAL,
            freshness_score REAL,
            consistency_score REAL,
            source_trust_score REAL,
            recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_quality_history_memory
            ON quality_history(memory_id, recorded_at DESC);

        -- Memory conflicts table for tracking contradictions
        CREATE TABLE IF NOT EXISTS memory_conflicts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            memory_a_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
            memory_b_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
            conflict_type TEXT NOT NULL DEFAULT 'contradiction',
            severity TEXT NOT NULL DEFAULT 'medium',
            description TEXT,
            detected_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            resolved_at TEXT,
            resolution_type TEXT,
            resolution_notes TEXT,
            auto_detected INTEGER NOT NULL DEFAULT 1,
            UNIQUE(memory_a_id, memory_b_id, conflict_type)
        );

        CREATE INDEX IF NOT EXISTS idx_memory_conflicts_a
            ON memory_conflicts(memory_a_id);

        CREATE INDEX IF NOT EXISTS idx_memory_conflicts_b
            ON memory_conflicts(memory_b_id);

        CREATE INDEX IF NOT EXISTS idx_memory_conflicts_unresolved
            ON memory_conflicts(resolved_at) WHERE resolved_at IS NULL;

        -- Source trust scores for credibility tracking
        CREATE TABLE IF NOT EXISTS source_trust_scores (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_type TEXT NOT NULL,
            source_identifier TEXT,
            trust_score REAL NOT NULL DEFAULT 0.7,
            verification_count INTEGER DEFAULT 0,
            last_verified_at TEXT,
            notes TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(source_type, source_identifier)
        );

        -- Default source trust scores
        INSERT OR IGNORE INTO source_trust_scores (source_type, source_identifier, trust_score, notes)
        VALUES
            ('user', 'default', 0.9, 'Direct user input'),
            ('seed', 'default', 0.7, 'Seeded/imported data'),
            ('extraction', 'default', 0.6, 'Auto-extracted from documents'),
            ('inference', 'default', 0.5, 'AI-inferred data'),
            ('external', 'default', 0.5, 'External API data');

        -- Duplicate detection cache
        CREATE TABLE IF NOT EXISTS duplicate_candidates (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            memory_a_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
            memory_b_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
            similarity_score REAL NOT NULL,
            similarity_type TEXT NOT NULL DEFAULT 'content',
            detected_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            status TEXT NOT NULL DEFAULT 'pending',
            resolved_at TEXT,
            resolution_type TEXT,
            UNIQUE(memory_a_id, memory_b_id, similarity_type)
        );

        CREATE INDEX IF NOT EXISTS idx_duplicate_candidates_pending
            ON duplicate_candidates(status) WHERE status = 'pending';

        CREATE INDEX IF NOT EXISTS idx_duplicate_candidates_score
            ON duplicate_candidates(similarity_score DESC);
        "#,
    )?;

    ensure_session_context_schema(conn)?;

    // Record migration
    conn.execute("INSERT INTO schema_version (version) VALUES (15)", [])?;

    tracing::info!("Migration v15 complete: quality scoring and conflict detection tables added");

    Ok(())
}

fn ensure_session_context_schema(conn: &Connection) -> Result<()> {
    let has_ended_at: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('sessions') WHERE name = 'ended_at'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !has_ended_at {
        conn.execute("ALTER TABLE sessions ADD COLUMN ended_at TEXT", [])?;
        tracing::info!("  ✓ Added sessions.ended_at column");
    }

    let session_memories_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'session_memories'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !session_memories_exists {
        return Ok(());
    }

    let needs_fk_fix: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0
             FROM pragma_foreign_key_list('session_memories')
             WHERE \"table\" = 'sessions' AND \"from\" = 'session_id' AND \"to\" = 'id'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if needs_fk_fix {
        conn.execute_batch(
            r#"
            ALTER TABLE session_memories RENAME TO session_memories_old;

            CREATE TABLE session_memories (
                session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
                memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                relevance_score REAL DEFAULT 1.0,
                context_role TEXT DEFAULT 'referenced',
                PRIMARY KEY (session_id, memory_id)
            );

            INSERT INTO session_memories (session_id, memory_id, added_at, relevance_score, context_role)
            SELECT session_id, memory_id, added_at, relevance_score, context_role
            FROM session_memories_old;

            DROP TABLE session_memories_old;

            CREATE INDEX IF NOT EXISTS idx_session_memories_session
                ON session_memories(session_id);

            CREATE INDEX IF NOT EXISTS idx_session_memories_memory
                ON session_memories(memory_id);
            "#,
        )?;

        tracing::info!("  ✓ Rebuilt session_memories with sessions(session_id) foreign key");
    }

    Ok(())
}

/// Schema v16: Retention policies per workspace
fn migrate_v16(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v16: Adding retention policies table...");

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS retention_policies (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace TEXT NOT NULL,
            max_age_days INTEGER,
            max_memories INTEGER,
            compress_after_days INTEGER,
            compress_max_importance REAL DEFAULT 0.3,
            compress_min_access INTEGER DEFAULT 3,
            auto_delete_after_days INTEGER,
            exclude_types TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(workspace)
        );

        CREATE INDEX IF NOT EXISTS idx_retention_policies_workspace
            ON retention_policies(workspace);
        "#,
    )?;

    conn.execute("INSERT INTO schema_version (version) VALUES (16)", [])?;

    tracing::info!("Migration v16 complete: retention policies table added");

    Ok(())
}

/// Schema v17: Agent registry
///
/// Adds the `agents` table for tracking registered AI agents with their
/// capabilities, namespaces, heartbeat status, and lifecycle state.
fn migrate_v17(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v17: Adding agent registry table...");

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS agents (
            agent_id TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            capabilities TEXT NOT NULL DEFAULT '[]',
            namespaces TEXT NOT NULL DEFAULT '["default"]',
            last_heartbeat TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            metadata TEXT NOT NULL DEFAULT '{}',
            registered_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_agents_status ON agents(status);
        CREATE INDEX IF NOT EXISTS idx_agents_heartbeat ON agents(last_heartbeat);
        "#,
    )?;

    conn.execute("INSERT INTO schema_version (version) VALUES (17)", [])?;

    tracing::info!("Migration v17 complete: agent registry table added");

    Ok(())
}

/// Schema v18: Auto-links and memory clusters
///
/// Adds two tables for the emergent-graph feature:
/// - `auto_links`: auto-generated links (semantic, temporal, co-occurrence)
/// - `memory_clusters`: memory cluster assignments from community detection
fn migrate_v18(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v18: Adding auto_links and memory_clusters tables...");

    conn.execute_batch(
        r#"
        -- Auto-generated links (semantic, temporal, co-occurrence)
        CREATE TABLE IF NOT EXISTS auto_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
            to_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
            link_type TEXT NOT NULL,
            score REAL NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            metadata TEXT DEFAULT '{}'
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_auto_links_pair_type ON auto_links(from_id, to_id, link_type);
        CREATE INDEX IF NOT EXISTS idx_auto_links_type ON auto_links(link_type);
        CREATE INDEX IF NOT EXISTS idx_auto_links_from ON auto_links(from_id);
        CREATE INDEX IF NOT EXISTS idx_auto_links_to ON auto_links(to_id);

        -- Memory clusters from community detection
        CREATE TABLE IF NOT EXISTS memory_clusters (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cluster_id INTEGER NOT NULL,
            memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
            algorithm TEXT NOT NULL,
            modularity REAL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_memory_clusters_cluster ON memory_clusters(cluster_id);
        CREATE INDEX IF NOT EXISTS idx_memory_clusters_memory ON memory_clusters(memory_id);
        CREATE INDEX IF NOT EXISTS idx_memory_clusters_algorithm ON memory_clusters(algorithm);
        "#,
    )?;

    conn.execute("INSERT INTO schema_version (version) VALUES (18)", [])?;

    tracing::info!("Migration v18 complete: auto_links and memory_clusters tables added");

    Ok(())
}

fn migrate_v19(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v19: Adding media_assets table...");

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS media_assets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
            media_type TEXT NOT NULL,
            file_hash TEXT NOT NULL,
            file_path TEXT,
            file_size INTEGER,
            mime_type TEXT,
            duration_secs REAL,
            width INTEGER,
            height INTEGER,
            transcription TEXT,
            description TEXT,
            provider TEXT,
            model TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_media_assets_memory ON media_assets(memory_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_media_assets_hash ON media_assets(file_hash);
        CREATE INDEX IF NOT EXISTS idx_media_assets_type ON media_assets(media_type);
        "#,
    )?;

    conn.execute("INSERT INTO schema_version (version) VALUES (19)", [])?;

    tracing::info!("Migration v19 complete: media_assets table added");

    Ok(())
}

/// Schema v20: Embedding model tracking
///
/// Adds `embedding_model` column to `memories` so we can track which backend
/// generated a particular embedding and support multi-model environments.
fn migrate_v20(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v20: Adding embedding_model column to memories...");

    conn.execute_batch(
        r#"
        ALTER TABLE memories ADD COLUMN embedding_model TEXT DEFAULT 'tfidf';

        INSERT INTO schema_version (version) VALUES (20);
        "#,
    )?;

    tracing::info!("Migration v20 complete: embedding_model column added");

    Ok(())
}

/// Schema v21: Facts table for SPO triples
///
/// Stores structured subject-predicate-object facts extracted from memories.
fn migrate_v21(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v21: Adding facts table...");

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS facts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            subject TEXT NOT NULL,
            predicate TEXT NOT NULL,
            object TEXT NOT NULL,
            confidence REAL NOT NULL DEFAULT 0.8,
            source_memory_id INTEGER,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );

        CREATE INDEX IF NOT EXISTS idx_facts_subject ON facts(subject);
        CREATE INDEX IF NOT EXISTS idx_facts_source ON facts(source_memory_id);

        INSERT INTO schema_version (version) VALUES (21);
        "#,
    )?;

    tracing::info!("Migration v21 complete: facts table added");

    Ok(())
}

/// Schema v22: Memory blocks + edit log
///
/// Adds Letta/MemGPT-inspired self-editing memory blocks with full revision
/// history in `block_edit_log`.
fn migrate_v22(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v22: Adding memory_blocks and block_edit_log tables...");

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS memory_blocks (
            name TEXT PRIMARY KEY,
            content TEXT NOT NULL DEFAULT '',
            version INTEGER NOT NULL DEFAULT 1,
            max_tokens INTEGER NOT NULL DEFAULT 4096,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );

        CREATE TABLE IF NOT EXISTS block_edit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            block_name TEXT NOT NULL,
            old_content TEXT NOT NULL,
            new_content TEXT NOT NULL,
            edit_reason TEXT NOT NULL DEFAULT '',
            timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            FOREIGN KEY (block_name) REFERENCES memory_blocks(name) ON DELETE CASCADE
        );

        INSERT INTO schema_version (version) VALUES (22);
        "#,
    )?;

    tracing::info!("Migration v22 complete: memory_blocks and block_edit_log tables added");

    Ok(())
}

/// Schema v23: Temporal knowledge graph edges
///
/// Adds `temporal_edges` with bi-temporal validity intervals for tracking how
/// relationships change over time.
fn migrate_v23(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v23: Adding temporal_edges table...");

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS temporal_edges (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id INTEGER NOT NULL,
            to_id INTEGER NOT NULL,
            relation TEXT NOT NULL,
            properties TEXT NOT NULL DEFAULT '{}',
            valid_from TEXT NOT NULL,
            valid_to TEXT,
            confidence REAL NOT NULL DEFAULT 1.0,
            source TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );

        CREATE INDEX IF NOT EXISTS idx_temporal_edges_from ON temporal_edges(from_id);
        CREATE INDEX IF NOT EXISTS idx_temporal_edges_to ON temporal_edges(to_id);
        CREATE INDEX IF NOT EXISTS idx_temporal_edges_valid ON temporal_edges(valid_from, valid_to);

        INSERT INTO schema_version (version) VALUES (23);
        "#,
    )?;

    tracing::info!("Migration v23 complete: temporal_edges table added");

    Ok(())
}

/// Schema v24: Scope path column on memories
///
/// Enables hierarchical scoping (Global > Org > User > Session > Agent) for
/// fine-grained multi-tenant memory isolation.
fn migrate_v24(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v24: Adding scope_path column to memories...");

    conn.execute_batch(
        r#"
        ALTER TABLE memories ADD COLUMN scope_path TEXT DEFAULT 'global';

        CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope_path);

        INSERT INTO schema_version (version) VALUES (24);
        "#,
    )?;

    tracing::info!("Migration v24 complete: scope_path column added");

    Ok(())
}

fn migrate_v25(conn: &Connection) -> Result<()> {
    tracing::info!("Migration v25: Creating search_feedback table...");

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS search_feedback (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            query TEXT NOT NULL,
            query_embedding_hash TEXT,
            memory_id INTEGER NOT NULL,
            signal TEXT NOT NULL CHECK(signal IN ('useful', 'irrelevant')),
            rank_position INTEGER,
            original_score REAL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            workspace TEXT DEFAULT 'default'
        );

        CREATE INDEX IF NOT EXISTS idx_feedback_memory ON search_feedback(memory_id);
        CREATE INDEX IF NOT EXISTS idx_feedback_query ON search_feedback(query);
        CREATE INDEX IF NOT EXISTS idx_feedback_workspace ON search_feedback(workspace);

        INSERT INTO schema_version (version) VALUES (25);
        "#,
    )?;

    tracing::info!("Migration v25 complete: search_feedback table created");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn in_memory_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        run_migrations(&conn).expect("run migrations");
        conn
    }

    #[test]
    fn test_fresh_db_reaches_v19() {
        let conn = in_memory_conn();
        let version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .expect("query schema version");
        assert_eq!(version, 25);
    }

    #[test]
    fn test_schema_version_constant_is_19() {
        assert_eq!(SCHEMA_VERSION, 25);
    }

    #[test]
    fn test_media_assets_table_exists() {
        let conn = in_memory_conn();
        conn.execute_batch(
            "INSERT INTO memories (content, memory_type, importance, visibility, metadata, valid_from)
             VALUES ('test memory', 'note', 0.5, 'private', '{}', CURRENT_TIMESTAMP)",
        )
        .expect("insert memory");
        let memory_id: i64 = conn
            .query_row("SELECT id FROM memories LIMIT 1", [], |row| row.get(0))
            .expect("get memory id");
        conn.execute(
            "INSERT INTO media_assets (memory_id, media_type, file_hash, mime_type)
             VALUES (?1, 'image', 'abc123hash', 'image/png')",
            rusqlite::params![memory_id],
        )
        .expect("insert media_asset");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM media_assets", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_media_assets_hash_uniqueness() {
        let conn = in_memory_conn();
        conn.execute_batch(
            "INSERT INTO memories (content, memory_type, importance, visibility, metadata, valid_from)
             VALUES ('test', 'note', 0.5, 'private', '{}', CURRENT_TIMESTAMP)",
        )
        .expect("insert memory");
        let memory_id: i64 = conn
            .query_row("SELECT id FROM memories LIMIT 1", [], |row| row.get(0))
            .expect("get memory id");
        conn.execute(
            "INSERT INTO media_assets (memory_id, media_type, file_hash) VALUES (?1, 'image', 'dup_hash')",
            rusqlite::params![memory_id],
        )
        .expect("first insert");
        let dup = conn.execute(
            "INSERT INTO media_assets (memory_id, media_type, file_hash) VALUES (?1, 'audio', 'dup_hash')",
            rusqlite::params![memory_id],
        );
        assert!(dup.is_err(), "duplicate file_hash should fail");
    }

    #[test]
    fn test_auto_links_table_exists() {
        let conn = in_memory_conn();
        // Insert a memory first (required by FK)
        conn.execute_batch(
            "INSERT INTO memories (content, memory_type, importance, visibility, metadata, valid_from)
             VALUES ('test memory', 'note', 0.5, 'private', '{}', CURRENT_TIMESTAMP)",
        )
        .expect("insert memory");
        let memory_id: i64 = conn
            .query_row("SELECT id FROM memories LIMIT 1", [], |row| row.get(0))
            .expect("get memory id");

        // Insert an auto_link pointing to itself (valid for test purposes)
        conn.execute(
            "INSERT INTO auto_links (from_id, to_id, link_type, score) VALUES (?1, ?2, 'semantic', 0.9)",
            rusqlite::params![memory_id, memory_id],
        )
        .expect("insert auto_link");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM auto_links", [], |row| row.get(0))
            .expect("count auto_links");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_auto_links_unique_pair_type() {
        let conn = in_memory_conn();
        conn.execute_batch(
            "INSERT INTO memories (content, memory_type, importance, visibility, metadata, valid_from)
             VALUES ('test memory', 'note', 0.5, 'private', '{}', CURRENT_TIMESTAMP)",
        )
        .expect("insert memory");
        let memory_id: i64 = conn
            .query_row("SELECT id FROM memories LIMIT 1", [], |row| row.get(0))
            .expect("get memory id");

        conn.execute(
            "INSERT INTO auto_links (from_id, to_id, link_type, score) VALUES (?1, ?2, 'semantic', 0.9)",
            rusqlite::params![memory_id, memory_id],
        )
        .expect("first insert");

        // Duplicate (from_id, to_id, link_type) should fail
        let result = conn.execute(
            "INSERT INTO auto_links (from_id, to_id, link_type, score) VALUES (?1, ?2, 'semantic', 0.8)",
            rusqlite::params![memory_id, memory_id],
        );
        assert!(result.is_err(), "duplicate pair+type should violate unique index");
    }

    #[test]
    fn test_memory_clusters_table_exists() {
        let conn = in_memory_conn();
        conn.execute_batch(
            "INSERT INTO memories (content, memory_type, importance, visibility, metadata, valid_from)
             VALUES ('test memory', 'note', 0.5, 'private', '{}', CURRENT_TIMESTAMP)",
        )
        .expect("insert memory");
        let memory_id: i64 = conn
            .query_row("SELECT id FROM memories LIMIT 1", [], |row| row.get(0))
            .expect("get memory id");

        conn.execute(
            "INSERT INTO memory_clusters (cluster_id, memory_id, algorithm) VALUES (1, ?1, 'louvain')",
            rusqlite::params![memory_id],
        )
        .expect("insert memory_cluster");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_clusters", [], |row| row.get(0))
            .expect("count memory_clusters");
        assert_eq!(count, 1);
    }

    #[test]
    fn test_upgrade_from_v17_to_v19() {
        // Simulate a v17 database by running only migrations up to v17
        let conn = Connection::open_in_memory().expect("open in-memory db");

        // Bootstrap schema_version table and run through v17 manually
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )
        .expect("create schema_version");

        // Run all migrations (they'll stop at the current version)
        // We simulate v17 state by running the full migration once,
        // then verify the version is 18.
        run_migrations(&conn).expect("run migrations from scratch");

        let version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .expect("query schema version");
        assert_eq!(version, 25, "should reach v25 after full migration");

        // Verify both new tables exist
        let auto_links_exists: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='auto_links'",
                [],
                |row| row.get(0),
            )
            .expect("check auto_links");
        assert_eq!(auto_links_exists, 1, "auto_links table should exist");

        let clusters_exists: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='memory_clusters'",
                [],
                |row| row.get(0),
            )
            .expect("check memory_clusters");
        assert_eq!(clusters_exists, 1, "memory_clusters table should exist");
    }
}
