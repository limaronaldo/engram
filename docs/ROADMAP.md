# Engram Roadmap

## Phase Overview

| Phase | Name | Status | Version | Description |
|-------|------|--------|---------|-------------|
| 0 | Storage Abstraction | Done | v0.1.0 | `StorageBackend` trait, SQLite + WAL, connection pooling, hybrid search (BM25 + vector + fuzzy + RRF) |
| 1 | Cognitive Memory Types | Done | v0.3.0 | Episodic, procedural, summary, checkpoint types with temporal queries |
| 2 | Context Compression | Done | v0.3.0 | Summarization, soft-trim, token budgeting (tiktoken-rs), batch archival |
| 3 | Langfuse Integration | Done | v0.3.0 | Trace sync, pattern extraction, async runtime (feature-gated) |
| 4 | Search Caching | Done | v0.3.0 | Embedding-based LRU cache, adaptive similarity threshold, feedback loop |
| 5 | Memory Lifecycle | Done | v0.3.0 | Active/Stale/Archived states, configurable thresholds, dry-run support |
| 6 | Turso/libSQL | Done | v0.3.0 | Distributed SQLite via Turso, full `StorageBackend` implementation |
| 7 | Meilisearch | Done | v0.5.0 | Full `StorageBackend` + background indexer, 4 MCP tools (feature-gated) |
| 8 | Salience Scoring | Done | v0.4.0 | Multi-signal salience (recency, frequency, importance, feedback), session context |
| 9 | Context Quality | Done | v0.4.0 | 5-component quality scoring, near-duplicate detection, conflict resolution, source trust |

All 10 phases complete. Published as v0.5.0.

---

## Phase Details

### Phase 0: Storage Abstraction (v0.1.0)

Foundation layer. Defined the `StorageBackend` trait (18 methods) so future backends (Turso, Meilisearch) can be swapped in without touching business logic.

- SQLite + WAL mode for crash recovery
- Connection pooling with read/write separation
- Hybrid search: BM25 (FTS5) + vector (sqlite-vec) + fuzzy (Levenshtein) + RRF fusion
- Knowledge graph with entity extraction, traversal, shortest-path
- Cloud sync (S3/R2) with AES-256 encryption
- MCP, REST, WebSocket, CLI interfaces
- 80+ MCP tools

### Phase 1: Cognitive Memory Types (v0.3.0)

Typed memory schema inspired by cognitive science.

- `Episodic`: event-based memories with temporal context
- `Procedural`: workflow/how-to patterns with success/failure tracking
- `Summary`: compressed knowledge from multiple memories
- `Checkpoint`: stable session state snapshots
- Schema v11: `event_time`, `event_duration_seconds`, `trigger_pattern`, `procedure_success_count`

### Phase 2: Context Compression (v0.3.0)

Keep context windows manageable without losing important information.

- `memory_summarize`: create summary from multiple memories
- `memory_soft_trim`: preserve head (60%) + tail (30%) with ellipsis
- `context_budget_check`: token counting via tiktoken-rs
- `memory_archive_old`: batch archive low-importance memories

### Phase 3: Langfuse Integration (v0.3.0)

Observability pipeline. Feature-gated behind `--features langfuse`.

- Connect to Langfuse API, sync traces to memories
- Extract patterns: successful prompts, errors, preferences, tool usage
- Async background sync with dedicated Tokio runtime

### Phase 4: Search Caching (v0.3.0)

Reduce latency for repeated/similar queries.

- Embedding-based similarity lookup (not just exact match)
- Adaptive threshold (0.85-0.98) tuned by quality feedback
- TTL expiration (5 min default), auto-invalidation on writes

### Phase 5: Memory Lifecycle (v0.3.0)

Automatic memory aging: Active -> Stale (30d) -> Archived (90d).

- `lifecycle_run`: trigger cycle with dry-run support
- Archived memories excluded from search/list by default
- Configurable thresholds per workspace

### Phase 6: Turso/libSQL (v0.3.0)

Distributed SQLite for edge deployments.

- Full `StorageBackend` implementation via libSQL
- Embedded replicas with sync to Turso cloud
- Feature-gated behind `--features turso`

### Phase 7: Meilisearch (v0.5.0)

Offload search to Meilisearch for larger deployments.

- Full `StorageBackend` implementation (1,285 lines, 33 tests)
- `MeilisearchIndexer`: background sync from SQLite (full + incremental)
- 4 MCP tools: `meilisearch_search`, `meilisearch_reindex`, `meilisearch_status`, `meilisearch_config`
- SQLite remains source of truth; Meilisearch is a read-optimized mirror
- Feature-gated behind `--features meilisearch`

### Phase 8: Salience Scoring (v0.4.0)

Dynamic memory prioritization based on multiple signals.

- Formula: `Salience = (Recency * 0.3) + (Frequency * 0.2) + (Importance * 0.3) + (Feedback * 0.2)`
- Temporal decay transitions memories through lifecycle states
- Session context: conversation-scoped memory tracking with relevance scoring
- 17 new MCP tools
- Schema v14: `salience_history`, `session_memories` tables

### Phase 9: Context Quality (v0.4.0)

Ensure memory quality doesn't degrade over time.

- Quality = (Clarity * 0.25) + (Completeness * 0.20) + (Freshness * 0.20) + (Consistency * 0.20) + (Source Trust * 0.15)
- Near-duplicate detection via character n-gram Jaccard similarity
- Conflict detection for contradictions between memories
- Source trust scoring by origin type
- 9 new MCP tools
- Schema v15: `quality_history`, `memory_conflicts`, `source_trust_scores`, `duplicate_candidates`

---

## What's Next

All 10 planned phases are complete. Future directions under consideration:

- **OpenClaw integration**: Memory plugin for the 192K-star AI assistant platform
- **Multi-agent memory sharing**: Cross-agent memory federation
- **Streaming ingestion**: Real-time memory creation from event streams
- **WASM target**: Run Engram in the browser
- **Benchmark suite**: Automated performance regression tracking

---

## Version History

| Version | Date | Phases | MCP Tools |
|---------|------|--------|-----------|
| v0.1.0 | 2025-01-23 | 0 | 80+ |
| v0.2.0 | 2026-01-28 | 0 | 110+ |
| v0.3.0 | 2026-01-30 | 0-6 | 130+ |
| v0.4.0 | 2026-02-12 | 0-6, 8-9 | 140+ |
| v0.4.1 | 2026-02-13 | 0-6, 8-9 | 140+ |
| v0.5.0 | 2026-02-13 | 0-9 (all) | 144+ |
