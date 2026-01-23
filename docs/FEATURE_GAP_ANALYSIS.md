# Feature Gap Analysis: Engram vs Competitors

## Executive Summary

This document analyzes features from Mem0, Letta, Cognee, and Zep/Graphiti to identify what Engram should incorporate to be competitive or superior.

**Key Finding:** Engram has strong foundations (Rust, hybrid search, MCP-native) but lacks several production-critical features that competitors offer.

---

## Competitor Feature Matrix

### Memory Types & Scoping

| Feature | Mem0 | Letta | Cognee | Zep | Engram | Priority |
|---------|------|-------|--------|-----|--------|----------|
| User Memory | Yes | Yes | Yes | Yes | Partial | **P1** |
| Session Memory | Yes | Yes | No | Yes | No | **P1** |
| Agent Memory | Yes | Yes | No | No | No | **P2** |
| Core Memory (always in context) | No | Yes | No | No | No | **P2** |
| Archival Memory (cold storage) | Yes | Yes | Yes | Yes | Partial | P3 |

**Gap:** Engram has no explicit memory scoping (user/session/agent). All memories are flat.

**Recommendation:** Add `scope` field with values: `user`, `session`, `agent`, `global`

```rust
pub enum MemoryScope {
    User(String),      // user_id
    Session(String),   // session_id
    Agent(String),     // agent_id
    Global,
}
```

---

### Memory Operations

| Feature | Mem0 | Letta | Cognee | Zep | Engram | Priority |
|---------|------|-------|--------|-----|--------|----------|
| Add/Create | Yes | Yes | Yes | Yes | Yes | - |
| Search (semantic) | Yes | Yes | Yes | Yes | Yes | - |
| Search (keyword) | Yes | No | Yes | Yes | Yes | - |
| Search (hybrid) | Yes | No | Yes | Yes | **Yes** | - |
| Update | Yes | Yes | Yes | Yes | Yes | - |
| Delete | Yes | Yes | Yes | Yes | Yes | - |
| Bulk Delete | Yes | Yes | No | Yes | No | **P2** |
| Memory Expiration (TTL) | Yes | No | No | Yes | No | **P1** |
| Memory Categories | Yes | Yes | No | Yes | Tags only | **P2** |
| Memory Deduplication | Yes | No | Yes | Yes | No | **P1** |

**Gap:** Missing TTL, bulk operations, and automatic deduplication.

**Recommendation:**
1. Add `expires_at` field with automatic cleanup
2. Add `memory_delete_bulk` with filter support
3. Add deduplication on create (content hash + similarity check)

---

### Graph & Relationships

| Feature | Mem0 | Letta | Cognee | Zep/Graphiti | Engram | Priority |
|---------|------|-------|--------|--------------|--------|----------|
| Cross-references | Yes | No | Yes | Yes | Yes | - |
| Entity extraction | Yes | No | Yes | Yes | No | **P1** |
| Relationship types | Basic | No | Rich | Rich | Basic | **P2** |
| Multi-hop queries | Yes | No | Yes | Yes | No | **P1** |
| Temporal edges | No | No | No | **Yes** | No | **P2** |
| Graph traversal | Basic | No | Yes | Yes | Basic | P3 |
| Community detection | No | No | Yes | Yes | No | P3 |

**Gap:** No automatic entity extraction or multi-hop queries.

**Recommendation:**
1. Add entity extraction on memory create (NER via LLM or spaCy)
2. Store entities in separate table with links to memories
3. Add `memory_search` with `depth` parameter for multi-hop

```sql
CREATE TABLE entities (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    entity_type TEXT,  -- person, org, concept, etc.
    created_at TIMESTAMP
);

CREATE TABLE memory_entities (
    memory_id INTEGER REFERENCES memories(id),
    entity_id INTEGER REFERENCES entities(id),
    relation TEXT,  -- mentions, defines, references
    PRIMARY KEY (memory_id, entity_id)
);
```

---

### Temporal Features (from Graphiti)

| Feature | Graphiti | Engram | Priority |
|---------|----------|--------|----------|
| Bi-temporal model | Yes | No | **P2** |
| Event time tracking | Yes | No | **P2** |
| Point-in-time queries | Yes | No | **P2** |
| Temporal edge invalidation | Yes | No | P3 |
| Contradiction handling | Yes | No | **P2** |

**Gap:** Engram has `created_at` and `updated_at` but no event time or temporal queries.

**Recommendation:**
1. Add `event_time` field (when the fact occurred, not when stored)
2. Add `valid_from` / `valid_to` for temporal validity
3. Support queries like "what did we know about X on date Y"

```rust
pub struct TemporalMemory {
    pub event_time: Option<DateTime<Utc>>,  // When fact occurred
    pub valid_from: DateTime<Utc>,          // When became true
    pub valid_to: Option<DateTime<Utc>>,    // When became false
}
```

---

### Self-Editing Memory (from Letta/MemGPT)

| Feature | Letta | Engram | Priority |
|---------|-------|--------|----------|
| Agent can edit own memory | Yes | No | **P1** |
| Memory blocks (pinned context) | Yes | No | **P2** |
| Recursive summarization | Yes | No | **P2** |
| Heartbeat mechanism | Yes | No | P3 |
| Sleep-time compute | Yes | No | P3 |

**Gap:** Engram is passive storage. Agents can't self-edit or manage their own memory.

**Recommendation:**
1. Add `memory_replace` tool for in-place edits
2. Add `memory_summarize` to consolidate related memories
3. Add "pinned" memories that always appear in context

```rust
// New MCP tools
pub async fn memory_replace(
    memory_id: i64,
    new_content: String,
    reason: String,  // Why the agent is editing
) -> Result<Memory>;

pub async fn memory_summarize(
    memory_ids: Vec<i64>,
    summary: String,
) -> Result<Memory>;  // Returns new consolidated memory
```

---

### Search Enhancements

| Feature | Mem0 | Cognee | Zep | Engram | Priority |
|---------|------|--------|-----|--------|----------|
| Hybrid (BM25 + Vector) | Yes | Yes | Yes | **Yes** | - |
| Fuzzy/Typo tolerance | No | No | No | **Yes** | - |
| Reranking | Yes | Yes | Yes | No | **P1** |
| Keyword expansion | Yes | No | No | No | **P2** |
| Metadata filters | Yes | Yes | Yes | Partial | **P1** |
| Explain scores | No | Yes | No | No | **P2** |

**Gap:** No reranking model, limited metadata filtering.

**Recommendation:**
1. Add reranker (Cohere, cross-encoder, or local model)
2. Expand metadata filter syntax: `metadata.project = "engram" AND metadata.type = "decision"`
3. Add `explain: true` to show why results matched

---

### Production Features

| Feature | Mem0 | Letta | Zep | Engram | Priority |
|---------|------|-------|-----|--------|----------|
| Webhooks | Yes | Yes | Yes | No | **P2** |
| Async operations | Yes | Yes | Yes | Partial | P3 |
| Rate limiting | Cloud | Cloud | Cloud | No | P2 |
| Usage tracking | Yes | Yes | Yes | Basic | P2 |
| Audit logging | Yes | Yes | Yes | No | **P2** |
| Multi-tenant | Yes | Yes | Yes | Cloud only | P2 |

**Gap:** Missing webhooks, audit logging.

**Recommendation:**
1. Add webhook support for memory events (created, updated, deleted)
2. Add audit log table for compliance

```rust
pub struct Webhook {
    pub url: String,
    pub events: Vec<WebhookEvent>,  // Created, Updated, Deleted, Searched
    pub secret: String,             // For HMAC signature
}
```

---

### Data Ingestion (from Cognee)

| Feature | Cognee | Engram | Priority |
|---------|--------|--------|----------|
| Document ingestion | Yes | No | **P1** |
| PDF parsing | Yes | No | **P1** |
| Audio transcription | Yes | No | P3 |
| Image understanding | Yes | No | P3 |
| Chunking strategies | Yes | No | **P2** |
| Source tracking | Yes | Partial | **P2** |

**Gap:** Engram only accepts text. No document/PDF support.

**Recommendation:**
1. Add `memory_ingest` tool for documents
2. Support PDF, Markdown, HTML
3. Track source file, page, line number in metadata

---

## Priority Feature List

### P1 - Must Have (Competitive Parity)

| Feature | Effort | Impact |
|---------|--------|--------|
| Memory scoping (user/session/agent) | Medium | High |
| Memory expiration (TTL) | Low | Medium |
| Memory deduplication | Medium | High |
| Entity extraction | High | High |
| Multi-hop graph queries | Medium | High |
| Reranking in search | Medium | High |
| Agent self-editing tools | Medium | High |
| Document ingestion (PDF, MD) | High | High |
| Full metadata filter syntax | Low | Medium |

### P2 - Should Have (Differentiation)

| Feature | Effort | Impact |
|---------|--------|--------|
| Temporal queries (point-in-time) | Medium | Medium |
| Memory blocks (pinned context) | Medium | Medium |
| Webhooks | Low | Medium |
| Audit logging | Low | Medium |
| Keyword expansion | Medium | Low |
| Search explain | Low | Medium |
| Bulk operations | Low | Medium |
| Custom categories | Low | Medium |
| Recursive summarization | High | Medium |

### P3 - Nice to Have (Future)

| Feature | Effort | Impact |
|---------|--------|--------|
| Sleep-time compute | Very High | Medium |
| Community detection | High | Low |
| Audio transcription | High | Low |
| Image understanding | High | Low |
| Heartbeat mechanism | Medium | Low |

---

## Implementation Roadmap

### Phase 1: Core Competitive Features (4-6 weeks)

1. **Memory Scoping**
   - Add `scope` enum: user, session, agent, global
   - Update all queries to filter by scope
   - Add scope to MCP tools

2. **Entity Extraction**
   - Extract entities on memory create
   - Store in entities table
   - Link to memories

3. **Reranking**
   - Add optional reranker step after hybrid search
   - Support local cross-encoder or API (Cohere)

4. **Document Ingestion**
   - Add `memory_ingest_document` tool
   - Support PDF, Markdown, HTML
   - Chunking with overlap

### Phase 2: Advanced Features (4-6 weeks)

1. **Self-Editing Memory**
   - `memory_replace` tool
   - `memory_summarize` tool
   - Pinned memory blocks

2. **Temporal Queries**
   - Add `event_time`, `valid_from`, `valid_to`
   - Point-in-time query syntax

3. **Multi-hop Queries**
   - Graph traversal with depth limit
   - Relationship-aware search

4. **Production Hardening**
   - Webhooks
   - Audit logging
   - TTL cleanup job

---

## Unique Engram Advantages to Preserve

While adding features, maintain these differentiators:

1. **Rust Performance** - Keep single binary, no runtime deps
2. **MCP-Native** - First-class MCP support, not plugin
3. **SQLite/Edge** - Works offline, edge deployable
4. **Hybrid Search** - BM25 + vectors + fuzzy in one call
5. **Project Context** - CLAUDE.md/AGENTS.md ingestion
6. **Local-First** - No cloud required

---

## Conclusion

To compete with Mem0/Letta/Cognee/Zep, Engram needs:

1. **Memory scoping** (user/session/agent) - table stakes
2. **Entity extraction** - enables graph queries
3. **Reranking** - improves search quality
4. **Document ingestion** - expands use cases
5. **Self-editing tools** - enables agentic memory management

With these additions, Engram can claim:
- "Mem0's features + Rust performance + MCP-native"
- "Letta's self-editing memory in a single binary"
- "Cognee's graph capabilities without Neo4j"

---

*Last updated: January 2026*
