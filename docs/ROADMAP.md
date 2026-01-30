# Engram Roadmap

**Date:** January 29, 2026  
**Version:** 1.0  
**Current Schema:** v4  
**Current MCP Tools:** 92

---

## Table of Contents

1. [Overview](#overview)
2. [Progress Summary](#progress-summary)
3. [Phase 0: Storage Abstraction](#phase-0-storage-abstraction)
4. [Phase 1: Cognitive Memory Types](#phase-1-cognitive-memory-types)
5. [Phase 2: Context Compression](#phase-2-context-compression)
6. [Phase 3: Langfuse Integration](#phase-3-langfuse-integration)
7. [Phase 4: Search Result Caching](#phase-4-search-result-caching)
8. [Phase 5: Memory Lifecycle](#phase-5-memory-lifecycle)
9. [Phase 6: Turso Support](#phase-6-turso-support)
10. [Phase 7: Meilisearch](#phase-7-meilisearch)
11. [Phase 8: Salience & Session Memory](#phase-8-salience--session-memory)
12. [Phase 9: Context Quality](#phase-9-context-quality)
13. [Tool Growth Projection](#tool-growth-projection)
14. [Schema Evolution](#schema-evolution)

---

## Overview

This roadmap outlines Engram's planned evolution from a robust memory system (v0.2.0) to a comprehensive cognitive memory platform (v1.0+). The phases are ordered by dependency, not strictly by priority.

### Guiding Principles

1. **Stability First:** Each phase must not break existing functionality
2. **Incremental Value:** Every phase delivers usable features
3. **Performance Aware:** New features must maintain <100ms p95 latency
4. **Schema Forward:** Database migrations are additive, not destructive

---

## Progress Summary

| Phase | Name | Status | Issues | Tools Added |
|-------|------|--------|--------|-------------|
| 0 | Storage Abstraction | In Progress | 19 | +12 |
| 1 | Cognitive Memory Types | Planned | 5 | +8 |
| 2 | Context Compression | Planned | 4 | +3 |
| 3 | Langfuse Integration | Planned | 3 | +4 |
| 4 | Search Result Caching | Planned | 4 | +2 |
| 5 | Memory Lifecycle | Planned | 5 | +6 |
| 6 | Turso Support | Planned | 4 | +0 |
| 7 | Meilisearch | Planned | 5 | +4 |
| 8 | Salience & Session Memory | Planned | 12 | +8 |
| 9 | Context Quality | Planned | 19 | +10 |

**Totals:**
- Current: 92 MCP tools
- Projected: 126+ MCP tools
- Schema: v4 -> v15

---

## Phase 0: Storage Abstraction

**Status:** In Progress  
**Linear Issues:** ENG-14 to ENG-38 (19 issues)  
**Target Schema:** v5

### Objective

Decouple storage operations from SQLite specifics to enable multiple backends (Turso, PostgreSQL, etc.) without code duplication.

### Issues

| Issue | Title | Priority | Status |
|-------|-------|----------|--------|
| ENG-14 | Define StorageBackend trait | P0 | Done |
| ENG-15 | Implement SQLite backend | P0 | Done |
| ENG-16 | Abstract connection pooling | P0 | In Progress |
| ENG-17 | Generic transaction wrapper | P0 | In Progress |
| ENG-18 | Migrate memory CRUD to trait | P1 | Pending |
| ENG-19 | Migrate search queries to trait | P1 | Pending |
| ENG-20 | Migrate graph queries to trait | P1 | Pending |
| ENG-21 | Migrate entity queries to trait | P1 | Pending |
| ENG-22 | Backend configuration system | P1 | Pending |
| ENG-23 | Connection string parsing | P1 | Pending |
| ENG-24 | Backend health checks | P2 | Pending |
| ENG-25 | Query logging abstraction | P2 | Pending |
| ENG-26 | Metrics per backend | P2 | Pending |
| ENG-27 | Migration runner abstraction | P1 | Pending |
| ENG-28 | Test harness for backends | P1 | Pending |
| ENG-29 | SQLite WAL configuration | P2 | Pending |
| ENG-30 | Connection timeout handling | P2 | Pending |
| ENG-31 | Retry logic for transient failures | P2 | Pending |
| ENG-32 | Backend capability detection | P2 | Pending |

### New MCP Tools

| Tool | Description |
|------|-------------|
| `storage_backend_info` | Get current backend type and capabilities |
| `storage_health_check` | Check backend connectivity |
| `storage_metrics` | Get storage performance metrics |

---

## Phase 1: Cognitive Memory Types

**Status:** Planned  
**Linear Issues:** ENG-33 to ENG-37 (5 issues)  
**Target Schema:** v8

### Objective

Implement memory types inspired by human cognition: episodic (events), semantic (facts), and procedural (how-to).

### Issues

| Issue | Title | Priority | Status |
|-------|-------|----------|--------|
| ENG-33 | Episodic memory schema | P0 | Pending |
| ENG-34 | Semantic memory extraction | P0 | Pending |
| ENG-35 | Procedural memory format | P1 | Pending |
| ENG-36 | Memory type inference | P1 | Pending |
| ENG-37 | Cross-type linking | P2 | Pending |

### Memory Type Definitions

**Episodic Memory:**
- Specific events with temporal context
- "What happened when X"
- Includes: session transcripts, error occurrences, deployment events

**Semantic Memory:**
- General facts and concepts
- "What is X"
- Includes: API documentation, architecture decisions, naming conventions

**Procedural Memory:**
- Step-by-step processes
- "How to do X"
- Includes: deployment procedures, debugging workflows, setup guides

### New MCP Tools

| Tool | Description |
|------|-------------|
| `memory_create_episodic` | Create event-based memory |
| `memory_create_semantic` | Create fact-based memory |
| `memory_create_procedural` | Create how-to memory |
| `memory_infer_type` | Auto-detect memory type |
| `memory_search_episodic` | Search events by time range |
| `memory_search_semantic` | Search facts by concept |
| `memory_search_procedural` | Search procedures by goal |
| `memory_consolidate` | Merge related memories |

---

## Phase 2: Context Compression

**Status:** Planned  
**Linear Issues:** ENG-38 to ENG-41 (4 issues)  
**Target Schema:** v9

### Objective

Reduce context window usage by intelligently compressing and summarizing memories.

### Issues

| Issue | Title | Priority | Status |
|-------|-------|----------|--------|
| ENG-38 | Implement soft trim algorithm | P0 | Pending |
| ENG-39 | Extractive summarization | P1 | Pending |
| ENG-40 | Hierarchical compression | P1 | Pending |
| ENG-41 | Compression metrics | P2 | Pending |

### Compression Strategies

1. **Soft Trim:** Preserve head (60%) + tail (30%) with ellipsis
2. **Extractive Summary:** Key sentences based on TF-IDF
3. **Hierarchical:** Group related memories into summaries

### New MCP Tools

| Tool | Description |
|------|-------------|
| `memory_compress` | Compress single memory |
| `memory_summarize_batch` | Summarize memory group |
| `memory_get_compressed` | Get memory with compression |

---

## Phase 3: Langfuse Integration

**Status:** Planned  
**Linear Issues:** ENG-42 to ENG-44 (3 issues)  
**Target Schema:** v10

### Objective

Integrate with Langfuse for observability, tracing, and analytics.

### Issues

| Issue | Title | Priority | Status |
|-------|-------|----------|--------|
| ENG-42 | Langfuse client integration | P0 | Pending |
| ENG-43 | Trace memory operations | P1 | Pending |
| ENG-44 | Search quality metrics | P1 | Pending |

### New MCP Tools

| Tool | Description |
|------|-------------|
| `trace_start` | Start trace span |
| `trace_end` | End trace span |
| `trace_log` | Log event to trace |
| `analytics_export` | Export analytics data |

---

## Phase 4: Search Result Caching

**Status:** Planned  
**Linear Issues:** ENG-45 to ENG-48 (4 issues)  
**Target Schema:** v10 (no change)

### Objective

Cache search results to reduce latency for repeated queries.

### Issues

| Issue | Title | Priority | Status |
|-------|-------|----------|--------|
| ENG-45 | LRU cache implementation | P0 | Pending |
| ENG-46 | Cache invalidation strategy | P0 | Pending |
| ENG-47 | Cache hit/miss metrics | P1 | Pending |
| ENG-48 | Configurable cache TTL | P2 | Pending |

### New MCP Tools

| Tool | Description |
|------|-------------|
| `search_cache_stats` | Get cache statistics |
| `search_cache_clear` | Clear search cache |

---

## Phase 5: Memory Lifecycle

**Status:** Planned  
**Linear Issues:** ENG-49 to ENG-53 (5 issues)  
**Target Schema:** v12

### Objective

Full lifecycle management: creation, validation, archival, and deletion.

### Issues

| Issue | Title | Priority | Status |
|-------|-------|----------|--------|
| ENG-49 | Validation status tracking | P0 | Pending |
| ENG-50 | Auto-archival rules | P1 | Pending |
| ENG-51 | Soft delete with recovery | P1 | Pending |
| ENG-52 | Lifecycle hooks | P2 | Pending |
| ENG-53 | Retention policies | P2 | Pending |

### New MCP Tools

| Tool | Description |
|------|-------------|
| `memory_validate` | Mark memory as validated |
| `memory_invalidate` | Mark memory as invalid |
| `memory_archive` | Archive memory |
| `memory_restore` | Restore archived memory |
| `memory_purge` | Permanently delete |
| `lifecycle_stats` | Get lifecycle statistics |

---

## Phase 6: Turso Support

**Status:** Planned  
**Linear Issues:** ENG-54 to ENG-57 (4 issues)  
**Target Schema:** v12 (no change)

### Objective

Add Turso (libSQL) as an alternative backend for edge deployments.

### Issues

| Issue | Title | Priority | Status |
|-------|-------|----------|--------|
| ENG-54 | Turso client integration | P0 | Pending |
| ENG-55 | Embedded replica sync | P1 | Pending |
| ENG-56 | Migration compatibility | P1 | Pending |
| ENG-57 | Performance benchmarks | P2 | Pending |

### No New MCP Tools

Turso is a backend change; existing tools work transparently.

---

## Phase 7: Meilisearch

**Status:** Planned  
**Linear Issues:** ENG-58 to ENG-62 (5 issues)  
**Target Schema:** v13

### Objective

Optional Meilisearch integration for enhanced full-text search.

### Issues

| Issue | Title | Priority | Status |
|-------|-------|----------|--------|
| ENG-58 | Meilisearch client | P0 | Pending |
| ENG-59 | Index synchronization | P0 | Pending |
| ENG-60 | Faceted search support | P1 | Pending |
| ENG-61 | Typo tolerance tuning | P1 | Pending |
| ENG-62 | Fallback to SQLite FTS | P2 | Pending |

### New MCP Tools

| Tool | Description |
|------|-------------|
| `search_faceted` | Search with facet filters |
| `search_suggest` | Get search suggestions |
| `index_rebuild` | Rebuild Meilisearch index |
| `index_stats` | Get index statistics |

---

## Phase 8: Salience & Session Memory

**Status:** Planned  
**Linear Issues:** ENG-66 to ENG-77 (12 issues)  
**Target Schema:** v14

### Objective

Implement salience scoring and session-scoped memory for context-aware retrieval.

### Issues

| Issue | Title | Priority | Status |
|-------|-------|----------|--------|
| ENG-66 | Salience score calculation | P0 | Pending |
| ENG-67 | Temporal decay function | P0 | Pending |
| ENG-68 | Access frequency tracking | P0 | Pending |
| ENG-69 | Session memory scope | P1 | Pending |
| ENG-70 | Session persistence options | P1 | Pending |
| ENG-71 | Cross-session linking | P1 | Pending |
| ENG-72 | Salience-based reranking | P1 | Pending |
| ENG-73 | Session summarization | P2 | Pending |
| ENG-74 | Session export/import | P2 | Pending |
| ENG-75 | Salience decay job | P2 | Pending |
| ENG-76 | Session analytics | P2 | Pending |
| ENG-77 | Memory attention weights | P2 | Pending |

### Salience Formula

```
Salience = (Recency * w1) + (Frequency * w2) + (Importance * w3) + (Feedback * w4)

Where:
- Recency: Exponential decay from last access
- Frequency: Log-scaled access count
- Importance: User-set importance score
- Feedback: Net positive/negative signals

Default weights: w1=0.3, w2=0.2, w3=0.3, w4=0.2
```

### New MCP Tools

| Tool | Description |
|------|-------------|
| `memory_get_salience` | Get salience score |
| `memory_set_importance` | Set user importance |
| `session_create` | Create session scope |
| `session_add_memory` | Add memory to session |
| `session_summarize` | Summarize session |
| `session_export` | Export session data |
| `salience_decay_run` | Run decay job |
| `salience_stats` | Get salience distribution |

---

## Phase 9: Context Quality

**Status:** Planned  
**Linear Issues:** ENG-48 to ENG-66 (19 issues)  
**Target Schema:** v15

### Objective

Improve context quality through deduplication, conflict detection, and quality scoring.

### Issues

| Issue | Title | Priority | Status |
|-------|-------|----------|--------|
| ENG-48 | Near-duplicate detection | P0 | Pending |
| ENG-49 | Semantic deduplication | P0 | Pending |
| ENG-50 | Conflict detection | P0 | Pending |
| ENG-51 | Contradiction resolution | P1 | Pending |
| ENG-52 | Quality score algorithm | P1 | Pending |
| ENG-53 | Source credibility scoring | P1 | Pending |
| ENG-54 | Freshness scoring | P1 | Pending |
| ENG-55 | Completeness scoring | P2 | Pending |
| ENG-56 | Auto-merge candidates | P2 | Pending |
| ENG-57 | Quality improvement suggestions | P2 | Pending |
| ENG-58 | Conflict visualization | P2 | Pending |
| ENG-59 | Quality trend tracking | P2 | Pending |
| ENG-60 | Source verification | P2 | Pending |
| ENG-61 | Cross-reference validation | P2 | Pending |
| ENG-62 | Quality alerts | P3 | Pending |
| ENG-63 | Batch quality assessment | P3 | Pending |
| ENG-64 | Quality report generation | P3 | Pending |
| ENG-65 | Auto-cleanup rules | P3 | Pending |
| ENG-66 | Quality dashboard data | P3 | Pending |

### Quality Score Components

```
Quality = (Clarity * 0.25) + (Completeness * 0.20) + (Freshness * 0.20) + 
          (Consistency * 0.20) + (Source_Trust * 0.15)

Where:
- Clarity: Readability and structure
- Completeness: Information density
- Freshness: Time since last update
- Consistency: Agreement with related memories
- Source_Trust: Origin credibility
```

### New MCP Tools

| Tool | Description |
|------|-------------|
| `memory_quality_score` | Get quality breakdown |
| `memory_find_duplicates` | Find near-duplicates |
| `memory_find_conflicts` | Find contradictions |
| `memory_suggest_merge` | Get merge candidates |
| `memory_auto_merge` | Merge duplicates |
| `memory_resolve_conflict` | Resolve contradiction |
| `quality_report` | Generate quality report |
| `quality_trend` | Get quality over time |
| `quality_alerts` | Get quality issues |
| `quality_improve` | Get improvement suggestions |

---

## Tool Growth Projection

### By Phase

| Phase | Starting Tools | New Tools | Ending Tools |
|-------|----------------|-----------|--------------|
| Current | 92 | - | 92 |
| Phase 0 | 92 | 3 | 95 |
| Phase 1 | 95 | 8 | 103 |
| Phase 2 | 103 | 3 | 106 |
| Phase 3 | 106 | 4 | 110 |
| Phase 4 | 110 | 2 | 112 |
| Phase 5 | 112 | 6 | 118 |
| Phase 6 | 118 | 0 | 118 |
| Phase 7 | 118 | 4 | 122 |
| Phase 8 | 122 | 8 | 130 |
| Phase 9 | 130 | 10 | 140 |

**Final Projection:** 126+ tools (conservative), 140 tools (all phases complete)

### By Category

| Category | Current | Projected |
|----------|---------|-----------|
| Core CRUD | 15 | 18 |
| Search | 8 | 14 |
| Organization | 12 | 18 |
| Graph/Links | 10 | 12 |
| Sessions | 6 | 12 |
| Quality | 4 | 14 |
| Analytics | 6 | 12 |
| Storage | 3 | 8 |
| Utilities | 12 | 16 |
| Sync/Export | 8 | 10 |
| **Total** | **92** | **126+** |

---

## Schema Evolution

### Version History

| Version | Phase | Key Changes |
|---------|-------|-------------|
| v4 | Current | Base schema (memories, tags, crossrefs, entities) |
| v5 | Phase 0 | Storage backend metadata |
| v6-v7 | Phase 1 | Cognitive memory types columns |
| v8 | Phase 1 | Memory type inference cache |
| v9 | Phase 2 | Compression metadata |
| v10 | Phase 3 | Trace/analytics tables |
| v11 | Phase 5 | Lifecycle status columns |
| v12 | Phase 5 | Retention policy tables |
| v13 | Phase 7 | Meilisearch sync tracking |
| v14 | Phase 8 | Salience columns, session tables |
| v15 | Phase 9 | Quality score columns, conflict tables |

### Schema v15 Preview

```sql
-- New columns on memories table
ALTER TABLE memories ADD COLUMN memory_class TEXT;  -- episodic, semantic, procedural
ALTER TABLE memories ADD COLUMN salience_score REAL DEFAULT 0.5;
ALTER TABLE memories ADD COLUMN quality_score REAL DEFAULT 0.5;
ALTER TABLE memories ADD COLUMN validation_status TEXT DEFAULT 'unverified';
ALTER TABLE memories ADD COLUMN last_accessed_at TEXT;
ALTER TABLE memories ADD COLUMN access_count INTEGER DEFAULT 0;

-- New tables
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    ended_at TEXT,
    summary TEXT,
    metadata TEXT
);

CREATE TABLE session_memories (
    session_id TEXT REFERENCES sessions(id),
    memory_id INTEGER REFERENCES memories(id),
    added_at TEXT NOT NULL,
    PRIMARY KEY (session_id, memory_id)
);

CREATE TABLE memory_conflicts (
    id INTEGER PRIMARY KEY,
    memory_a INTEGER REFERENCES memories(id),
    memory_b INTEGER REFERENCES memories(id),
    conflict_type TEXT NOT NULL,
    resolved INTEGER DEFAULT 0,
    resolution TEXT,
    detected_at TEXT NOT NULL
);

CREATE TABLE quality_history (
    id INTEGER PRIMARY KEY,
    memory_id INTEGER REFERENCES memories(id),
    quality_score REAL NOT NULL,
    components TEXT,  -- JSON breakdown
    recorded_at TEXT NOT NULL
);
```

---

## Timeline Estimates

**Disclaimer:** Timelines are estimates and depend on contributor availability.

| Phase | Estimated Duration | Dependencies |
|-------|-------------------|--------------|
| Phase 0 | 4-6 weeks | None |
| Phase 1 | 3-4 weeks | Phase 0 |
| Phase 2 | 2-3 weeks | Phase 1 |
| Phase 3 | 2 weeks | Phase 0 |
| Phase 4 | 1-2 weeks | Phase 0 |
| Phase 5 | 3-4 weeks | Phase 0, 1 |
| Phase 6 | 2-3 weeks | Phase 0 |
| Phase 7 | 3-4 weeks | Phase 0 |
| Phase 8 | 4-5 weeks | Phase 1, 5 |
| Phase 9 | 5-6 weeks | Phase 1, 2, 5, 8 |

**Total Estimated:** 6-9 months for all phases

---

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines. To work on a specific phase:

1. Check the Linear board for available issues
2. Comment on the issue to claim it
3. Create a branch: `feat/eng-XX-brief-description`
4. Submit PR with tests and documentation

---

**See Also:**
- [LINEAR_ISSUES.md](./LINEAR_ISSUES.md) - Complete issue reference
- [SCHEMA.md](./SCHEMA.md) - Database schema details
- [concepts/](./concepts/) - Design documents

---

**Last Updated:** January 29, 2026
