# Engram Improvements - Memora Feature Parity

**Date:** January 28, 2026  
**Version:** 0.2.0 (Unreleased)  
**Author:** Ronaldo Lima

This document details the improvements made to Engram to achieve feature parity with [Memora](https://github.com/limaronaldo/memora). Engram is a Rust-based AI memory engine, while Memora is Python-based. Both now share a comprehensive feature set for persistent AI memory.

---

## Table of Contents

1. [Feature Comparison Matrix](#feature-comparison-matrix)
2. [New MCP Tools Summary](#new-mcp-tools-summary)
3. [Batch Operations](#1-batch-operations)
4. [Tag Utilities](#2-tag-utilities)
5. [Import/Export](#3-importexport)
6. [Maintenance Tools](#4-maintenance-tools)
7. [Special Memory Types](#5-special-memory-types)
8. [Event System](#6-event-system)
9. [Advanced Sync](#7-advanced-sync)
10. [Multi-Agent Sharing](#8-multi-agent-sharing)
11. [Search Variants](#9-search-variants)
12. [Image Handling](#10-image-handling)
13. [Content Utilities](#11-content-utilities)
14. [Migration Guide](#migration-guide)

---

## Feature Comparison Matrix

| Feature | Engram (Rust) | Memora (Python) | Notes |
|---------|---------------|-----------------|-------|
| **Core Memory CRUD** | ✅ | ✅ | Both support create, read, update, delete |
| **Hybrid Search** | ✅ BM25+Vec+Fuzzy | ✅ BM25+Vec | Engram adds fuzzy matching |
| **Memory Tiering** | ✅ | ✅ | Daily/Permanent tiers with TTL |
| **Multi-Workspace** | ✅ | ✅ | Project-based isolation |
| **Identity Links** | ✅ | ✅ | Entity unification with aliases |
| **Session Indexing** | ✅ | ✅ | Conversation transcript storage |
| **Knowledge Graph** | ✅ | ✅ | Cross-references, traversal, paths |
| **Cloud Sync** | ✅ S3/R2 | ✅ S3/R2/D1 | Both support encrypted sync |
| **Batch Operations** | ✅ | ✅ | Bulk create/delete |
| **Event System** | ✅ | ✅ | Change tracking for sync |
| **Multi-Agent Sharing** | ✅ | ✅ | Share memories between agents |
| **Image Handling** | ✅ Local | ✅ R2 | Engram local, engram-cloud uses R2 |
| **Tag Hierarchy** | ✅ | ✅ | Slash-separated tag paths |
| **Soft Trim** | ✅ | ✅ | Content truncation with context |
| **Embedding Cache** | ✅ | ✅ | LRU cache for performance |
| **Document Ingestion** | ✅ | ❌ | Engram-only feature |
| **Graph Visualization** | ✅ vis.js | ✅ WebSocket | Different implementations |

---

## New MCP Tools Summary

Engram now provides **77 MCP tools** (up from ~55). Here are the new tools added:

### Batch Operations (2 tools)
- `memory_create_batch` - Create multiple memories in one operation
- `memory_delete_batch` - Delete multiple memories in one operation

### Tag Utilities (3 tools)
- `memory_tags` - List all tags with usage counts
- `memory_tag_hierarchy` - Get hierarchical tag tree
- `memory_validate_tags` - Check tag consistency

### Import/Export (2 tools)
- `memory_export` - Export all memories to JSON
- `memory_import` - Import memories from JSON

### Maintenance (2 tools)
- `memory_rebuild_embeddings` - Rebuild missing embeddings
- `memory_rebuild_crossrefs` - Rebuild cross-references

### Special Memory Types (3 tools)
- `memory_create_section` - Create hierarchical sections
- `memory_checkpoint` - Create session checkpoints
- `memory_boost` - Temporarily boost importance

### Event System (2 tools)
- `memory_events_poll` - Poll for change events
- `memory_events_clear` - Clear old events

### Advanced Sync (4 tools)
- `sync_version` - Get current sync version
- `sync_delta` - Get changes since version
- `sync_state` - Get/update agent sync state
- `sync_cleanup` - Clean up old sync data

### Multi-Agent Sharing (3 tools)
- `memory_share` - Share memory with another agent
- `memory_shared_poll` - Poll for shared memories
- `memory_share_ack` - Acknowledge shared memory

### Search Variants (2 tools)
- `memory_search_by_identity` - Search by entity/alias
- `memory_session_search` - Search session transcripts

### Image Handling (2 tools)
- `memory_upload_image` - Upload and attach image to memory
- `memory_migrate_images` - Migrate base64 images to storage

### Content Utilities (3 tools)
- `memory_soft_trim` - Intelligent content truncation
- `memory_list_compact` - Compact memory listing
- `memory_content_stats` - Content statistics

---

## 1. Batch Operations

### Problem

Creating or deleting many memories one at a time is inefficient for bulk imports or cleanup operations.

### Solution

Added batch operations that process multiple memories in a single database transaction.

### MCP Tools

#### `memory_create_batch`

```json
{
  "memories": [
    {"content": "First memory", "tags": ["import"]},
    {"content": "Second memory", "type": "decision"},
    {"content": "Third memory", "workspace": "project-a"}
  ]
}
```

**Response:**
```json
{
  "created_count": 3,
  "failed_count": 0,
  "ids": [101, 102, 103],
  "errors": []
}
```

#### `memory_delete_batch`

```json
{
  "ids": [101, 102, 103]
}
```

**Response:**
```json
{
  "deleted_count": 3,
  "not_found_count": 0
}
```

### Implementation

Located in `src/storage/queries.rs`:
- `create_memory_batch()` - Uses transaction for atomicity
- `delete_memory_batch()` - Soft-deletes with versioning

---

## 2. Tag Utilities

### Problem

Tags accumulate over time. Without utilities, it's hard to discover what tags exist, find inconsistencies, or understand tag relationships.

### Solution

Added tag management tools for listing, hierarchy visualization, and validation.

### MCP Tools

#### `memory_tags`

Lists all tags with usage statistics.

**Response:**
```json
{
  "tags": [
    {"tag": "project/engram", "count": 45, "last_used": "2026-01-28T10:00:00Z"},
    {"tag": "decision", "count": 23, "last_used": "2026-01-27T15:30:00Z"},
    {"tag": "todo", "count": 12, "last_used": "2026-01-28T09:15:00Z"}
  ]
}
```

#### `memory_tag_hierarchy`

Returns tags as a tree structure. Tags with slashes (e.g., `project/engram/core`) are nested.

**Response:**
```json
{
  "hierarchy": [
    {
      "name": "project",
      "full_path": "project",
      "count": 0,
      "children": [
        {
          "name": "engram",
          "full_path": "project/engram",
          "count": 30,
          "children": [
            {"name": "core", "full_path": "project/engram/core", "count": 15, "children": []}
          ]
        }
      ]
    },
    {"name": "decision", "full_path": "decision", "count": 23, "children": []}
  ]
}
```

#### `memory_validate_tags`

Checks tag consistency and suggests normalizations.

**Response:**
```json
{
  "total_tags": 45,
  "orphaned_tags": ["old-project"],
  "similar_tags": [
    {"tags": ["TODO", "todo", "Todo"], "suggestion": "todo"}
  ],
  "empty_tags": []
}
```

---

## 3. Import/Export

### Problem

Need to backup memories, migrate between instances, or share memory sets.

### Solution

Added JSON-based import/export with deduplication support.

### MCP Tools

#### `memory_export`

```json
{
  "workspace": "project-a",
  "include_embeddings": false
}
```

**Response:**
```json
{
  "version": "1.0",
  "exported_at": "2026-01-28T10:00:00Z",
  "memory_count": 150,
  "memories": [
    {
      "id": 1,
      "content": "Memory content",
      "memory_type": "note",
      "tags": ["tag1"],
      "metadata": {},
      "created_at": "2026-01-15T09:00:00Z"
    }
  ]
}
```

#### `memory_import`

```json
{
  "data": { /* exported data object */ },
  "skip_duplicates": true
}
```

**Response:**
```json
{
  "imported_count": 145,
  "skipped_count": 5,
  "error_count": 0
}
```

---

## 4. Maintenance Tools

### Problem

Over time, embeddings may become stale or cross-references may need rebuilding after schema changes.

### Solution

Added maintenance tools for rebuilding derived data.

### MCP Tools

#### `memory_rebuild_embeddings`

Regenerates embeddings for all memories missing them.

**Response:**
```json
{
  "rebuilt": 23
}
```

#### `memory_rebuild_crossrefs`

Re-analyzes all memories to find and create cross-reference links.

**Response:**
```json
{
  "rebuilt": 156
}
```

---

## 5. Special Memory Types

### Problem

Some memories have special semantics: section headers for organization, checkpoints for session state, or temporarily important items.

### Solution

Added specialized creation functions with appropriate defaults.

### MCP Tools

#### `memory_create_section`

Creates a section memory for hierarchical organization.

```json
{
  "title": "Architecture Decisions",
  "content": "This section contains ADRs",
  "parent_id": 10,
  "level": 2,
  "workspace": "project-a"
}
```

**Response:** Returns created memory with `section:Architecture Decisions` tag and section metadata.

#### `memory_checkpoint`

Creates a checkpoint marking session state.

```json
{
  "session_id": "session-abc123",
  "summary": "Completed auth refactor, starting on API tests",
  "context": {"files_modified": 12, "tests_passing": true},
  "workspace": "project-a"
}
```

**Response:** Returns checkpoint memory with `checkpoint` and `session:session-abc123` tags.

#### `memory_boost`

Temporarily increases a memory's importance score.

```json
{
  "id": 42,
  "boost_amount": 0.3,
  "duration_seconds": 3600
}
```

**Response:** Returns updated memory. Boost decays after duration (stored in metadata for future decay implementation).

---

## 6. Event System

### Problem

Multi-agent systems need to track changes for synchronization.

### Solution

Added an event log that records all memory operations.

### Schema

```sql
CREATE TABLE memory_events (
    id INTEGER PRIMARY KEY,
    event_type TEXT NOT NULL,  -- created, updated, deleted, linked, unlinked, shared, synced
    memory_id INTEGER,
    agent_id TEXT,
    data TEXT,  -- JSON payload
    created_at TEXT NOT NULL
);
```

### MCP Tools

#### `memory_events_poll`

Poll for events since a point in time or event ID.

```json
{
  "since_id": 100,
  "agent_id": "agent-1",
  "limit": 50
}
```

**Response:**
```json
{
  "events": [
    {
      "id": 101,
      "event_type": "created",
      "memory_id": 42,
      "agent_id": "agent-1",
      "data": {"workspace": "default"},
      "created_at": "2026-01-28T10:00:00Z"
    }
  ]
}
```

#### `memory_events_clear`

Clean up old events.

```json
{
  "keep_recent": 1000
}
```

**Response:**
```json
{
  "deleted": 5000
}
```

---

## 7. Advanced Sync

### Problem

Multi-instance deployments need version tracking and delta synchronization.

### Solution

Added version tracking and delta queries for efficient sync.

### Schema

```sql
CREATE TABLE agent_sync_state (
    agent_id TEXT PRIMARY KEY,
    last_sync_version INTEGER NOT NULL,
    last_sync_time TEXT NOT NULL
);
```

### MCP Tools

#### `sync_version`

Get current sync version and metadata.

**Response:**
```json
{
  "version": 1234,
  "last_modified": "2026-01-28T10:00:00Z",
  "memory_count": 500,
  "checksum": "500-1234-2026-01-28T10:00:00Z"
}
```

#### `sync_delta`

Get changes since a specific version.

```json
{
  "since_version": 1200
}
```

**Response:**
```json
{
  "created": [/* new memories */],
  "updated": [/* modified memories */],
  "deleted": [42, 43, 44],
  "from_version": 1200,
  "to_version": 1234
}
```

#### `sync_state`

Get or update agent sync state.

```json
{
  "agent_id": "agent-1",
  "update_version": 1234
}
```

**Response:**
```json
{
  "agent_id": "agent-1",
  "last_sync_version": 1234,
  "last_sync_time": "2026-01-28T10:00:00Z",
  "pending_changes": 0
}
```

#### `sync_cleanup`

Clean up old sync data.

```json
{
  "older_than_days": 30
}
```

---

## 8. Multi-Agent Sharing

### Problem

Multiple agents may need to share specific memories with each other.

### Solution

Added explicit memory sharing with acknowledgment tracking.

### Schema

```sql
CREATE TABLE shared_memories (
    id INTEGER PRIMARY KEY,
    memory_id INTEGER NOT NULL,
    from_agent TEXT NOT NULL,
    to_agent TEXT NOT NULL,
    message TEXT,
    acknowledged INTEGER DEFAULT 0,
    acknowledged_at TEXT,
    created_at TEXT NOT NULL
);
```

### MCP Tools

#### `memory_share`

Share a memory with another agent.

```json
{
  "memory_id": 42,
  "from_agent": "agent-1",
  "to_agent": "agent-2",
  "message": "Check out this decision about auth"
}
```

**Response:**
```json
{
  "share_id": 1
}
```

#### `memory_shared_poll`

Poll for memories shared with this agent.

```json
{
  "agent_id": "agent-2",
  "include_acknowledged": false
}
```

**Response:**
```json
{
  "shares": [
    {
      "id": 1,
      "memory_id": 42,
      "from_agent": "agent-1",
      "to_agent": "agent-2",
      "message": "Check out this decision about auth",
      "acknowledged": false,
      "created_at": "2026-01-28T10:00:00Z"
    }
  ]
}
```

#### `memory_share_ack`

Acknowledge receipt of a shared memory.

```json
{
  "share_id": 1,
  "agent_id": "agent-2"
}
```

---

## 9. Search Variants

### Problem

Need specialized search for identities and session transcripts.

### Solution

Added targeted search functions.

### MCP Tools

#### `memory_search_by_identity`

Search memories mentioning a specific identity or alias.

```json
{
  "identity": "ronaldo",
  "workspace": "project-a",
  "limit": 20
}
```

#### `memory_session_search`

Search within session transcript chunks.

```json
{
  "query": "authentication discussion",
  "session_id": "session-abc",
  "limit": 10
}
```

---

## 10. Image Handling

### Problem

Memories may reference images, either as base64 data URIs or external files.

### Solution

Added local image storage with migration support.

**Note:** For cloud deployments, [engram-cloud](https://github.com/limaronaldo/engram-cloud) uses Cloudflare R2 for image storage instead of local files. The R2 integration provides:
- Presigned URLs for secure access (1-hour expiry)
- Tenant-isolated storage paths
- Same MCP tool interface via `/v1/mcp` endpoint

### Storage Structure

**Local (engram):**
```
~/.local/share/engram/images/
└── images/
    └── {memory_id}/
        └── {timestamp}_{index}_{hash}.{ext}
```

**Cloud (engram-cloud R2):**
```
s3://bucket/tenants/{tenant_id}/images/{memory_id}/
    └── {timestamp}_{index}_{hash}.{ext}
```

### MCP Tools

#### `memory_upload_image`

Upload an image file and attach it to a memory.

```json
{
  "memory_id": 42,
  "file_path": "/path/to/screenshot.png",
  "image_index": 0,
  "caption": "Architecture diagram"
}
```

**Response:**
```json
{
  "success": true,
  "image": {
    "url": "local://images/42/1706436000_0_a1b2c3d4.png",
    "caption": "Architecture diagram",
    "index": 0,
    "content_type": "image/png",
    "size": 45678
  }
}
```

#### `memory_migrate_images`

Migrate base64-encoded images to file storage.

```json
{
  "dry_run": true
}
```

**Response:**
```json
{
  "memories_scanned": 500,
  "memories_with_images": 23,
  "images_migrated": 45,
  "images_failed": 0,
  "errors": [],
  "dry_run": true
}
```

---

## 11. Content Utilities

### Problem

Long content needs intelligent truncation for display, and content statistics are useful for analysis.

### Solution

Added content manipulation and analysis utilities.

### MCP Tools

#### `memory_soft_trim`

Intelligently trim content preserving head and tail context.

```json
{
  "id": 42,
  "max_chars": 500,
  "head_percent": 60,
  "tail_percent": 30,
  "preserve_words": true
}
```

**Response:**
```json
{
  "id": 42,
  "original_length": 2000,
  "trimmed_length": 500,
  "trimmed_content": "This is the beginning of the content...\n...\n...and this is how it ends.",
  "was_trimmed": true
}
```

#### `memory_list_compact`

List memories with compact preview.

```json
{
  "limit": 20,
  "workspace": "project-a",
  "preview_chars": 100
}
```

**Response:**
```json
{
  "memories": [
    {
      "id": 42,
      "preview": "This is the beginning of a long memory that has been truncated...",
      "full_length": 2000,
      "memory_type": "note",
      "tags": ["important"],
      "created_at": "2026-01-28T10:00:00Z"
    }
  ]
}
```

#### `memory_content_stats`

Get content statistics.

```json
{
  "id": 42
}
```

**Response:**
```json
{
  "id": 42,
  "stats": {
    "char_count": 2000,
    "word_count": 350,
    "line_count": 45,
    "sentence_count": 28,
    "paragraph_count": 8
  }
}
```

---

## Migration Guide

### From Memora to Engram

1. **Export from Memora:**
   ```python
   result = memory_export()
   with open("memora_export.json", "w") as f:
       json.dump(result, f)
   ```

2. **Import to Engram:**
   ```bash
   # Via MCP tool
   engram-cli call memory_import --data @memora_export.json
   ```

3. **Migrate images:**
   ```bash
   # First run dry-run to preview
   engram-cli call memory_migrate_images --dry_run true
   
   # Then run actual migration
   engram-cli call memory_migrate_images
   ```

### Schema Version

Engram v0.2.0 uses schema version 10, which includes:
- `memory_events` table for event tracking
- `shared_memories` table for multi-agent sharing
- `agent_sync_state` table for sync tracking

Migration is automatic on first run.

---

## Files Modified

### New Files
- `src/storage/image_storage.rs` - Image storage module (~400 lines)
- `src/intelligence/content_utils.rs` - Content utilities (~150 lines)

### Modified Files
- `src/storage/queries.rs` - Added ~800 lines of new query functions
- `src/storage/migrations.rs` - Added v10 migration
- `src/storage/mod.rs` - Added exports
- `src/mcp/tools.rs` - Added 24 new tool definitions
- `src/bin/server.rs` - Added 24 new tool handlers
- `Cargo.toml` - Added `dirs` dependency

### Test Coverage
- 258 tests passing
- New tests for image storage, content utilities

---

## Performance Considerations

1. **Batch Operations** - Use transactions for atomicity and performance
2. **Event System** - Events should be periodically cleaned up to prevent unbounded growth
3. **Image Storage** - Local storage avoids network latency; consider R2 for cloud deployments
4. **Embedding Cache** - Uses Arc<[f32]> for zero-copy sharing

---

## Future Enhancements

1. **R2/S3 Image Storage** - Add cloud image storage option
2. **Auto-linking Identities** - NER-based automatic identity detection
3. **Workspace Templates** - Pre-configured workspace settings
4. **Event Webhooks** - Push events to external systems
5. **Tiered Search Boost** - Boost permanent memories over daily

---

**Last Updated:** January 28, 2026  
**Total MCP Tools:** 77
