# Engram MCP Guide

**Purpose:** Complete reference for AI assistants using Engram via MCP (Model Context Protocol).

---

## Quick Start

Engram provides persistent memory for AI agents with hybrid search, knowledge graphs, and optional cloud sync.

### Most Used Tools

| Task | Tool | Example |
|------|------|---------|
| Save knowledge | `memory_create` | `memory_create(content="User prefers TypeScript")` |
| Search | `memory_search` | `memory_search(query="authentication setup")` |
| Find related | `memory_related` | `memory_related(id=42)` |
| List memories | `memory_list` | `memory_list(limit=20, workspace="my-project")` |
| Create TODO | `memory_create_todo` | `memory_create_todo(content="Fix bug", priority="high")` |
| Create issue | `memory_create_issue` | `memory_create_issue(title="Login fails")` |

---

## Core Concepts

### Workspaces
Isolate memories by project. Default workspace is `"default"`.

### Tiers
- **permanent**: Never expires (default)
- **daily**: Auto-expires after TTL

### Identities
Canonical entities with aliases for unified search across mentions.

### Sessions
Indexed conversation transcripts, chunked for efficient search.

---

## Memory CRUD

### memory_create

Store a new memory.

```json
{
  "content": "User prefers dark mode and TypeScript",
  "type": "preference",
  "tags": ["ui", "coding"],
  "workspace": "my-project",
  "tier": "permanent",
  "importance": 0.8,
  "metadata": {"source": "user-request"},
  "ttl_seconds": null,
  "dedup_mode": "allow",
  "dedup_threshold": 0.9
}
```

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `content` | string | required | The content to remember |
| `type` | enum | `"note"` | note, todo, issue, decision, preference, learning, context, credential |
| `tags` | array | `[]` | Tags for categorization |
| `workspace` | string | `"default"` | Workspace to store in |
| `tier` | enum | `"permanent"` | Memory tier: permanent (never expires) or daily (auto-expires) |
| `importance` | number | `0.5` | Importance score (0-1) |
| `metadata` | object | `{}` | Additional key-value pairs |
| `ttl_seconds` | integer | null | Time-to-live (null = permanent). Setting this implies tier='daily' |
| `dedup_mode` | enum | `"allow"` | reject, merge, skip, allow |
| `dedup_threshold` | number | null | Semantic similarity threshold for dedup |

### memory_get

Retrieve a memory by ID.

```json
{
  "id": 42
}
```

### memory_update

Update an existing memory.

```json
{
  "id": 42,
  "content": "Updated content",
  "tags": ["new-tag"],
  "importance": 0.9,
  "ttl_seconds": 0
}
```

**Note:** Setting `ttl_seconds: 0` to remove expiration only works for memories with `tier="permanent"`. For daily tier memories, you must first promote them using `memory_promote_to_permanent`.

### memory_delete

Delete a memory (soft delete).

```json
{
  "id": 42
}
```

### memory_list

List memories with filtering.

```json
{
  "limit": 20,
  "offset": 0,
  "tags": ["decision"],
  "type": "decision",
  "workspace": "my-project",
  "workspaces": ["project-a", "project-b"],
  "tier": "permanent",
  "sort_by": "created_at",
  "sort_order": "desc",
  "filter": {
    "AND": [
      {"importance": {"gte": 0.5}},
      {"metadata.reviewed": {"eq": true}}
    ]
  }
}
```

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `limit` | integer | 20 | Max results |
| `offset` | integer | 0 | Pagination offset |
| `tags` | array | null | Filter by tags (AND logic) |
| `type` | string | null | Filter by memory type |
| `workspace` | string | null | Filter by single workspace |
| `workspaces` | array | null | Filter by multiple workspaces |
| `tier` | enum | null | `"permanent"` or `"daily"` |
| `sort_by` | enum | `"created_at"` | created_at, updated_at, importance, access_count |
| `sort_order` | enum | `"desc"` | asc, desc |
| `filter` | object | null | Advanced filter with AND/OR logic |

**Note:** To exclude transcript chunks from results, filter by type or use the `filter` parameter to exclude `memory_type="transcript_chunk"`.

### memory_list_compact

List with preview only (more efficient for browsing).

```json
{
  "limit": 50,
  "workspace": "my-project",
  "tier": "permanent",
  "preview_chars": 100
}
```

---

## Search

### memory_search

Hybrid search combining keyword (BM25) and semantic similarity.

```json
{
  "query": "authentication JWT tokens",
  "limit": 10,
  "min_score": 0.1,
  "tags": ["auth"],
  "type": "decision",
  "workspace": "api-project",
  "workspaces": ["api-project", "shared"],
  "tier": "permanent",
  "include_transcripts": false,
  "strategy": "hybrid",
  "explain": true,
  "rerank": true,
  "rerank_strategy": "heuristic",
  "filter": {
    "importance": {"gte": 0.5}
  }
}
```

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `query` | string | required | Search query |
| `limit` | integer | 10 | Max results |
| `min_score` | number | 0.1 | Minimum relevance score |
| `workspace` | string | null | Filter by workspace |
| `workspaces` | array | null | Filter by multiple workspaces |
| `tier` | enum | null | Filter by tier |
| `include_transcripts` | boolean | false | Include transcript chunks |
| `strategy` | enum | auto | `"keyword"`, `"semantic"`, `"hybrid"` |
| `explain` | boolean | false | Include match explanations |
| `rerank` | boolean | true | Apply reranking |
| `rerank_strategy` | enum | `"heuristic"` | none, heuristic, multi_signal |

### memory_search_suggest

Get search suggestions and typo corrections.

```json
{
  "query": "authentcation"
}
```

### memory_search_by_identity

Search memories mentioning an identity by name or alias.

**Note:** This currently performs a content/tag LIKE search for the identity string, not a lookup in the identity table. For true identity-based search with alias resolution, use `identity_resolve` first to get the canonical ID, then search with that.

```json
{
  "identity": "user:ronaldo",
  "workspace": "my-project",
  "limit": 50
}
```

### memory_session_search

Search within session transcript chunks.

```json
{
  "query": "refresh token implementation",
  "session_id": "session-abc123",
  "workspace": "my-project",
  "limit": 20
}
```

---

## Specialized Memory Types

### memory_create_todo

```json
{
  "content": "Implement rate limiting",
  "priority": "high",
  "due_date": "2026-02-15",
  "tags": ["security", "api"]
}
```

Priority: `low`, `medium`, `high`, `critical`

### memory_create_issue

```json
{
  "title": "Login fails with special characters",
  "description": "Password containing & causes 500 error",
  "severity": "high",
  "tags": ["auth", "bug"]
}
```

Severity: `low`, `medium`, `high`, `critical`

### memory_create_daily

Create auto-expiring memory.

```json
{
  "content": "Currently debugging session timeout issue",
  "tags": ["session-context"],
  "ttl_seconds": 86400,
  "workspace": "my-project"
}
```

### memory_promote_to_permanent

Convert daily memory to permanent.

```json
{
  "id": 42
}
```

### memory_cleanup_expired

Delete all expired memories.

```json
{}
```

### memory_create_section

Create hierarchical section header.

```json
{
  "title": "Authentication",
  "content": "Auth system documentation",
  "parent_id": 10,
  "level": 2,
  "workspace": "docs"
}
```

### memory_checkpoint

Mark session checkpoint for resumption.

```json
{
  "session_id": "session-abc123",
  "summary": "Completed auth refactor, starting tests",
  "context": {"current_file": "auth.ts", "line": 45},
  "workspace": "my-project"
}
```

---

## Workspace Management

### workspace_list

List all workspaces with statistics.

```json
{}
```

Returns:
```json
[
  {"workspace": "my-project", "memory_count": 142, "permanent_count": 130, "daily_count": 12},
  {"workspace": "default", "memory_count": 23}
]
```

### workspace_stats

Detailed statistics for a workspace.

```json
{
  "workspace": "my-project"
}
```

### workspace_move

Move a memory to different workspace.

```json
{
  "id": 42,
  "workspace": "archive"
}
```

### workspace_delete

Delete workspace and handle memories.

```json
{
  "workspace": "old-project",
  "move_to_default": true
}
```

Set `move_to_default: false` to delete all memories in workspace.

---

## Identity Management

### identity_create

```json
{
  "canonical_id": "user:ronaldo",
  "display_name": "Ronaldo Lima",
  "entity_type": "person",
  "description": "Lead developer",
  "aliases": ["@ronaldo", "limaronaldo", "ronaldo@email.com"],
  "metadata": {"github": "limaronaldo"}
}
```

Entity types: `person`, `organization`, `project`, `tool`, `concept`, `other`

### identity_get

```json
{
  "canonical_id": "user:ronaldo"
}
```

### identity_update

```json
{
  "canonical_id": "user:ronaldo",
  "display_name": "Ronaldo M. Lima",
  "description": "Senior developer"
}
```

### identity_delete

```json
{
  "canonical_id": "user:ronaldo"
}
```

### identity_add_alias

```json
{
  "canonical_id": "user:ronaldo",
  "alias": "rlima",
  "source": "github"
}
```

### identity_remove_alias

```json
{
  "alias": "rlima"
}
```

### identity_resolve

Resolve alias to canonical identity.

```json
{
  "alias": "@ronaldo"
}
```

### identity_list

```json
{
  "entity_type": "person",
  "limit": 50
}
```

### identity_search

```json
{
  "query": "ronaldo",
  "limit": 20
}
```

### identity_link

Link identity to memory.

```json
{
  "memory_id": 42,
  "canonical_id": "user:ronaldo",
  "mention_text": "Ronaldo reviewed this"
}
```

### identity_unlink

```json
{
  "memory_id": 42,
  "canonical_id": "user:ronaldo"
}
```

---

## Session Transcript Indexing

### session_index

Index conversation with chunking.

```json
{
  "session_id": "session-abc123",
  "messages": [
    {"role": "user", "content": "How do I implement JWT?", "timestamp": "2026-01-29T10:00:00Z"},
    {"role": "assistant", "content": "Use the jsonwebtoken library..."}
  ],
  "title": "JWT Implementation Discussion",
  "workspace": "api-project",
  "agent_id": "claude-1",
  "max_messages": 10,
  "max_chars": 8000,
  "overlap": 2,
  "ttl_days": 7
}
```

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `session_id` | string | required | Unique session identifier |
| `messages` | array | required | Conversation messages |
| `title` | string | null | Session title |
| `workspace` | string | `"default"` | Workspace for chunks |
| `max_messages` | integer | 10 | Max messages per chunk |
| `max_chars` | integer | 8000 | Max characters per chunk |
| `overlap` | integer | 2 | Overlap messages between chunks |
| `ttl_days` | integer | 7 | TTL for transcript chunks |

### session_index_delta

Add new messages incrementally.

```json
{
  "session_id": "session-abc123",
  "messages": [
    {"role": "user", "content": "What about refresh tokens?"},
    {"role": "assistant", "content": "Store in httpOnly cookies..."}
  ]
}
```

### session_get

```json
{
  "session_id": "session-abc123"
}
```

### session_list

```json
{
  "workspace": "api-project",
  "limit": 20
}
```

### session_delete

```json
{
  "session_id": "session-abc123"
}
```

---

## Knowledge Graph

### memory_link

Create cross-reference between memories.

```json
{
  "from_id": 42,
  "to_id": 43,
  "edge_type": "implements",
  "strength": 0.9,
  "source_context": "Feature implements design decision",
  "pinned": false
}
```

Edge types: `related_to`, `supersedes`, `contradicts`, `implements`, `extends`, `references`, `depends_on`, `blocks`, `follows_up`

### memory_unlink

```json
{
  "from_id": 42,
  "to_id": 43,
  "edge_type": "implements"
}
```

### memory_related

Get related memories.

```json
{
  "id": 42,
  "depth": 2,
  "include_entities": true,
  "edge_type": "implements",
  "include_decayed": false
}
```

### memory_traverse

Full graph traversal control.

```json
{
  "id": 42,
  "depth": 3,
  "direction": "both",
  "edge_types": ["implements", "depends_on"],
  "min_score": 0.3,
  "min_confidence": 0.5,
  "limit_per_hop": 50,
  "include_entities": true
}
```

Direction: `outgoing`, `incoming`, `both`

### memory_find_path

Find shortest path between memories.

```json
{
  "from_id": 42,
  "to_id": 100,
  "max_depth": 5
}
```

### memory_boost

Increase memory importance.

```json
{
  "id": 42,
  "boost_amount": 0.2,
  "duration_seconds": 86400
}
```

### memory_clusters

Find clusters of related memories.

```json
{
  "min_similarity": 0.7,
  "min_cluster_size": 2
}
```

### memory_export_graph

Export knowledge graph visualization.

```json
{
  "format": "html",
  "max_nodes": 500,
  "focus_id": 42
}
```

---

## Entity Extraction

### memory_extract_entities

Extract named entities from memory.

```json
{
  "id": 42
}
```

### memory_get_entities

Get entities linked to memory.

```json
{
  "id": 42
}
```

### memory_search_entities

Search entities by name.

```json
{
  "query": "ronaldo",
  "entity_type": "person",
  "limit": 20
}
```

### memory_entity_stats

Get entity statistics.

```json
{}
```

---

## Document Ingestion

### memory_ingest_document

Ingest PDF or Markdown document.

```json
{
  "path": "/path/to/document.pdf",
  "format": "auto",
  "chunk_size": 1200,
  "overlap": 200,
  "max_file_size": 10485760,
  "tags": ["documentation", "api"]
}
```

Format: `auto`, `md`, `pdf`

---

## Versioning

### memory_versions

Get version history.

```json
{
  "id": 42
}
```

### memory_get_version

Get specific version.

```json
{
  "id": 42,
  "version": 3
}
```

### memory_revert

Revert to previous version.

```json
{
  "id": 42,
  "version": 3
}
```

---

## Duplicates & Merging

### memory_find_duplicates

```json
{
  "threshold": 0.9
}
```

### memory_merge

```json
{
  "ids": [42, 43, 44],
  "keep_id": 42
}
```

---

## Batch Operations

### memory_create_batch

```json
{
  "memories": [
    {"content": "First memory", "tags": ["batch"]},
    {"content": "Second memory", "tags": ["batch"], "workspace": "project-a"}
  ]
}
```

### memory_delete_batch

```json
{
  "ids": [42, 43, 44]
}
```

---

## Content Utilities

### memory_soft_trim

Intelligently trim content preserving head and tail.

```json
{
  "id": 42,
  "max_chars": 500,
  "head_percent": 60,
  "tail_percent": 30,
  "ellipsis": "\n...\n",
  "preserve_words": true
}
```

### memory_content_stats

Get content statistics.

```json
{
  "id": 42
}
```

Returns: character count, word count, line count, sentence count, paragraph count.

---

## Tags

### memory_tags

List all tags with usage counts.

```json
{}
```

### memory_tag_hierarchy

Get tags as hierarchical tree.

```json
{}
```

### memory_validate_tags

Validate tag consistency.

```json
{}
```

### memory_suggest_tags

AI-powered tag suggestions.

```json
{
  "id": 42,
  "min_confidence": 0.5,
  "max_tags": 5
}
```

---

## Sync & Multi-Agent

### sync_version

Get current sync version.

```json
{}
```

### sync_delta

Get changes since version.

```json
{
  "since_version": 100
}
```

### sync_state

Get/update agent sync state.

```json
{
  "agent_id": "claude-1",
  "update_version": 150
}
```

### sync_cleanup

Clean old sync data.

```json
{
  "older_than_days": 30
}
```

### memory_share

Share memory with another agent.

```json
{
  "memory_id": 42,
  "from_agent": "claude-1",
  "to_agent": "claude-2",
  "message": "Check this important finding"
}
```

### memory_shared_poll

Poll for shared memories.

```json
{
  "agent_id": "claude-2",
  "include_acknowledged": false
}
```

### memory_share_ack

Acknowledge shared memory.

```json
{
  "share_id": 123,
  "agent_id": "claude-2"
}
```

---

## Events

### memory_events_poll

Poll for memory events.

```json
{
  "since_id": 100,
  "since_time": "2026-01-28T00:00:00Z",
  "agent_id": "claude-1",
  "limit": 100
}
```

### memory_events_clear

Clear old events.

```json
{
  "before_id": 50,
  "keep_recent": 1000
}
```

---

## Cloud Sync

### memory_sync_status

Get cloud sync status.

```json
{}
```

---

## Embedding Cache

**Note:** These tools are currently placeholders and return zeros. The embedding cache is implemented internally but not yet exposed through these MCP tools.

### embedding_cache_stats

```json
{}
```

Returns: enabled, hits, misses, hit_rate, entries, bytes. (Currently returns placeholder values)

### embedding_cache_clear

```json
{}
```

(Currently a no-op)

---

## Maintenance

### memory_rebuild_embeddings

Rebuild all embeddings.

```json
{}
```

### memory_rebuild_crossrefs

Rebuild cross-reference links.

```json
{}
```

### memory_stats

Get database statistics.

```json
{}
```

### memory_quality_report

Get quality report.

```json
{
  "limit": 20,
  "min_quality": 0.5
}
```

### memory_aggregate

Aggregate by field.

```json
{
  "group_by": "type",
  "metrics": ["count", "avg_importance"]
}
```

---

## Import/Export

### memory_export

```json
{
  "workspace": "my-project",
  "include_embeddings": false
}
```

### memory_import

```json
{
  "data": {...},
  "skip_duplicates": true
}
```

---

## Project Context

### memory_scan_project

Scan for AI instruction files.

```json
{
  "path": "/path/to/project",
  "scan_parents": false,
  "extract_sections": true
}
```

Discovers: CLAUDE.md, AGENTS.md, .cursorrules, .github/copilot-instructions.md, etc.

### memory_get_project_context

Get indexed project context.

```json
{
  "path": "/path/to/project",
  "include_sections": true,
  "file_types": ["claude-md", "cursorrules"]
}
```

---

## Images

### memory_upload_image

```json
{
  "memory_id": 42,
  "file_path": "/path/to/image.png",
  "image_index": 0,
  "caption": "Screenshot of the error"
}
```

### memory_migrate_images

Migrate base64 images to file storage.

```json
{
  "dry_run": true
}
```

---

## TTL & Expiration

### memory_set_expiration

```json
{
  "id": 42,
  "ttl_seconds": 86400
}
```

**Important:** Setting `ttl_seconds: 0` to remove expiration only works for memories with `tier="permanent"`. For daily tier memories, this will return an error. To convert a daily memory to permanent, use `memory_promote_to_permanent` instead.

### memory_embedding_status

Check if embedding is computed.

```json
{
  "id": 42
}
```

---

## Advanced Filter Syntax

The `filter` parameter in `memory_list` and `memory_search` supports:

### Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `eq` | Equals | `{"type": {"eq": "decision"}}` |
| `neq` | Not equals | `{"type": {"neq": "note"}}` |
| `gt` | Greater than | `{"importance": {"gt": 0.5}}` |
| `gte` | Greater or equal | `{"importance": {"gte": 0.5}}` |
| `lt` | Less than | `{"importance": {"lt": 0.5}}` |
| `lte` | Less or equal | `{"importance": {"lte": 0.5}}` |
| `contains` | Contains substring | `{"content": {"contains": "auth"}}` |
| `not_contains` | Doesn't contain | `{"content": {"not_contains": "test"}}` |
| `exists` | Field exists | `{"metadata.reviewed": {"exists": true}}` |

### Logical Operators

```json
{
  "AND": [
    {"workspace": {"eq": "my-project"}},
    {"tier": {"eq": "permanent"}},
    {"importance": {"gte": 0.5}}
  ]
}
```

```json
{
  "OR": [
    {"type": {"eq": "decision"}},
    {"type": {"eq": "preference"}}
  ]
}
```

### Filterable Fields

- `content`
- `memory_type` / `type`
- `importance`
- `tags`
- `workspace`
- `tier`
- `created_at`
- `updated_at`
- `metadata.*` (any metadata field)

---

## MCP Configuration

### Local Storage

```json
{
  "mcpServers": {
    "engram": {
      "command": "/path/to/engram-server",
      "args": [],
      "env": {
        "ENGRAM_DB_PATH": "~/.local/share/engram/memories.db",
        "ENGRAM_EMBEDDING_MODEL": "tfidf",
        "ENGRAM_CLEANUP_INTERVAL": "3600"
      }
    }
  }
}
```

### Cloud Sync (S3/R2)

```json
{
  "mcpServers": {
    "engram": {
      "command": "/path/to/engram-server",
      "args": [],
      "env": {
        "AWS_PROFILE": "engram",
        "AWS_ENDPOINT_URL": "https://account.r2.cloudflarestorage.com",
        "ENGRAM_STORAGE_URI": "s3://bucket/memories.db",
        "ENGRAM_CLOUD_ENCRYPT": "true",
        "ENGRAM_EMBEDDING_MODEL": "tfidf"
      }
    }
  }
}
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ENGRAM_DB_PATH` | `~/.local/share/engram/memories.db` | Database path |
| `ENGRAM_STORAGE_MODE` | `local` | `local` or `cloud-safe` |
| `ENGRAM_STORAGE_URI` | null | Cloud URI (s3://bucket/path) |
| `ENGRAM_CLOUD_ENCRYPT` | false | Enable AES-256 encryption |
| `ENGRAM_EMBEDDING_MODEL` | `tfidf` | `tfidf` or `openai` |
| `OPENAI_API_KEY` | null | For OpenAI embeddings |
| `ENGRAM_SYNC_DEBOUNCE_MS` | 5000 | Sync debounce interval |
| `ENGRAM_CLEANUP_INTERVAL` | 3600 | Auto-cleanup interval (seconds) |
| `ENGRAM_CONFIDENCE_HALF_LIFE` | 30 | Confidence decay half-life (days) |
| `ENGRAM_WS_PORT` | 0 | WebSocket port (0 = disabled) |

---

## Best Practices

### 1. Use Workspaces for Projects
```json
{"content": "...", "workspace": "ibvi-api"}
```

### 2. Use Descriptive Tags
```json
{"tags": ["decision", "database", "postgresql"]}
```

### 3. Search Before Creating
Avoid duplicates by searching first.

### 4. Use Daily Tier for Transient Context
```json
{"content": "Debugging...", "ttl_seconds": 28800}
```

### 5. Exclude Transcripts in Regular Search
Transcripts are excluded by default. Include only when searching conversations.

### 6. Track Identities for People/Projects
Create canonical identities with aliases for unified search.

### 7. Index Important Conversations
Use session indexing to make conversations searchable.

---

**Last Updated:** January 29, 2026
**Engram Version:** 0.1.0
