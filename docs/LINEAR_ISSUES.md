# Linear Issues Reference

**Date:** January 29, 2026  
**Project:** Engram (ENG)  
**Total Issues:** 80+

---

## Table of Contents

1. [Overview](#overview)
2. [Phase 0: Storage Abstraction (ENG-14 to ENG-32)](#phase-0-storage-abstraction)
3. [Phase 1-5: Core Features (ENG-33 to ENG-37)](#phase-1-5-core-features)
4. [Phase 8: Salience & Session Memory (ENG-66 to ENG-77)](#phase-8-salience--session-memory)
5. [Phase 9: Context Quality (ENG-48 to ENG-66)](#phase-9-context-quality)
6. [Issue Status Legend](#issue-status-legend)
7. [Priority Definitions](#priority-definitions)

---

## Overview

All Engram development work is tracked in Linear under the `ENG` project. Issues are organized by phase, with dependencies clearly marked.

### Quick Stats

| Phase | Issue Range | Count | Status |
|-------|-------------|-------|--------|
| Phase 0 | ENG-14 to ENG-32 | 19 | In Progress |
| Phase 1-5 | ENG-33 to ENG-37 | 5 | Planned |
| Phase 8 | ENG-66 to ENG-77 | 12 | Planned |
| Phase 9 | ENG-48 to ENG-66 | 19 | Planned |

---

## Phase 0: Storage Abstraction

**Objective:** Decouple storage operations from SQLite to enable multiple backends.

### Issues (ENG-14 to ENG-32)

| Issue | Title | Priority | Status | Depends On |
|-------|-------|----------|--------|------------|
| ENG-14 | Define StorageBackend trait | P0 | Done | - |
| ENG-15 | Implement SQLite backend | P0 | Done | ENG-14 |
| ENG-16 | Abstract connection pooling | P0 | In Progress | ENG-14 |
| ENG-17 | Generic transaction wrapper | P0 | In Progress | ENG-14 |
| ENG-18 | Migrate memory CRUD to trait | P1 | Pending | ENG-14, ENG-15 |
| ENG-19 | Migrate search queries to trait | P1 | Pending | ENG-18 |
| ENG-20 | Migrate graph queries to trait | P1 | Pending | ENG-18 |
| ENG-21 | Migrate entity queries to trait | P1 | Pending | ENG-18 |
| ENG-22 | Backend configuration system | P1 | Pending | ENG-14 |
| ENG-23 | Connection string parsing | P1 | Pending | ENG-22 |
| ENG-24 | Backend health checks | P2 | Pending | ENG-14 |
| ENG-25 | Query logging abstraction | P2 | Pending | ENG-14 |
| ENG-26 | Metrics per backend | P2 | Pending | ENG-24, ENG-25 |
| ENG-27 | Migration runner abstraction | P1 | Pending | ENG-14 |
| ENG-28 | Test harness for backends | P1 | Pending | ENG-14 |
| ENG-29 | SQLite WAL configuration | P2 | Pending | ENG-15 |
| ENG-30 | Connection timeout handling | P2 | Pending | ENG-16 |
| ENG-31 | Retry logic for transient failures | P2 | Pending | ENG-16 |
| ENG-32 | Backend capability detection | P2 | Pending | ENG-14 |

### Issue Details

#### ENG-14: Define StorageBackend trait

**Description:**
Create the core `StorageBackend` trait that all storage implementations must implement. This trait defines the contract for memory operations.

**Acceptance Criteria:**
- [ ] Trait defined in `src/storage/backend.rs`
- [ ] CRUD methods: create, read, update, delete
- [ ] Search methods: search, hybrid_search
- [ ] Transaction support via associated type
- [ ] Error type defined
- [ ] Documentation with examples

**Code Sketch:**
```rust
pub trait StorageBackend: Send + Sync {
    type Connection;
    type Transaction<'a>;
    type Error: std::error::Error;
    
    fn with_connection<F, T>(&self, f: F) -> Result<T, Self::Error>
    where F: FnOnce(&Self::Connection) -> Result<T, Self::Error>;
    
    fn with_transaction<F, T>(&self, f: F) -> Result<T, Self::Error>
    where F: FnOnce(&mut Self::Transaction<'_>) -> Result<T, Self::Error>;
}
```

---

#### ENG-15: Implement SQLite backend

**Description:**
Implement `StorageBackend` for SQLite, wrapping the existing rusqlite-based code.

**Acceptance Criteria:**
- [ ] `SqliteBackend` struct implementing `StorageBackend`
- [ ] Connection pooling via `r2d2`
- [ ] WAL mode enabled by default
- [ ] All existing tests pass
- [ ] Benchmark shows no regression

---

#### ENG-16: Abstract connection pooling

**Description:**
Create a generic connection pool interface that works across backends.

**Acceptance Criteria:**
- [ ] `ConnectionPool` trait defined
- [ ] SQLite implementation using r2d2
- [ ] Configurable pool size
- [ ] Health check support
- [ ] Metrics: active/idle connections

---

#### ENG-17: Generic transaction wrapper

**Description:**
Create a transaction wrapper that provides consistent semantics across backends.

**Acceptance Criteria:**
- [ ] `Transaction` trait with commit/rollback
- [ ] Automatic rollback on drop
- [ ] Savepoint support (optional)
- [ ] Nested transaction handling

---

#### ENG-18: Migrate memory CRUD to trait

**Description:**
Refactor `storage/queries.rs` to use the `StorageBackend` trait.

**Acceptance Criteria:**
- [ ] `create_memory` uses trait
- [ ] `get_memory` uses trait
- [ ] `update_memory` uses trait
- [ ] `delete_memory` uses trait
- [ ] `list_memories` uses trait
- [ ] Backward compatible API

---

#### ENG-19: Migrate search queries to trait

**Description:**
Refactor search operations to work with any storage backend.

**Acceptance Criteria:**
- [ ] FTS search abstracted
- [ ] Vector search abstracted
- [ ] Hybrid search works with both
- [ ] Backend-specific optimizations preserved

---

#### ENG-20: Migrate graph queries to trait

**Description:**
Refactor knowledge graph operations for backend independence.

**Acceptance Criteria:**
- [ ] `create_crossref` abstracted
- [ ] `get_related` abstracted
- [ ] Multi-hop traversal works
- [ ] Path finding preserved

---

#### ENG-21: Migrate entity queries to trait

**Description:**
Refactor entity operations for backend independence.

**Acceptance Criteria:**
- [ ] Entity CRUD abstracted
- [ ] Memory-entity linking abstracted
- [ ] Entity search preserved

---

#### ENG-22: Backend configuration system

**Description:**
Create a configuration system for selecting and configuring storage backends.

**Acceptance Criteria:**
- [ ] Environment variable support
- [ ] Config file support (optional)
- [ ] Sensible defaults
- [ ] Validation on startup

**Configuration Example:**
```toml
[storage]
backend = "sqlite"
path = "~/.local/share/engram/memories.db"

[storage.sqlite]
wal_mode = true
pool_size = 10
```

---

#### ENG-23: Connection string parsing

**Description:**
Support connection strings for backend configuration.

**Acceptance Criteria:**
- [ ] SQLite: `sqlite:///path/to/db.sqlite`
- [ ] Turso: `libsql://host/db?authToken=xxx`
- [ ] Parse and validate on startup
- [ ] Error messages for invalid strings

---

#### ENG-24: Backend health checks

**Description:**
Implement health check mechanism for storage backends.

**Acceptance Criteria:**
- [ ] `health_check()` method on trait
- [ ] Checks connection liveness
- [ ] Returns latency metrics
- [ ] MCP tool exposure: `storage_health_check`

---

#### ENG-25: Query logging abstraction

**Description:**
Add query logging that works across backends.

**Acceptance Criteria:**
- [ ] Log slow queries (> threshold)
- [ ] Include query parameters (sanitized)
- [ ] Configurable log level
- [ ] Optional query explain plans

---

#### ENG-26: Metrics per backend

**Description:**
Collect and expose metrics for each storage backend.

**Acceptance Criteria:**
- [ ] Query count by type
- [ ] Query latency histograms
- [ ] Connection pool stats
- [ ] MCP tool: `storage_metrics`

---

#### ENG-27: Migration runner abstraction

**Description:**
Create a generic migration system that works across backends.

**Acceptance Criteria:**
- [ ] Migration trait defined
- [ ] Version tracking table
- [ ] Forward migrations
- [ ] Dry-run support
- [ ] SQLite implementation

---

#### ENG-28: Test harness for backends

**Description:**
Create a test suite that can run against any backend implementation.

**Acceptance Criteria:**
- [ ] Trait-based test suite
- [ ] Run against SQLite
- [ ] Easy to add new backends
- [ ] CI integration

---

#### ENG-29: SQLite WAL configuration

**Description:**
Make SQLite WAL mode configurable with sensible defaults.

**Acceptance Criteria:**
- [ ] WAL mode on by default
- [ ] Configurable checkpoint interval
- [ ] Auto-checkpoint tuning
- [ ] Documentation

---

#### ENG-30: Connection timeout handling

**Description:**
Handle connection timeouts gracefully.

**Acceptance Criteria:**
- [ ] Configurable timeout
- [ ] Retry logic
- [ ] Clear error messages
- [ ] No hanging operations

---

#### ENG-31: Retry logic for transient failures

**Description:**
Implement retry logic for transient database errors.

**Acceptance Criteria:**
- [ ] Exponential backoff
- [ ] Configurable max retries
- [ ] Distinguish transient vs permanent errors
- [ ] Logging of retries

---

#### ENG-32: Backend capability detection

**Description:**
Detect and expose backend capabilities for feature negotiation.

**Acceptance Criteria:**
- [ ] Capability enum/flags
- [ ] FTS support detection
- [ ] Vector support detection
- [ ] MCP tool: `storage_backend_info`

---

## Phase 1-5: Core Features

**Objective:** Implement cognitive memory types and core lifecycle features.

### Issues (ENG-33 to ENG-37)

| Issue | Title | Priority | Status | Phase |
|-------|-------|----------|--------|-------|
| ENG-33 | Episodic memory schema | P0 | Pending | 1 |
| ENG-34 | Semantic memory extraction | P0 | Pending | 1 |
| ENG-35 | Procedural memory format | P1 | Pending | 1 |
| ENG-36 | Memory type inference | P1 | Pending | 1 |
| ENG-37 | Cross-type linking | P2 | Pending | 1 |

### Issue Details

#### ENG-33: Episodic memory schema

**Description:**
Define schema and implementation for episodic (event-based) memories.

**Acceptance Criteria:**
- [ ] Schema migration for episodic fields
- [ ] Event time tracking
- [ ] Participant tracking
- [ ] Location tracking (optional)
- [ ] MCP tool: `memory_create_episodic`

---

#### ENG-34: Semantic memory extraction

**Description:**
Extract semantic (fact-based) information from content.

**Acceptance Criteria:**
- [ ] Fact extraction algorithm
- [ ] Confidence scoring
- [ ] Deduplication of facts
- [ ] MCP tool: `memory_create_semantic`

---

#### ENG-35: Procedural memory format

**Description:**
Define format for procedural (how-to) memories.

**Acceptance Criteria:**
- [ ] Step-by-step structure
- [ ] Prerequisite tracking
- [ ] Expected outcome per step
- [ ] MCP tool: `memory_create_procedural`

---

#### ENG-36: Memory type inference

**Description:**
Automatically infer memory type from content.

**Acceptance Criteria:**
- [ ] Classification algorithm
- [ ] Confidence threshold
- [ ] Manual override support
- [ ] MCP tool: `memory_infer_type`

---

#### ENG-37: Cross-type linking

**Description:**
Link memories across cognitive types.

**Acceptance Criteria:**
- [ ] Episodic -> Semantic extraction
- [ ] Procedural -> Semantic fact links
- [ ] Cross-type search support

---

## Phase 8: Salience & Session Memory

**Objective:** Implement salience scoring and session-scoped memory.

### Issues (ENG-66 to ENG-77)

| Issue | Title | Priority | Status | Depends On |
|-------|-------|----------|--------|------------|
| ENG-66 | Salience score calculation | P0 | Pending | - |
| ENG-67 | Temporal decay function | P0 | Pending | ENG-66 |
| ENG-68 | Access frequency tracking | P0 | Pending | - |
| ENG-69 | Session memory scope | P1 | Pending | - |
| ENG-70 | Session persistence options | P1 | Pending | ENG-69 |
| ENG-71 | Cross-session linking | P1 | Pending | ENG-69 |
| ENG-72 | Salience-based reranking | P1 | Pending | ENG-66 |
| ENG-73 | Session summarization | P2 | Pending | ENG-69 |
| ENG-74 | Session export/import | P2 | Pending | ENG-69 |
| ENG-75 | Salience decay job | P2 | Pending | ENG-67 |
| ENG-76 | Session analytics | P2 | Pending | ENG-69 |
| ENG-77 | Memory attention weights | P2 | Pending | ENG-66 |

### Issue Details

#### ENG-66: Salience score calculation

**Description:**
Implement the salience score algorithm combining recency, frequency, importance, and feedback.

**Formula:**
```
Salience = (Recency * 0.3) + (Frequency * 0.2) + (Importance * 0.3) + (Feedback * 0.2)
```

**Acceptance Criteria:**
- [ ] Score calculation function
- [ ] Score range 0.0-1.0
- [ ] Update on access
- [ ] MCP tool: `memory_get_salience`

---

#### ENG-67: Temporal decay function

**Description:**
Implement exponential decay for recency component of salience.

**Acceptance Criteria:**
- [ ] Configurable half-life
- [ ] Decay calculation efficient
- [ ] Background decay job
- [ ] No negative scores

---

#### ENG-68: Access frequency tracking

**Description:**
Track how often each memory is accessed.

**Acceptance Criteria:**
- [ ] Increment on read
- [ ] Log-scale for scoring
- [ ] `last_accessed_at` column
- [ ] `access_count` column

---

#### ENG-69: Session memory scope

**Description:**
Create session-scoped memories that belong to a specific interaction session.

**Acceptance Criteria:**
- [ ] Session table
- [ ] Session-memory link table
- [ ] Session create/end API
- [ ] MCP tool: `session_create`

---

#### ENG-70: Session persistence options

**Description:**
Allow sessions to persist or expire based on configuration.

**Acceptance Criteria:**
- [ ] Ephemeral sessions (deleted on end)
- [ ] Persistent sessions (kept for history)
- [ ] Configurable retention
- [ ] Cleanup job

---

#### ENG-71: Cross-session linking

**Description:**
Link memories across sessions for continuity.

**Acceptance Criteria:**
- [ ] Link memories from different sessions
- [ ] Track link provenance
- [ ] Search across sessions

---

#### ENG-72: Salience-based reranking

**Description:**
Use salience scores in search result reranking.

**Acceptance Criteria:**
- [ ] Integrate with existing reranker
- [ ] Configurable weight
- [ ] Performance benchmark

---

#### ENG-73: Session summarization

**Description:**
Generate summaries of session content.

**Acceptance Criteria:**
- [ ] Extractive summary
- [ ] Key points extraction
- [ ] MCP tool: `session_summarize`

---

#### ENG-74: Session export/import

**Description:**
Export and import session data.

**Acceptance Criteria:**
- [ ] JSON export format
- [ ] Import with ID mapping
- [ ] MCP tools: `session_export`, `session_import`

---

#### ENG-75: Salience decay job

**Description:**
Background job to update salience scores based on time.

**Acceptance Criteria:**
- [ ] Configurable interval
- [ ] Batch processing
- [ ] Minimal locking

---

#### ENG-76: Session analytics

**Description:**
Analytics for session usage patterns.

**Acceptance Criteria:**
- [ ] Session duration stats
- [ ] Memory creation per session
- [ ] Topic distribution

---

#### ENG-77: Memory attention weights

**Description:**
Track attention weights when memories are used in context.

**Acceptance Criteria:**
- [ ] Record attention per use
- [ ] Aggregate for importance
- [ ] Visualization support

---

## Phase 9: Context Quality

**Objective:** Improve context quality through deduplication, conflict detection, and quality scoring.

### Issues (ENG-48 to ENG-66)

| Issue | Title | Priority | Status | Depends On |
|-------|-------|----------|--------|------------|
| ENG-48 | Near-duplicate detection | P0 | Pending | - |
| ENG-49 | Semantic deduplication | P0 | Pending | ENG-48 |
| ENG-50 | Conflict detection | P0 | Pending | - |
| ENG-51 | Contradiction resolution | P1 | Pending | ENG-50 |
| ENG-52 | Quality score algorithm | P1 | Pending | - |
| ENG-53 | Source credibility scoring | P1 | Pending | ENG-52 |
| ENG-54 | Freshness scoring | P1 | Pending | ENG-52 |
| ENG-55 | Completeness scoring | P2 | Pending | ENG-52 |
| ENG-56 | Auto-merge candidates | P2 | Pending | ENG-48, ENG-49 |
| ENG-57 | Quality improvement suggestions | P2 | Pending | ENG-52 |
| ENG-58 | Conflict visualization | P2 | Pending | ENG-50 |
| ENG-59 | Quality trend tracking | P2 | Pending | ENG-52 |
| ENG-60 | Source verification | P2 | Pending | ENG-53 |
| ENG-61 | Cross-reference validation | P2 | Pending | ENG-50 |
| ENG-62 | Quality alerts | P3 | Pending | ENG-52 |
| ENG-63 | Batch quality assessment | P3 | Pending | ENG-52 |
| ENG-64 | Quality report generation | P3 | Pending | ENG-52 |
| ENG-65 | Auto-cleanup rules | P3 | Pending | ENG-48 |
| ENG-66 | Quality dashboard data | P3 | Pending | ENG-52 |

### Issue Details

#### ENG-48: Near-duplicate detection

**Description:**
Detect near-duplicate memories using text similarity.

**Acceptance Criteria:**
- [ ] SimHash or MinHash implementation
- [ ] Configurable threshold
- [ ] Batch processing
- [ ] MCP tool: `memory_find_duplicates`

---

#### ENG-49: Semantic deduplication

**Description:**
Detect semantic duplicates using embedding similarity.

**Acceptance Criteria:**
- [ ] Cosine similarity threshold
- [ ] Cluster similar memories
- [ ] Suggest canonical version

---

#### ENG-50: Conflict detection

**Description:**
Detect conflicting or contradictory memories.

**Acceptance Criteria:**
- [ ] Contradiction detection
- [ ] Staleness detection (old vs new)
- [ ] Conflict table
- [ ] MCP tool: `memory_find_conflicts`

---

#### ENG-51: Contradiction resolution

**Description:**
UI/API for resolving detected contradictions.

**Acceptance Criteria:**
- [ ] Keep one, archive other
- [ ] Merge into new
- [ ] Record resolution
- [ ] MCP tool: `memory_resolve_conflict`

---

#### ENG-52: Quality score algorithm

**Description:**
Implement comprehensive quality scoring.

**Formula:**
```
Quality = (Clarity * 0.25) + (Completeness * 0.20) + (Freshness * 0.20) + 
          (Consistency * 0.20) + (Source_Trust * 0.15)
```

**Acceptance Criteria:**
- [ ] Score calculation
- [ ] Component breakdown
- [ ] Update triggers
- [ ] MCP tool: `memory_quality_score`

---

#### ENG-53: Source credibility scoring

**Description:**
Score memory sources by reliability.

**Acceptance Criteria:**
- [ ] Source trust table
- [ ] Default scores by source type
- [ ] Adjustable per source
- [ ] Impact on quality score

---

#### ENG-54: Freshness scoring

**Description:**
Score memories by how current they are.

**Acceptance Criteria:**
- [ ] Time-based decay
- [ ] Update detection
- [ ] Domain-specific freshness

---

#### ENG-55: Completeness scoring

**Description:**
Score memories by information completeness.

**Acceptance Criteria:**
- [ ] Length consideration
- [ ] Structure detection
- [ ] Missing field detection

---

#### ENG-56: Auto-merge candidates

**Description:**
Automatically suggest memories to merge.

**Acceptance Criteria:**
- [ ] Candidate detection
- [ ] Merge preview
- [ ] MCP tool: `memory_suggest_merge`

---

#### ENG-57: Quality improvement suggestions

**Description:**
Suggest ways to improve memory quality.

**Acceptance Criteria:**
- [ ] Per-memory suggestions
- [ ] Actionable recommendations
- [ ] MCP tool: `quality_improve`

---

#### ENG-58: Conflict visualization

**Description:**
Visualize conflicts in the knowledge graph.

**Acceptance Criteria:**
- [ ] Graph export with conflict edges
- [ ] Color coding
- [ ] Integration with vis.js

---

#### ENG-59: Quality trend tracking

**Description:**
Track quality scores over time.

**Acceptance Criteria:**
- [ ] History table
- [ ] Trend calculation
- [ ] MCP tool: `quality_trend`

---

#### ENG-60: Source verification

**Description:**
Verify source claims against external data.

**Acceptance Criteria:**
- [ ] URL verification
- [ ] Timestamp verification
- [ ] Manual verification flag

---

#### ENG-61: Cross-reference validation

**Description:**
Validate that cross-references are still accurate.

**Acceptance Criteria:**
- [ ] Periodic validation job
- [ ] Broken link detection
- [ ] Repair suggestions

---

#### ENG-62: Quality alerts

**Description:**
Alert on quality issues.

**Acceptance Criteria:**
- [ ] Low quality threshold
- [ ] Conflict detection alerts
- [ ] Staleness alerts

---

#### ENG-63: Batch quality assessment

**Description:**
Assess quality of multiple memories at once.

**Acceptance Criteria:**
- [ ] Workspace-level assessment
- [ ] Tag-level assessment
- [ ] Progress tracking

---

#### ENG-64: Quality report generation

**Description:**
Generate quality reports.

**Acceptance Criteria:**
- [ ] Summary statistics
- [ ] Top issues
- [ ] Recommendations
- [ ] MCP tool: `quality_report`

---

#### ENG-65: Auto-cleanup rules

**Description:**
Automatically clean up low-quality content.

**Acceptance Criteria:**
- [ ] Configurable rules
- [ ] Dry-run support
- [ ] Audit trail

---

#### ENG-66: Quality dashboard data

**Description:**
Provide data for external quality dashboards.

**Acceptance Criteria:**
- [ ] JSON export
- [ ] Time series data
- [ ] Comparison metrics

---

## Issue Status Legend

| Status | Description |
|--------|-------------|
| Done | Completed and merged |
| In Progress | Currently being worked on |
| Pending | Ready to start |
| Blocked | Waiting on dependencies |
| Canceled | No longer planned |

---

## Priority Definitions

| Priority | Description | Timeline |
|----------|-------------|----------|
| P0 | Critical, blocks other work | This sprint |
| P1 | Important, high impact | Next 2 sprints |
| P2 | Nice to have, medium impact | This quarter |
| P3 | Future consideration | Backlog |

---

**See Also:**
- [ROADMAP.md](./ROADMAP.md) - Overall project roadmap
- [SCHEMA.md](./SCHEMA.md) - Database schema changes

---

**Last Updated:** January 29, 2026
