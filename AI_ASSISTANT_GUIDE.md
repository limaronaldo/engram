# AI Memory Systems Guide

**Purpose:** Complete reference for AI assistants to use Memora (Python) and Engram (Rust) for persistent memory management.

---

## Overview

This guide covers two memory systems that share the same feature set:

| System | Language | Best For | Tool Prefix |
|--------|----------|----------|-------------|
| **Memora** | Python | Cloud-first (D1, R2), quick setup | `memory_*` |
| **Engram** | Rust | Performance-critical, local-first | `engram_*` |

Both systems provide:
- Hybrid search (BM25 + semantic + fuzzy)
- Multi-workspace isolation
- Memory tiering (permanent/daily)
- Identity links (entity unification)
- Session transcript indexing
- Knowledge graph with cross-references
- Cloud sync (S3/R2)

---

## Quick Reference

### Most Used Tools

| Task | Memora | Engram |
|------|--------|--------|
| Save knowledge | `memory_create` | `engram_memory_create` |
| Search | `memory_hybrid_search` | `engram_memory_search` |
| Find related | `memory_related` | `engram_memory_related` |
| List memories | `memory_list` | `engram_memory_list` |
| Create TODO | `memory_create_todo` | `engram_memory_create_todo` |
| Create issue | `memory_create_issue` | `engram_memory_create_issue` |

---

## When to Create Memories

### ALWAYS create memories for:

**1. User Preferences**
```python
# Memora
memory_create(
    content="User prefers functional programming style over OOP",
    tags=["preference", "coding-style"]
)

# Engram
engram_memory_create(
    content="User prefers functional programming style over OOP",
    type="preference",
    tags=["coding-style"]
)
```

**2. Architectural Decisions**
```python
memory_create(
    content="Decision: Use PostgreSQL instead of MongoDB for ACID compliance",
    tags=["decision", "database", "architecture"]
)
```

**3. Project Context**
```python
memory_create(
    content="IBVI API uses FastAPI with Pydantic models, JWT auth, PostgreSQL",
    tags=["project", "ibvi-api", "stack"],
    workspace="ibvi-api"
)
```

**4. Bugs and Issues**
```python
memory_create_issue(
    content="Login fails when password contains special chars like & or #",
    status="open",
    severity="major",
    component="auth"
)
```

**5. TODOs and Tasks**
```python
memory_create_todo(
    content="Implement rate limiting for API endpoints",
    status="open",
    priority="medium",
    category="security"
)
```

**6. Important Learnings**
```python
memory_create(
    content="SQLite VACUUM requires 2x database size in free space",
    tags=["learning", "sqlite", "operations"]
)
```

### Use DAILY tier for temporary context:

```python
# Memora
memory_create_daily(
    content="Currently debugging auth issue - user session expires early",
    tags=["session-context"],
    ttl_hours=24
)

# Engram
engram_memory_create_daily(
    content="Currently debugging auth issue - user session expires early",
    tags=["session-context"],
    ttl_seconds=86400
)
```

---

## Search Strategies

### 1. Hybrid Search (Recommended Default)

Combines keyword matching (BM25) with semantic similarity. Best for most queries.

```python
# Memora
memory_hybrid_search(
    query="how to implement authentication",
    limit=10
)

# Engram
engram_memory_search(
    query="how to implement authentication",
    limit=10,
    strategy="hybrid"
)
```

### 2. Semantic Search

Best for conceptual queries where exact keywords may not match.

```python
# Memora
memory_semantic_search(
    query="user login problems",  # Will find "authentication issues" too
    limit=10
)

# Engram
engram_memory_search(
    query="user login problems",
    strategy="semantic",
    limit=10
)
```

### 3. Filtered Search

Narrow results by workspace, tags, tier, or date range.

```python
# By workspace
memory_hybrid_search(
    query="database setup",
    workspace="ibvi-api"
)

# By multiple workspaces
memory_hybrid_search(
    query="authentication",
    workspaces=["ibvi-api", "mbras-web"]
)

# By tier
memory_list(
    tier="permanent",
    workspace="ibvi-api"
)

# Exclude transcript chunks (default behavior)
memory_hybrid_search(
    query="architecture",
    include_transcripts=False  # default
)

# Include transcript chunks
memory_hybrid_search(
    query="what did we discuss about auth",
    include_transcripts=True
)

# By tags (AND logic)
memory_list(
    tags=["decision", "database"]
)

# By date range
memory_list(
    from_date="2026-01-01",
    to_date="2026-01-31"
)
```

### 4. Find Related Memories

Get cross-referenced memories based on similarity.

```python
memory_related(
    memory_id=42,
    limit=5
)
```

---

## Workspace Management

Workspaces isolate memories by project. Use them to avoid cross-project confusion.

### Creating Memories in Workspaces

```python
memory_create(
    content="MBRAS uses Next.js 14 with App Router",
    tags=["stack", "frontend"],
    workspace="mbras-web"
)
```

### Listing Workspaces

```python
# Memora
memory_workspace_list()

# Engram
engram_workspace_list()

# Returns:
# [
#   {"workspace": "ibvi-api", "memory_count": 142},
#   {"workspace": "mbras-web", "memory_count": 87},
#   {"workspace": "default", "memory_count": 23}
# ]
```

### Workspace-Scoped Operations

```python
# List memories in a workspace
memory_list(workspace="ibvi-api")

# Search within a workspace
memory_hybrid_search(query="auth", workspace="ibvi-api")

# Get workspace statistics
memory_workspace_stats(workspace="ibvi-api")
```

### Moving Memories Between Workspaces

```python
# Memora
memory_workspace_move(
    memory_ids=[1, 2, 3],
    target_workspace="archive"
)

# Engram
engram_workspace_move(
    id=1,
    workspace="archive"
)
```

---

## Memory Tiering

### Permanent vs Daily Memories

| Tier | Behavior | Use For |
|------|----------|---------|
| `permanent` | Never expires (default) | Decisions, knowledge, important context |
| `daily` | Auto-expires after TTL | Session notes, temporary context |

### Creating Daily Memories

```python
# Memora (ttl in hours)
memory_create_daily(
    content="Currently working on auth refactor - session issue",
    tags=["session-context"],
    ttl_hours=24
)

# Engram (ttl in seconds)
engram_memory_create_daily(
    content="Currently working on auth refactor - session issue",
    tags=["session-context"],
    ttl_seconds=86400
)
```

### Promoting to Permanent

If a daily memory turns out to be important:

```python
memory_promote_to_permanent(memory_id=42)
```

### Cleanup Expired

```python
memory_cleanup_expired()
# Returns: {"cleaned_count": 15}
```

---

## Identity Management

Track entities (people, projects, tools) with multiple aliases.

### Creating Identities

```python
# Memora
memory_identity_create(
    canonical_id="user:ronaldo",
    display_name="Ronaldo Lima",
    entity_type="person",
    aliases=["@ronaldo", "limaronaldo", "ronaldo@email.com"]
)

# Engram
engram_identity_create(
    canonical_id="user:ronaldo",
    display_name="Ronaldo Lima",
    entity_type="person",
    aliases=["@ronaldo", "limaronaldo", "ronaldo@email.com"]
)
```

### Linking Memories to Identities

```python
memory_identity_link(
    memory_id=42,
    identity_id="user:ronaldo",
    mention_text="Ronaldo"
)
```

### Searching by Identity

```python
# Memora
memory_search_by_identity(
    identity_id="user:ronaldo",
    include_aliases=True
)

# Engram
engram_memory_search_by_identity(
    identity="user:ronaldo",
    workspace="ibvi-api"  # optional
)
```

### Adding Aliases

```python
memory_identity_add_alias(
    canonical_id="user:ronaldo",
    alias="rlima",
    source="github"
)
```

---

## Session Indexing

Index long conversations for searchable chunks.

### Indexing a Conversation

```python
# Memora
memory_index_conversation(
    messages=[
        {"role": "user", "content": "How do I implement JWT auth?"},
        {"role": "assistant", "content": "Use jsonwebtoken library..."},
    ],
    session_id="session-abc123",
    chunk_size=10,
    overlap=2,
    tags=["session", "auth-discussion"]
)

# Engram
engram_session_index(
    session_id="session-abc123",
    messages=[
        {"role": "user", "content": "How do I implement JWT auth?"},
        {"role": "assistant", "content": "Use jsonwebtoken library..."},
    ],
    max_messages=10,
    max_chars=8000,
    overlap=2,
    ttl_days=7
)
```

### Delta Indexing (Add New Messages)

```python
# Memora
memory_index_conversation_delta(
    session_id="session-abc123",
    new_messages=[
        {"role": "user", "content": "What about refresh tokens?"},
        {"role": "assistant", "content": "Use httpOnly cookies..."}
    ]
)

# Engram
engram_session_index_delta(
    session_id="session-abc123",
    messages=[
        {"role": "user", "content": "What about refresh tokens?"},
        {"role": "assistant", "content": "Use httpOnly cookies..."}
    ]
)
```

### Searching Sessions

```python
# Memora
memory_session_search(query="refresh token security")

# Engram
engram_memory_session_search(query="refresh token security")
```

---

## TODOs and Issues

### Creating TODOs

```python
# Memora
memory_create_todo(
    content="Implement rate limiting for /api/search endpoint",
    status="open",      # open, in_progress, completed, blocked
    priority="high",    # low, medium, high, critical
    category="security"
)

# Engram
engram_memory_create_todo(
    content="Implement rate limiting for /api/search endpoint",
    priority="high",
    tags=["security"]
)
```

### Creating Issues

```python
# Memora
memory_create_issue(
    content="Login fails when password contains & character",
    status="open",      # open, in_progress, resolved, wont_fix
    severity="major",   # minor, major, critical, blocker
    component="auth"
)

# Engram
engram_memory_create_issue(
    title="Login fails with special characters",
    description="Password containing & character causes login failure",
    severity="high",
    tags=["auth", "bug"]
)
```

### Updating Status

```python
memory_update(
    memory_id=42,
    status="completed"
)
```

---

## Duplicate Detection and Merging

### Finding Duplicates

```python
# Memora
memory_find_duplicates(
    min_similarity=0.7,
    max_similarity=0.95,
    limit=10,
    use_llm=True
)

# Engram
engram_memory_find_duplicates(
    threshold=0.9
)
```

### Merging Duplicates

```python
# Memora
memory_merge(
    source_id=123,
    target_id=456,
    merge_strategy="append"
)

# Engram
engram_memory_merge(
    ids=[123, 456],
    keep_id=456
)
```

---

## Knowledge Graph

### Graph Traversal

```python
# Engram - full traversal control
engram_memory_traverse(
    id=42,
    depth=2,
    direction="both",  # outgoing, incoming, both
    edge_types=["implements", "depends_on"],
    include_entities=True
)

# Find path between memories
engram_memory_find_path(
    from_id=42,
    to_id=100,
    max_depth=5
)
```

### Linking Memories

```python
memory_link(
    from_id=42,
    to_id=43,
    edge_type="implements"  # related_to, supersedes, contradicts, implements, extends, references, depends_on, blocks, follows_up
)
```

### Export Graph

```python
memory_export_graph(
    format="html",
    max_nodes=500
)
```

---

## Document Ingestion (Engram Only)

```python
engram_memory_ingest_document(
    path="/path/to/document.pdf",
    format="auto",  # auto, md, pdf
    chunk_size=1200,
    overlap=200,
    tags=["documentation"]
)
```

---

## Project Context Discovery

Scan for AI instruction files (CLAUDE.md, .cursorrules, etc.)

```python
# Memora
memory_scan_project(
    path="/path/to/project",
    extract_sections=True,
    scan_parents=False
)

# Engram
engram_memory_scan_project(
    path="/path/to/project",
    extract_sections=True
)
```

---

## Multi-Agent Sync

### Get Sync Version

```python
# Memora
memory_sync_version()

# Engram
engram_sync_version()
```

### Get Changes Since Version

```python
# Memora
memory_sync_delta(
    since_version=35,
    agent_id="agent-1"
)

# Engram
engram_sync_delta(
    since_version=35
)
```

### Share Memory with Another Agent

```python
# Memora
memory_share(
    memory_id=42,
    source_agent="agent-1",
    target_agents=["agent-2", "agent-3"],
    message="Check out this finding"
)

# Engram
engram_memory_share(
    memory_id=42,
    from_agent="agent-1",
    to_agent="agent-2"
)
```

### Poll for Shared Memories

```python
memory_shared_poll(
    agent_id="agent-2",
    since_timestamp="2026-01-28T00:00:00"
)
```

---

## Embedding Cache

```python
# Get cache stats
memory_embedding_cache_stats()
# Returns: {"enabled": True, "hits": 12340, "misses": 567, "hit_rate": 0.956}

# Clear cache
memory_embedding_cache_clear()
```

---

## Best Practices

### 1. Use Descriptive Tags

```python
# Good
memory_create(content="...", tags=["decision", "database", "postgresql"])

# Not as useful
memory_create(content="...", tags=["db"])
```

### 2. Use Workspaces for Projects

```python
memory_create(content="...", workspace="ibvi-api")
memory_create(content="...", workspace="mbras-web")
```

### 3. Search Before Creating

Before creating a memory, search to avoid duplicates:

```python
existing = memory_hybrid_search(query="JWT authentication setup", limit=3)
if not existing["results"]:
    memory_create(content="JWT authentication setup: use RS256...")
```

### 4. Use Daily Tier for Transient Context

```python
memory_create_daily(
    content="Debugging session: checked auth.js line 45",
    ttl_hours=8
)
```

### 5. Track Identities for People and Projects

```python
memory_identity_create(
    canonical_id="project:ibvi",
    display_name="IBVI Project",
    entity_type="project",
    aliases=["IBVI", "ibvi-api", "IBVI API"]
)
```

### 6. Exclude Transcripts in Regular Search

Transcript chunks are excluded by default. Only include them when specifically searching conversation history:

```python
# Regular search (transcripts excluded)
memory_hybrid_search(query="authentication")

# Searching conversations (include transcripts)
memory_hybrid_search(query="what did we discuss", include_transcripts=True)
```

---

## Complete Tool Reference

### Core Memory Operations

| Tool | Description | Key Parameters |
|------|-------------|----------------|
| `memory_create` | Create a memory | `content`, `tags`, `workspace`, `tier`, `type` |
| `memory_get` | Get by ID | `id` |
| `memory_update` | Update memory | `id`, `content`, `tags` |
| `memory_delete` | Delete memory | `id` |
| `memory_list` | List with filters | `limit`, `tags`, `workspace`, `workspaces`, `tier`, `include_transcripts` |
| `memory_search` | Hybrid search | `query`, `workspace`, `workspaces`, `tier`, `include_transcripts`, `strategy` |
| `memory_list_compact` | Compact list | `limit`, `preview_chars` |
| `memory_create_batch` | Create multiple | `memories` |
| `memory_delete_batch` | Delete multiple | `ids` |

### Specialized Types

| Tool | Description |
|------|-------------|
| `memory_create_todo` | Create TODO with priority |
| `memory_create_issue` | Create issue with severity |
| `memory_create_section` | Section header |
| `memory_create_daily` | Auto-expiring memory |
| `memory_promote_to_permanent` | Daily → permanent |
| `memory_cleanup_expired` | Delete expired |

### Workspaces

| Tool | Description |
|------|-------------|
| `workspace_list` / `memory_workspace_list` | List all workspaces |
| `workspace_stats` / `memory_workspace_stats` | Statistics |
| `workspace_move` / `memory_workspace_move` | Move memories |
| `workspace_delete` / `memory_workspace_delete` | Delete workspace |

### Identities

| Tool | Description |
|------|-------------|
| `identity_create` / `memory_identity_create` | Create identity |
| `identity_get` / `memory_identity_get` | Get identity |
| `identity_update` / `memory_identity_update` | Update identity |
| `identity_delete` / `memory_identity_delete` | Delete identity |
| `identity_list` / `memory_identity_list` | List identities |
| `identity_search` / `memory_identity_search` | Search identities |
| `identity_add_alias` / `memory_identity_add_alias` | Add alias |
| `identity_link` / `memory_identity_link` | Link to memory |
| `identity_unlink` / `memory_identity_unlink` | Unlink |
| `memory_search_by_identity` | Find by identity |
| `memory_get_identities` | Get in memory |

### Sessions

| Tool | Description |
|------|-------------|
| `session_index` / `memory_index_conversation` | Index conversation |
| `session_index_delta` / `memory_index_conversation_delta` | Delta indexing |
| `session_get` / `memory_session_get` | Get session |
| `session_list` / `memory_session_list` | List sessions |
| `session_delete` / `memory_session_delete` | Delete session |
| `memory_session_search` | Search sessions |

### Linking & Graph

| Tool | Description |
|------|-------------|
| `memory_link` | Create link |
| `memory_unlink` | Remove link |
| `memory_related` | Get related |
| `memory_traverse` | Graph traversal (Engram) |
| `memory_find_path` | Find path (Engram) |
| `memory_clusters` | Find clusters |
| `memory_boost` | Increase importance |

### Maintenance

| Tool | Description |
|------|-------------|
| `memory_find_duplicates` | Find duplicates |
| `memory_merge` | Merge memories |
| `memory_rebuild_embeddings` | Recompute vectors |
| `memory_rebuild_crossrefs` | Recompute refs |
| `memory_stats` | Database stats |
| `memory_soft_trim` | Trimmed view |
| `memory_ingest_document` | Ingest PDF/MD (Engram) |

### Sync (Multi-Agent)

| Tool | Description |
|------|-------------|
| `sync_version` / `memory_sync_version` | Current version |
| `sync_delta` / `memory_sync_delta` | Changes since |
| `sync_state` / `memory_sync_state` | Agent state |
| `sync_cleanup` / `memory_sync_cleanup` | Clean deletions |
| `memory_share` | Share memory |
| `memory_shared_poll` | Poll shares |
| `memory_share_ack` | Acknowledge |

### Export/Import

| Tool | Description |
|------|-------------|
| `memory_export` | Export to JSON |
| `memory_import` | Import from JSON |
| `memory_export_graph` | Export HTML graph |

### Cache & Performance

| Tool | Description |
|------|-------------|
| `embedding_cache_stats` | Cache stats |
| `embedding_cache_clear` | Clear cache |

### Entity Extraction (Engram)

| Tool | Description |
|------|-------------|
| `memory_extract_entities` | Extract from memory |
| `memory_get_entities` | Get linked entities |
| `memory_search_entities` | Search entities |
| `memory_entity_stats` | Entity statistics |

### Versioning (Engram)

| Tool | Description |
|------|-------------|
| `memory_versions` | Version history |
| `memory_get_version` | Get specific version |
| `memory_revert` | Revert to version |

---

## Common Workflows

### Starting a New Session

1. Search for relevant context:
   ```python
   memory_hybrid_search(query="project:current-task setup", limit=5)
   ```

2. Check recent memories:
   ```python
   memory_list(limit=10, from_date="2026-01-28")
   ```

3. Create session context (daily tier):
   ```python
   memory_create_daily(content="Session goal: implement feature X")
   ```

### Ending a Session

1. Save important decisions as permanent:
   ```python
   memory_create(
       content="Decision: Use WebSocket for real-time updates",
       tags=["decision", "architecture"]
   )
   ```

2. Promote any important daily memories:
   ```python
   memory_promote_to_permanent(memory_id=42)
   ```

3. Index the conversation for future search:
   ```python
   memory_index_conversation(
       messages=[...],
       session_id="session-xyz",
       tags=["session"]
   )
   ```

### Researching a Topic

1. Search across all memories:
   ```python
   memory_hybrid_search(query="authentication implementation")
   ```

2. Include past conversations:
   ```python
   memory_hybrid_search(
       query="authentication discussion",
       include_transcripts=True
   )
   ```

3. Find related memories:
   ```python
   memory_related(memory_id=best_match_id)
   ```

4. Search by identity if relevant:
   ```python
   memory_search_by_identity(identity_id="project:auth-service")
   ```

### Project Handoff

1. List workspace contents:
   ```python
   memory_workspace_stats(workspace="project-name")
   ```

2. Export project memories:
   ```python
   memory_list(workspace="project-name", limit=100)
   ```

3. Search for key decisions:
   ```python
   memory_hybrid_search(query="decision", workspace="project-name")
   ```

---

## Configuration

### Memora MCP Config

```json
{
  "mcpServers": {
    "memory": {
      "command": "memora-server",
      "env": {
        "MEMORA_DB_PATH": "~/.local/share/memora/memories.db",
        "MEMORA_ALLOW_ANY_TAG": "1",
        "MEMORA_GRAPH_PORT": "8765"
      }
    }
  }
}
```

### Engram MCP Config

```json
{
  "mcpServers": {
    "engram": {
      "command": "/path/to/engram-server",
      "env": {
        "ENGRAM_DB_PATH": "~/.local/share/engram/memories.db",
        "ENGRAM_EMBEDDING_MODEL": "tfidf",
        "ENGRAM_CLEANUP_INTERVAL": "3600"
      }
    }
  }
}
```

### Cloud Sync (Both)

```json
{
  "env": {
    "AWS_PROFILE": "memora",
    "AWS_ENDPOINT_URL": "https://account.r2.cloudflarestorage.com",
    "MEMORA_STORAGE_URI": "s3://bucket/memories.db",
    "MEMORA_CLOUD_ENCRYPT": "true"
  }
}
```

---

## Feature Comparison

| Feature | Memora (Python) | Engram (Rust) |
|---------|-----------------|---------------|
| **Hybrid Search** | BM25 + Vector | BM25 + Vector + Fuzzy |
| **Memory Tiering** | Yes | Yes |
| **Multi-Workspace** | Yes | Yes |
| **Identity Links** | Yes | Yes |
| **Session Indexing** | Yes | Yes |
| **Knowledge Graph** | Yes | Yes |
| **Cloud Sync** | S3/R2/D1 | S3/R2 |
| **Document Ingestion** | No | Yes (PDF, MD) |
| **Memory Versioning** | No | Yes |
| **Entity Extraction** | No | Yes |
| **Graph Traversal** | Basic | Full (depth, direction, filters) |
| **Deduplication** | LLM-powered | Hash + Semantic |
| **Embedding Cache** | LRU | Arc-based (zero-copy) |
| **Auto-Tagging** | No | Yes |

---

**Last Updated:** January 29, 2026
