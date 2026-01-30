# Engram Database Schema

**Date:** January 29, 2026  
**Current Version:** v4  
**Target Version:** v15

---

## Table of Contents

1. [Overview](#overview)
2. [Current Schema (v4)](#current-schema-v4)
3. [Table Descriptions](#table-descriptions)
4. [Indexes](#indexes)
5. [Planned Migrations](#planned-migrations)
6. [Migration Strategy](#migration-strategy)

---

## Overview

Engram uses SQLite with WAL (Write-Ahead Logging) mode for its storage layer. The schema supports:

- Core memory storage with metadata
- Full-text search via FTS5
- Vector embeddings via sqlite-vec
- Knowledge graph via cross-references
- Entity extraction and linking
- Memory tiering and lifecycle
- Multi-agent synchronization

### Design Principles

1. **Additive Migrations:** New columns and tables; avoid destructive changes
2. **Nullable by Default:** New columns allow NULL for backward compatibility
3. **Index Thoughtfully:** Balance query speed vs. write performance
4. **JSON for Flexibility:** Metadata stored as JSON for schema evolution

---

## Current Schema (v4)

### Entity Relationship Diagram

```
┌─────────────────┐     ┌─────────────────┐
│    memories     │────<│   memory_tags   │
│  (id, content)  │     │  (memory_id)    │
└────────┬────────┘     └─────────────────┘
         │
         │              ┌─────────────────┐
         ├─────────────<│memory_metadata  │
         │              │  (memory_id)    │
         │              └─────────────────┘
         │
         │              ┌─────────────────┐
         ├─────────────<│   crossrefs     │
         │              │(source, target) │
         │              └─────────────────┘
         │
         │              ┌─────────────────┐     ┌─────────────────┐
         └─────────────<│memory_entities  │────>│    entities     │
                        │(memory, entity) │     │ (id, name)      │
                        └─────────────────┘     └─────────────────┘
```

### Core Tables

#### memories

The primary table storing all memory content.

```sql
CREATE TABLE memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,
    memory_type TEXT NOT NULL DEFAULT 'note',
    importance REAL NOT NULL DEFAULT 0.5,
    quality_score REAL NOT NULL DEFAULT 0.5,
    scope TEXT NOT NULL DEFAULT 'global',
    owner_id TEXT,
    source TEXT,
    workspace TEXT DEFAULT 'default',
    tier TEXT DEFAULT 'permanent',
    expires_at TEXT,
    version INTEGER DEFAULT 1,
    deleted INTEGER DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER | Auto-increment primary key |
| content | TEXT | Memory content (required) |
| memory_type | TEXT | Type: note, todo, issue, decision, preference, learning, context, credential |
| importance | REAL | 0.0-1.0 user-set importance |
| quality_score | REAL | 0.0-1.0 computed quality |
| scope | TEXT | global, user:{id}, session:{id}, agent:{id} |
| owner_id | TEXT | ID when scope is user/session/agent |
| source | TEXT | Origin of memory (e.g., "manual", "github_api") |
| workspace | TEXT | Workspace isolation |
| tier | TEXT | "permanent" or "daily" |
| expires_at | TEXT | ISO8601 expiration for daily tier |
| version | INTEGER | Optimistic concurrency control |
| deleted | INTEGER | Soft delete flag |
| created_at | TEXT | ISO8601 creation timestamp |
| updated_at | TEXT | ISO8601 last update timestamp |

#### memory_tags

Tags for categorization and filtering.

```sql
CREATE TABLE memory_tags (
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (memory_id, tag)
);
```

| Column | Type | Description |
|--------|------|-------------|
| memory_id | INTEGER | Foreign key to memories |
| tag | TEXT | Tag value (e.g., "project/engram") |

#### memory_metadata

Arbitrary key-value metadata.

```sql
CREATE TABLE memory_metadata (
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (memory_id, key)
);
```

| Column | Type | Description |
|--------|------|-------------|
| memory_id | INTEGER | Foreign key to memories |
| key | TEXT | Metadata key |
| value | TEXT | Metadata value (JSON-encoded if complex) |

#### crossrefs

Knowledge graph edges between memories.

```sql
CREATE TABLE crossrefs (
    source_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    target_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    edge_type TEXT NOT NULL DEFAULT 'related_to',
    weight REAL NOT NULL DEFAULT 1.0,
    confidence REAL NOT NULL DEFAULT 1.0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (source_id, target_id, edge_type)
);
```

| Column | Type | Description |
|--------|------|-------------|
| source_id | INTEGER | Source memory ID |
| target_id | INTEGER | Target memory ID |
| edge_type | TEXT | Relationship type (see below) |
| weight | REAL | Edge strength (0.0-1.0) |
| confidence | REAL | Confidence in relationship |
| created_at | TEXT | When link was created |
| updated_at | TEXT | Last update |

**Edge Types:**
- `related_to` - General relationship
- `supersedes` - Source replaces target
- `contradicts` - Source conflicts with target
- `depends_on` - Source requires target
- `derived_from` - Source was created from target
- `mentions` - Source references target
- `part_of` - Source is component of target

### Entity Tables

#### entities

Extracted named entities.

```sql
CREATE TABLE entities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    mention_count INTEGER DEFAULT 1,
    first_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    metadata TEXT
);
```

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER | Auto-increment primary key |
| name | TEXT | Original entity name |
| normalized_name | TEXT | Lowercase, trimmed name |
| entity_type | TEXT | person, organization, project, concept, location, datetime, reference, other |
| mention_count | INTEGER | How many times mentioned |
| first_seen_at | TEXT | First occurrence |
| last_seen_at | TEXT | Most recent occurrence |
| metadata | TEXT | JSON additional data |

#### memory_entities

Links memories to entities.

```sql
CREATE TABLE memory_entities (
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    entity_id INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    relationship_type TEXT DEFAULT 'mentions',
    confidence REAL DEFAULT 1.0,
    context_snippet TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (memory_id, entity_id)
);
```

| Column | Type | Description |
|--------|------|-------------|
| memory_id | INTEGER | Foreign key to memories |
| entity_id | INTEGER | Foreign key to entities |
| relationship_type | TEXT | How entity relates (mentions, about, authored_by) |
| confidence | REAL | NER confidence score |
| context_snippet | TEXT | Surrounding text context |
| created_at | TEXT | When link was created |

### Identity Tables

#### identities

Canonical identities for entity unification.

```sql
CREATE TABLE identities (
    canonical_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    metadata TEXT
);
```

#### identity_aliases

Aliases that resolve to canonical identities.

```sql
CREATE TABLE identity_aliases (
    alias TEXT PRIMARY KEY,
    canonical_id TEXT NOT NULL REFERENCES identities(canonical_id) ON DELETE CASCADE
);
```

#### memory_identities

Links memories to identities.

```sql
CREATE TABLE memory_identities (
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    canonical_id TEXT NOT NULL REFERENCES identities(canonical_id) ON DELETE CASCADE,
    mention_text TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (memory_id, canonical_id)
);
```

### Search Tables

#### memories_fts (FTS5)

Full-text search virtual table.

```sql
CREATE VIRTUAL TABLE memories_fts USING fts5(
    content,
    content='memories',
    content_rowid='id'
);

-- Triggers to keep FTS in sync
CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content) 
    VALUES('delete', old.id, old.content);
END;

CREATE TRIGGER memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content) 
    VALUES('delete', old.id, old.content);
    INSERT INTO memories_fts(rowid, content) VALUES (new.id, new.content);
END;
```

#### embeddings (sqlite-vec)

Vector embeddings for semantic search.

```sql
CREATE VIRTUAL TABLE embeddings USING vec0(
    memory_id INTEGER PRIMARY KEY,
    embedding FLOAT[1536]  -- Dimension matches embedding model
);
```

### Event & Sync Tables

#### memory_events

Event log for change tracking.

```sql
CREATE TABLE memory_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,
    memory_id INTEGER,
    agent_id TEXT,
    data TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

**Event Types:** created, updated, deleted, linked, unlinked, shared, synced

#### shared_memories

Multi-agent memory sharing.

```sql
CREATE TABLE shared_memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    from_agent TEXT NOT NULL,
    to_agent TEXT NOT NULL,
    message TEXT,
    acknowledged INTEGER DEFAULT 0,
    acknowledged_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

#### agent_sync_state

Sync state per agent.

```sql
CREATE TABLE agent_sync_state (
    agent_id TEXT PRIMARY KEY,
    last_sync_version INTEGER NOT NULL DEFAULT 0,
    last_sync_time TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### Session Tables

#### sessions

Session tracking for transcript indexing.

```sql
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    ended_at TEXT,
    message_count INTEGER DEFAULT 0,
    metadata TEXT
);
```

#### session_chunks

Indexed conversation chunks.

```sql
CREATE TABLE session_chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

---

## Table Descriptions

### Memory Types

| Type | Description | Use Case |
|------|-------------|----------|
| note | General information | Default type |
| todo | Action item | Task tracking |
| issue | Bug or problem | Issue tracking |
| decision | Architectural decision | ADR records |
| preference | User preference | Personalization |
| learning | Lesson learned | Knowledge capture |
| context | Project context | CLAUDE.md content |
| credential | Secret/credential | Secure storage |

### Memory Scopes

| Scope | Description |
|-------|-------------|
| global | Available to all |
| user:{id} | Specific to user |
| session:{id} | Specific to session |
| agent:{id} | Specific to agent |

### Memory Tiers

| Tier | Behavior |
|------|----------|
| permanent | Never expires |
| daily | Expires after TTL (default 24h) |

---

## Indexes

### Primary Indexes

```sql
-- Memory lookups
CREATE INDEX idx_memories_workspace ON memories(workspace);
CREATE INDEX idx_memories_tier ON memories(tier);
CREATE INDEX idx_memories_created_at ON memories(created_at);
CREATE INDEX idx_memories_deleted ON memories(deleted);

-- Tag lookups
CREATE INDEX idx_memory_tags_tag ON memory_tags(tag);

-- Entity lookups
CREATE INDEX idx_entities_normalized ON entities(normalized_name);
CREATE INDEX idx_entities_type ON entities(entity_type);
CREATE INDEX idx_memory_entities_entity ON memory_entities(entity_id);

-- Crossref lookups
CREATE INDEX idx_crossrefs_target ON crossrefs(target_id);
CREATE INDEX idx_crossrefs_type ON crossrefs(edge_type);

-- Event lookups
CREATE INDEX idx_events_type ON memory_events(event_type);
CREATE INDEX idx_events_created ON memory_events(created_at);

-- Session lookups
CREATE INDEX idx_session_chunks_session ON session_chunks(session_id);
```

### Composite Indexes

```sql
-- Common query patterns
CREATE INDEX idx_memories_workspace_created 
ON memories(workspace, created_at DESC);

CREATE INDEX idx_memories_tier_expires 
ON memories(tier, expires_at) 
WHERE tier = 'daily';

CREATE INDEX idx_crossrefs_source_type 
ON crossrefs(source_id, edge_type);
```

---

## Planned Migrations

### v5: Storage Abstraction

```sql
-- Backend metadata
CREATE TABLE storage_backends (
    id TEXT PRIMARY KEY,
    backend_type TEXT NOT NULL,
    config TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Query metrics
CREATE TABLE query_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    query_type TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    backend TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### v8: Cognitive Memory Types

```sql
-- Add memory class
ALTER TABLE memories ADD COLUMN memory_class TEXT;
-- Values: episodic, semantic, procedural

-- Episodic memory metadata
CREATE TABLE episodic_metadata (
    memory_id INTEGER PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
    event_time TEXT,
    event_duration TEXT,
    participants TEXT,  -- JSON array
    location TEXT
);

-- Procedural memory steps
CREATE TABLE procedural_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    step_number INTEGER NOT NULL,
    instruction TEXT NOT NULL,
    expected_result TEXT
);
```

### v12: Memory Lifecycle

```sql
-- Add lifecycle columns
ALTER TABLE memories ADD COLUMN validation_status TEXT DEFAULT 'unverified';
-- Values: unverified, validated, confirmed, invalidated

ALTER TABLE memories ADD COLUMN archived INTEGER DEFAULT 0;
ALTER TABLE memories ADD COLUMN archived_at TEXT;
ALTER TABLE memories ADD COLUMN archive_reason TEXT;

-- Retention policies
CREATE TABLE retention_policies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    workspace TEXT,
    memory_type TEXT,
    max_age_days INTEGER,
    action TEXT NOT NULL,  -- archive, delete
    enabled INTEGER DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Lifecycle events
CREATE TABLE lifecycle_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id INTEGER NOT NULL REFERENCES memories(id),
    event_type TEXT NOT NULL,  -- validated, invalidated, archived, restored
    triggered_by TEXT,
    metadata TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### v14: Salience & Session Memory

```sql
-- Add salience columns
ALTER TABLE memories ADD COLUMN salience_score REAL DEFAULT 0.5;
ALTER TABLE memories ADD COLUMN last_accessed_at TEXT;
ALTER TABLE memories ADD COLUMN access_count INTEGER DEFAULT 0;

-- Enhanced sessions
ALTER TABLE sessions ADD COLUMN summary TEXT;
ALTER TABLE sessions ADD COLUMN context TEXT;

-- Session memories
CREATE TABLE session_memories (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    added_at TEXT NOT NULL DEFAULT (datetime('now')),
    relevance_score REAL DEFAULT 1.0,
    PRIMARY KEY (session_id, memory_id)
);

-- Salience history
CREATE TABLE salience_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    salience_score REAL NOT NULL,
    recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### v15: Context Quality

```sql
-- Quality breakdown
ALTER TABLE memories ADD COLUMN quality_breakdown TEXT;  -- JSON

-- Conflicts
CREATE TABLE memory_conflicts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_a INTEGER NOT NULL REFERENCES memories(id),
    memory_b INTEGER NOT NULL REFERENCES memories(id),
    conflict_type TEXT NOT NULL,  -- contradiction, duplication, staleness
    severity REAL DEFAULT 0.5,
    resolved INTEGER DEFAULT 0,
    resolution TEXT,
    resolved_by TEXT,
    detected_at TEXT NOT NULL DEFAULT (datetime('now')),
    resolved_at TEXT
);

-- Quality history
CREATE TABLE quality_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    quality_score REAL NOT NULL,
    components TEXT,  -- JSON breakdown
    recorded_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Duplicate candidates
CREATE TABLE duplicate_candidates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_a INTEGER NOT NULL REFERENCES memories(id),
    memory_b INTEGER NOT NULL REFERENCES memories(id),
    similarity_score REAL NOT NULL,
    detected_at TEXT NOT NULL DEFAULT (datetime('now')),
    reviewed INTEGER DEFAULT 0,
    action TEXT  -- merge, keep_both, delete_one
);
```

---

## Migration Strategy

### Principles

1. **Non-Breaking:** Migrations never delete columns or tables in active use
2. **Backward Compatible:** Old code continues to work during migration
3. **Atomic:** Each migration is a single transaction
4. **Reversible:** Keep rollback scripts for emergencies

### Migration Process

```rust
pub fn run_migrations(conn: &Connection) -> Result<()> {
    let current_version = get_schema_version(conn)?;
    
    for (version, migration) in MIGRATIONS.iter() {
        if *version > current_version {
            conn.execute_batch(&migration.up)?;
            set_schema_version(conn, *version)?;
            log::info!("Migrated to schema v{}", version);
        }
    }
    
    Ok(())
}
```

### Version Tracking

```sql
CREATE TABLE schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

---

**See Also:**
- [ROADMAP.md](./ROADMAP.md) - When migrations are planned
- [LINEAR_ISSUES.md](./LINEAR_ISSUES.md) - Issues related to schema changes

---

**Last Updated:** January 29, 2026
