# Engram Database Schema

**Date:** February 12, 2026  
**Current Version:** v15  
**Engine:** SQLite with WAL mode

---

## Table of Contents

1. [Overview](#overview)
2. [Schema (v15)](#schema-v15)
3. [Migration History](#migration-history)
4. [Indexes](#indexes)
5. [Migration Strategy](#migration-strategy)

---

## Overview

Engram uses SQLite with WAL (Write-Ahead Logging) mode for its storage layer. The schema supports:

- Core memory storage with metadata and versioning
- Full-text search via FTS5 (BM25 scoring)
- Vector embeddings via sqlite-vec
- Knowledge graph via cross-references
- Entity extraction and linking
- Identity unification (canonical IDs + aliases)
- Memory tiering and lifecycle management
- Session transcript indexing
- Multi-agent synchronization and sharing
- Salience scoring with trend history
- Quality scoring with conflict detection

### Design Principles

1. **Additive Migrations:** New columns and tables only; no destructive changes
2. **Nullable by Default:** New columns allow NULL for backward compatibility
3. **Index Thoughtfully:** Balance query speed vs. write performance
4. **JSON for Flexibility:** Metadata stored as JSON for schema evolution

---

## Schema (v15)

### Entity Relationship Diagram

```
┌─────────────────────┐       ┌──────────────────┐
│      memories       │──────<│   memory_tags    │
│  (core storage)     │       │ (memory_id, tag) │
└──────────┬──────────┘       └──────────────────┘
           │
           │  ┌──────────────────┐     ┌──────────────────┐
           ├─<│ memory_entities  │────>│    entities       │
           │  └──────────────────┘     └──────────────────┘
           │
           │  ┌──────────────────┐     ┌──────────────────┐
           ├─<│ mem_identity_lnk │────>│   identities     │
           │  └──────────────────┘     │       │           │
           │                           │       ▼           │
           │                           │ identity_aliases  │
           │                           └──────────────────┘
           │
           │  ┌──────────────────┐
           ├─<│    crossrefs     │ (source ↔ target)
           │  └──────────────────┘
           │
           │  ┌──────────────────┐
           ├─<│ memory_versions  │ (version history)
           │  └──────────────────┘
           │
           │  ┌──────────────────┐
           ├─<│   embeddings     │ (vector storage)
           │  └──────────────────┘
           │
           │  ┌──────────────────┐
           ├─<│ salience_history │ (score tracking)
           │  └──────────────────┘
           │
           │  ┌──────────────────┐
           ├─<│ quality_history  │ (quality tracking)
           │  └──────────────────┘
           │
           │  ┌──────────────────┐
           ├─<│ memory_conflicts │ (contradiction tracking)
           │  └──────────────────┘
           │
           │  ┌──────────────────┐
           ├─<│ duplicate_cands  │ (dedup cache)
           │  └──────────────────┘
           │
           │  ┌──────────────────┐     ┌──────────────────┐
           └─<│ session_memories │────>│    sessions       │
              └──────────────────┘     │       │           │
                                       │       ▼           │
                                       │ session_chunks    │
                                       └──────────────────┘

 Standalone tables:
 ┌────────────────────┐  ┌─────────────────────┐  ┌──────────────────┐
 │   memory_events    │  │  shared_memories     │  │ source_trust     │
 │  (change log)      │  │  (agent sharing)     │  │ _scores          │
 └────────────────────┘  └─────────────────────┘  └──────────────────┘
 ┌────────────────────┐  ┌─────────────────────┐  ┌──────────────────┐
 │   sync_state       │  │  agent_sync_state    │  │  audit_log       │
 └────────────────────┘  └─────────────────────┘  └──────────────────┘
 ┌────────────────────┐  ┌─────────────────────┐  ┌──────────────────┐
 │   sync_tasks       │  │  embedding_queue     │  │ schema_version   │
 └────────────────────┘  └─────────────────────┘  └──────────────────┘
```

---

### Core Tables

#### memories

Primary table storing all memory content. Accumulated columns from v1 through v15.

```sql
CREATE TABLE memories (
    -- Core (v1)
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
    metadata TEXT NOT NULL DEFAULT '{}',

    -- Scoping (v2)
    scope_type TEXT NOT NULL DEFAULT 'global',
    scope_id TEXT,

    -- Expiration (v5)
    expires_at TEXT,

    -- Deduplication (v6)
    content_hash TEXT,

    -- Workspace & Tier (v7)
    workspace TEXT NOT NULL DEFAULT 'default',
    tier TEXT NOT NULL DEFAULT 'permanent',

    -- Cognitive Types (v12)
    event_time TEXT,
    event_duration_seconds INTEGER,
    trigger_pattern TEXT,
    procedure_success_count INTEGER DEFAULT 0,
    procedure_failure_count INTEGER DEFAULT 0,
    summary_of_id INTEGER REFERENCES memories(id) ON DELETE SET NULL,

    -- Lifecycle (v13)
    lifecycle_state TEXT DEFAULT 'active',

    -- Quality (v15)
    quality_score REAL DEFAULT 0.5,
    validation_status TEXT DEFAULT 'unverified'
);
```

| Column | Type | Added | Description |
|--------|------|-------|-------------|
| id | INTEGER | v1 | Auto-increment primary key |
| content | TEXT | v1 | Memory content (required, non-empty) |
| memory_type | TEXT | v1 | note, todo, issue, decision, preference, learning, context, credential |
| importance | REAL | v1 | 0.0-1.0 user-set importance |
| access_count | INTEGER | v1 | Number of times accessed |
| created_at | TEXT | v1 | ISO8601 creation timestamp |
| updated_at | TEXT | v1 | ISO8601 last update timestamp |
| last_accessed_at | TEXT | v1 | ISO8601 last access timestamp |
| owner_id | TEXT | v1 | Owner identifier |
| visibility | TEXT | v1 | private or public |
| version | INTEGER | v1 | Optimistic concurrency control |
| has_embedding | INTEGER | v1 | Whether embedding has been computed |
| embedding_queued_at | TEXT | v1 | When embedding was queued |
| valid_from | TEXT | v1 | Temporal validity start |
| valid_to | TEXT | v1 | Temporal validity end |
| metadata | TEXT | v1 | JSON metadata |
| scope_type | TEXT | v2 | global, user, session, agent |
| scope_id | TEXT | v2 | The actual scope identifier |
| expires_at | TEXT | v5 | ISO8601 expiration (NULL = never) |
| content_hash | TEXT | v6 | SHA256 hash for deduplication |
| workspace | TEXT | v7 | Workspace isolation key |
| tier | TEXT | v7 | permanent or daily |
| event_time | TEXT | v12 | When event occurred (episodic memories) |
| event_duration_seconds | INTEGER | v12 | Event duration in seconds |
| trigger_pattern | TEXT | v12 | Trigger for procedural memories |
| procedure_success_count | INTEGER | v12 | Procedure success count |
| procedure_failure_count | INTEGER | v12 | Procedure failure count |
| summary_of_id | INTEGER | v12 | Source memory ID for summaries |
| lifecycle_state | TEXT | v13 | active, stale, or archived |
| quality_score | REAL | v15 | 0.0-1.0 computed quality |
| validation_status | TEXT | v15 | unverified, verified, disputed, stale |

#### tags

Normalized tag storage.

```sql
CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
);
```

#### memory_tags

Memory-to-tag relationship.

```sql
CREATE TABLE memory_tags (
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (memory_id, tag_id)
);
```

---

### Knowledge Graph Tables

#### crossrefs

Rich edges between memories for knowledge graph traversal.

```sql
CREATE TABLE crossrefs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    from_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    to_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
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
    UNIQUE(from_id, to_id, edge_type)
);
```

**Edge Types:** `related_to`, `supersedes`, `contradicts`, `depends_on`, `derived_from`, `mentions`, `part_of`

---

### Entity Tables (v3)

#### entities

Extracted named entities from NER.

```sql
CREATE TABLE entities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    aliases TEXT NOT NULL DEFAULT '[]',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    mention_count INTEGER NOT NULL DEFAULT 1,
    UNIQUE(normalized_name, entity_type)
);
```

**Entity Types:** `person`, `organization`, `project`, `concept`, `location`, `datetime`, `reference`, `other`

#### memory_entities

Links memories to extracted entities.

```sql
CREATE TABLE memory_entities (
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    entity_id INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    relation TEXT NOT NULL DEFAULT 'mentions',
    confidence REAL NOT NULL DEFAULT 1.0,
    char_offset INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (memory_id, entity_id, relation)
);
```

---

### Identity Tables (v9)

#### identities

Canonical identities for entity unification.

```sql
CREATE TABLE identities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    canonical_id TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    entity_type TEXT NOT NULL DEFAULT 'person',
    description TEXT,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

#### identity_aliases

Maps names/aliases to canonical identities.

```sql
CREATE TABLE identity_aliases (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    canonical_id TEXT NOT NULL REFERENCES identities(canonical_id) ON DELETE CASCADE,
    alias TEXT NOT NULL,
    alias_normalized TEXT NOT NULL UNIQUE,
    source TEXT,
    confidence REAL NOT NULL DEFAULT 1.0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

#### memory_identity_links

Links memories to canonical identities.

```sql
CREATE TABLE memory_identity_links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    canonical_id TEXT NOT NULL REFERENCES identities(canonical_id) ON DELETE CASCADE,
    mention_text TEXT,
    mention_count INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(memory_id, canonical_id)
);
```

---

### Session Tables (v8, v14)

#### sessions

Session tracking for transcript indexing and context management.

```sql
CREATE TABLE sessions (
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
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- Added in v14:
    summary TEXT,
    context TEXT,
    ended_at TEXT
);
```

#### session_chunks

Indexed conversation chunks linked to memories.

```sql
CREATE TABLE session_chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    start_message_index INTEGER NOT NULL,
    end_message_index INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(session_id, chunk_index)
);
```

#### session_memories (v14)

Links memories to sessions for context tracking.

```sql
CREATE TABLE session_memories (
    session_id TEXT NOT NULL REFERENCES sessions(session_id) ON DELETE CASCADE,
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    relevance_score REAL DEFAULT 1.0,
    context_role TEXT DEFAULT 'referenced',
    PRIMARY KEY (session_id, memory_id)
);
```

---

### Salience Tables (v14)

#### salience_history

Tracks salience scores over time for trend analysis.

```sql
CREATE TABLE salience_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    salience_score REAL NOT NULL,
    recency_score REAL,
    frequency_score REAL,
    importance_score REAL,
    feedback_score REAL,
    recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

**Salience Formula:**
```
Salience = (Recency × 0.3) + (Frequency × 0.2) + (Importance × 0.3) + (Feedback × 0.2)
```

---

### Quality Tables (v15)

#### quality_history

Tracks quality scores with component breakdown.

```sql
CREATE TABLE quality_history (
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
```

**Quality Formula:**
```
Quality = (Clarity × 0.25) + (Completeness × 0.20) + (Freshness × 0.20) +
          (Consistency × 0.20) + (Source_Trust × 0.15)
```

#### memory_conflicts

Tracks contradictions and conflicts between memories.

```sql
CREATE TABLE memory_conflicts (
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
```

**Conflict Types:** `contradiction`, `duplication`, `staleness`

#### source_trust_scores

Credibility scoring by origin type.

```sql
CREATE TABLE source_trust_scores (
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
```

**Default Trust Scores:** user (0.9) > seed (0.7) > extraction (0.6) > inference (0.5) > external (0.5)

#### duplicate_candidates

Cache for near-duplicate detection results.

```sql
CREATE TABLE duplicate_candidates (
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
```

---

### Embedding & Search Tables

#### embeddings

Vector embeddings for semantic search.

```sql
CREATE TABLE embeddings (
    memory_id INTEGER PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
    embedding BLOB NOT NULL,
    model TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

#### embedding_queue

Async embedding processing queue.

```sql
CREATE TABLE embedding_queue (
    memory_id INTEGER PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending',
    queued_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at TEXT,
    completed_at TEXT,
    error TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0
);
```

#### memories_fts (FTS5 Virtual Table)

Full-text search with BM25 scoring.

```sql
CREATE VIRTUAL TABLE memories_fts USING fts5(
    content, tags, metadata,
    content='memories', content_rowid='id',
    tokenize='porter unicode61'
);
```

Kept in sync via `AFTER INSERT`, `AFTER DELETE`, and `AFTER UPDATE` triggers on the `memories` table.

---

### Versioning & Audit Tables

#### memory_versions

Content version history.

```sql
CREATE TABLE memory_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    content TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_by TEXT,
    change_summary TEXT,
    UNIQUE(memory_id, version)
);
```

#### audit_log

Audit trail for all operations.

```sql
CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    user_id TEXT,
    action TEXT NOT NULL,
    memory_id INTEGER,
    changes TEXT,
    ip_address TEXT
);
```

---

### Event & Sync Tables (v10)

#### memory_events

Event log for real-time change notifications.

```sql
CREATE TABLE memory_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,
    memory_id INTEGER REFERENCES memories(id) ON DELETE SET NULL,
    agent_id TEXT,
    data TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    processed INTEGER NOT NULL DEFAULT 0
);
```

**Event Types:** `created`, `updated`, `deleted`, `linked`, `unlinked`, `shared`, `synced`

#### shared_memories

Multi-agent memory sharing.

```sql
CREATE TABLE shared_memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    memory_id INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    from_agent TEXT NOT NULL,
    to_agent TEXT,
    message TEXT,
    acknowledged INTEGER NOT NULL DEFAULT 0,
    acknowledged_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TEXT
);
```

#### sync_state

Global sync state (singleton row).

```sql
CREATE TABLE sync_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_sync TEXT,
    pending_changes INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    is_syncing INTEGER NOT NULL DEFAULT 0,
    version INTEGER NOT NULL DEFAULT 0  -- Added in v10
);
```

#### agent_sync_state

Per-agent sync tracking for delta synchronization.

```sql
CREATE TABLE agent_sync_state (
    agent_id TEXT PRIMARY KEY,
    last_sync_version INTEGER NOT NULL DEFAULT 0,
    last_sync_at TEXT,
    sync_metadata TEXT NOT NULL DEFAULT '{}'
);
```

#### sync_tasks (v12)

Background sync task tracking (used by Langfuse integration).

```sql
CREATE TABLE sync_tasks (
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
```

---

### System Tables

#### schema_version

Migration version tracking.

```sql
CREATE TABLE schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

---

## Migration History

| Version | Phase | Description | Key Changes |
|---------|-------|-------------|-------------|
| v1 | - | Initial schema | memories, tags, crossrefs, FTS5, embeddings, audit, sync |
| v2 | - | Memory scoping (RML-924) | `scope_type`, `scope_id` columns |
| v3 | - | Entity extraction (RML-925) | `entities`, `memory_entities` tables |
| v4 | - | Entity count fix | Recompute `mention_count` from links |
| v5 | - | Memory expiration (RML-930) | `expires_at` column |
| v6 | - | Deduplication (RML-931) | `content_hash` column + SHA256 backfill |
| v7 | - | Workspace & tier (RML-950) | `workspace`, `tier` columns |
| v8 | - | Session indexing | `sessions`, `session_chunks` tables |
| v9 | - | Identity links | `identities`, `identity_aliases`, `memory_identity_links` tables |
| v10 | - | Events & sharing | `memory_events`, `shared_memories`, `agent_sync_state` tables |
| v11 | - | Event agent fix | Ensure `agent_id` column on `memory_events` |
| v12 | Phase 1 | Cognitive types (ENG-33) | Episodic, procedural, summary columns; `sync_tasks` table |
| v13 | Phase 5 | Lifecycle (ENG-37) | `lifecycle_state` column |
| v14 | Phase 8 | Salience & sessions (ENG-66+) | `salience_history`, `session_memories` tables; session extensions |
| v15 | Phase 9 | Context quality (ENG-48+) | `quality_history`, `memory_conflicts`, `source_trust_scores`, `duplicate_candidates` tables |

---

## Indexes

### Memory Indexes

```sql
CREATE INDEX idx_memories_type ON memories(memory_type);
CREATE INDEX idx_memories_created ON memories(created_at DESC);
CREATE INDEX idx_memories_updated ON memories(updated_at DESC);
CREATE INDEX idx_memories_importance ON memories(importance DESC);
CREATE INDEX idx_memories_owner ON memories(owner_id);
CREATE INDEX idx_memories_visibility ON memories(visibility);
CREATE INDEX idx_memories_valid ON memories(valid_from, valid_to);
CREATE INDEX idx_memories_scope ON memories(scope_type, scope_id);
CREATE INDEX idx_memories_scope_type_created ON memories(scope_type, scope_id, memory_type, created_at DESC);
CREATE INDEX idx_memories_expires_at ON memories(expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX idx_memories_content_hash ON memories(content_hash, scope_type, scope_id) WHERE content_hash IS NOT NULL;
CREATE INDEX idx_memories_workspace_created ON memories(workspace, created_at DESC);
CREATE INDEX idx_memories_workspace_scope ON memories(workspace, scope_type, scope_id);
CREATE INDEX idx_memories_event_time ON memories(event_time) WHERE event_time IS NOT NULL;
CREATE INDEX idx_memories_summary_of ON memories(summary_of_id) WHERE summary_of_id IS NOT NULL;
CREATE INDEX idx_memories_lifecycle ON memories(lifecycle_state) WHERE lifecycle_state IS NOT NULL;
```

### Relationship Indexes

```sql
CREATE INDEX idx_crossrefs_from ON crossrefs(from_id);
CREATE INDEX idx_crossrefs_to ON crossrefs(to_id);
CREATE INDEX idx_crossrefs_type ON crossrefs(edge_type);
CREATE INDEX idx_crossrefs_valid ON crossrefs(valid_from, valid_to);
CREATE INDEX idx_memory_tags_memory ON memory_tags(memory_id);
CREATE INDEX idx_memory_tags_tag ON memory_tags(tag_id);
```

### Entity & Identity Indexes

```sql
CREATE INDEX idx_entities_type ON entities(entity_type);
CREATE INDEX idx_entities_normalized ON entities(normalized_name);
CREATE INDEX idx_entities_mention_count ON entities(mention_count DESC);
CREATE INDEX idx_memory_entities_memory ON memory_entities(memory_id);
CREATE INDEX idx_memory_entities_entity ON memory_entities(entity_id);
CREATE INDEX idx_memory_entities_relation ON memory_entities(relation);
CREATE INDEX idx_identity_aliases_normalized ON identity_aliases(alias_normalized);
CREATE INDEX idx_identity_aliases_canonical ON identity_aliases(canonical_id);
CREATE INDEX idx_identities_type ON identities(entity_type);
CREATE INDEX idx_memory_identity_links_canonical ON memory_identity_links(canonical_id);
CREATE INDEX idx_memory_identity_links_memory ON memory_identity_links(memory_id);
```

### Session & Salience Indexes

```sql
CREATE INDEX idx_sessions_workspace ON sessions(workspace, started_at DESC);
CREATE INDEX idx_session_chunks_session ON session_chunks(session_id, chunk_index);
CREATE INDEX idx_session_chunks_memory ON session_chunks(memory_id);
CREATE INDEX idx_session_memories_session ON session_memories(session_id);
CREATE INDEX idx_session_memories_memory ON session_memories(memory_id);
CREATE INDEX idx_salience_history_memory ON salience_history(memory_id, recorded_at DESC);
CREATE INDEX idx_salience_history_time ON salience_history(recorded_at DESC);
```

### Quality & Conflict Indexes

```sql
CREATE INDEX idx_quality_history_memory ON quality_history(memory_id, recorded_at DESC);
CREATE INDEX idx_memory_conflicts_a ON memory_conflicts(memory_a_id);
CREATE INDEX idx_memory_conflicts_b ON memory_conflicts(memory_b_id);
CREATE INDEX idx_memory_conflicts_unresolved ON memory_conflicts(resolved_at) WHERE resolved_at IS NULL;
CREATE INDEX idx_duplicate_candidates_pending ON duplicate_candidates(status) WHERE status = 'pending';
CREATE INDEX idx_duplicate_candidates_score ON duplicate_candidates(similarity_score DESC);
```

### Event & Sync Indexes

```sql
CREATE INDEX idx_memory_events_unprocessed ON memory_events(processed, created_at);
CREATE INDEX idx_memory_events_type ON memory_events(event_type, created_at DESC);
CREATE INDEX idx_memory_events_agent ON memory_events(agent_id, created_at DESC);
CREATE INDEX idx_shared_memories_recipient ON shared_memories(to_agent, acknowledged, created_at DESC);
CREATE INDEX idx_shared_memories_sender ON shared_memories(from_agent, created_at DESC);
CREATE INDEX idx_versions_memory ON memory_versions(memory_id);
CREATE INDEX idx_audit_memory ON audit_log(memory_id);
CREATE INDEX idx_audit_timestamp ON audit_log(timestamp DESC);
CREATE INDEX idx_audit_user ON audit_log(user_id);
CREATE INDEX idx_embedding_queue_status ON embedding_queue(status);
```

---

## Migration Strategy

### Principles

1. **Non-Breaking:** Migrations never delete columns or tables in active use
2. **Backward Compatible:** Old code continues to work during migration
3. **Atomic:** Each migration is a single transaction
4. **Reversible:** Keep rollback scripts for emergencies

### Migration Runner

```rust
pub fn run_migrations(conn: &Connection) -> Result<()> {
    let current_version = get_schema_version(conn)?;
    for version in (current_version + 1)..=SCHEMA_VERSION {
        migrate(conn, version)?;
        log::info!("Migrated to schema v{}", version);
    }
    Ok(())
}
```

### Version Tracking

```sql
CREATE TABLE schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

All migrations are located in `src/storage/migrations.rs`.

---

**See Also:**
- [ROADMAP.md](./ROADMAP.md) - Phase planning and completion status
- [../CHANGELOG.md](../CHANGELOG.md) - Version history with schema changes

---

**Last Updated:** February 12, 2026
