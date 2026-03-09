# Changelog

All notable changes to Engram will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

---

## [0.11.0] - 2026-03-09

### Added — Cognitive Evolution & Platform Excellence (Phases E-K)

This release implements two major roadmap rounds spanning 7 new phases, 46+ new MCP tools, and schema v17 → v30.

#### Phase E: Memory Compression & Consolidation (RML-1207..1211)

Feature flag: `compression`

- **Semantic Structured Compression** — SimpleMem-inspired 30x token reduction via filler removal, SVO extraction, and deduplication (`src/intelligence/compression_semantic.rs`)
- **Online Semantic Synthesis** — Intra-session dedup with Jaccard overlap detection (`src/intelligence/synthesis.rs`)
- **Sleep-time Consolidation** — LightMem-inspired offline batch consolidation (`src/intelligence/consolidation_offline.rs`)
- **Active Context Compression** — Token-budget aware adaptive compression (`src/intelligence/context_compression.rs`)
- MCP tools: `memory_compress`, `memory_decompress`, `memory_compress_for_context`, `memory_consolidate`, `memory_synthesis`

#### Phase F: Agentic Memory Evolution (RML-1212..1215)

Feature flag: `agentic-evolution`

- **Historical Memory Update** — A-Mem-inspired auto-update with contradiction/supplement detection (`src/intelligence/memory_update.rs`)
- **Retrieval Utility Scoring** — MemRL-inspired Q-value utility with temporal decay (`src/search/utility.rs`)
- **Emotional & Reflective Memory** — Rule-based sentiment analysis + reflection engine (`src/intelligence/emotional.rs`)
- MCP tools: `memory_detect_updates`, `memory_utility_score`, `memory_sentiment_analyze`, `memory_sentiment_timeline`, `memory_reflect`

#### Phase G: Advanced Graph Intelligence (RML-1216..1219)

Feature flag: `advanced-graph`

- **Graph Conflict Detection & Resolution** — Mem0g-inspired contradiction, cycle, and orphan detection (`src/graph/conflicts.rs`)
- **Temporal Coactivation / Hebbian Learning** — "Neurons that fire together wire together" edge strengthening (`src/graph/coactivation.rs`)
- **Semantic Triplet Matching** — SPARQL-like SPO pattern matching with transitive inference (`src/graph/triplets.rs`)
- MCP tools: `memory_detect_conflicts`, `memory_resolve_conflict`, `memory_coactivation_report`, `memory_query_triplets`, `memory_add_knowledge`

#### Phase H: Autonomous Memory Agent (RML-1220..1223)

Feature flag: `autonomous-agent` (depends on compression + agentic-evolution + advanced-graph)

- **Proactive Memory Acquisition** — Gap detection + interest tracking (`src/intelligence/proactive.rs`)
- **Autonomous Pruning & Gardening** — 4-pass pipeline: dedup, compress, prune, link (`src/intelligence/gardening.rs`)
- **Memory Agent Loop** — Observe→decide→act tick-based agent (`src/intelligence/agent_loop.rs`)
- New binary: `engram-agent` (run/status/garden/suggest)
- MCP tools: `memory_agent_start`, `memory_agent_stop`, `memory_agent_status`, `memory_agent_metrics`, `memory_agent_configure`, `memory_garden`, `memory_garden_preview`, `memory_garden_undo`, `memory_suggest_acquisition`, `memory_proactive_scan`

#### Phase I: Retrieval Excellence (RML-1224..1231, RML-1242..1243)

Feature flags: `retrieval-excellence`, `ollama`, `cohere`, `voyage`, `onnx-embed`, `neural-rerank`

- **Multi-Provider Embeddings** — EmbeddingProvider trait + registry supporting Ollama, Cohere, Voyage AI, ONNX Runtime (`src/embedding/provider.rs`, `ollama.rs`, `cohere.rs`, `voyage.rs`, `onnx.rs`)
- **MMR Diversity-Aware Retrieval** — Maximal Marginal Relevance for result diversity (`src/search/mmr.rs`)
- **Semantic Query Cache** — Cosine-similarity DashMap cache with TTL and LRU (`src/search/semantic_cache.rs`)
- **Cross-Encoder Neural Reranking** — ONNX Runtime ms-marco-MiniLM reranker (`src/search/neural_rerank.rs`)
- **Search Explainability** — Per-result scoring breakdown with signal contributions (`src/search/explain.rs`)
- **Relevance Feedback Loop** — Useful/irrelevant signals with Laplace-smoothed boost (`src/search/feedback.rs`)
- MCP tools: `memory_cache_stats`, `memory_cache_clear`, `memory_embedding_providers`, `memory_embedding_migrate`, `memory_explain_search`, `memory_feedback`, `memory_feedback_stats`

#### Phase J: Context Engineering (RML-1225, RML-1232..1234)

Feature flag: `context-engineering`

- **Automatic Fact Extraction** — Rule-based SPO triple extraction with 80% compression target (`src/intelligence/fact_extraction.rs`)
- **Memory-Aware Prompt Construction** — 3 strategies (Greedy/Balanced/Recency) with token counting (`src/intelligence/context_builder.rs`)
- **Self-Editing Memory Blocks** — Letta-inspired 3-tier blocks with edit log (`src/storage/memory_blocks.rs`)
- MCP tools: `memory_extract_facts`, `memory_list_facts`, `memory_fact_graph`, `memory_build_context`, `memory_prompt_template`, `memory_token_estimate`, `memory_block_get`, `memory_block_edit`, `memory_block_list`, `memory_block_create`

#### Phase K: Temporal Graph & Platform Maturity (RML-1226, RML-1235..1237)

Feature flag: `temporal-graph`

- **Temporal Knowledge Graph** — Zep/Graphiti-inspired edges with validity periods, contradiction detection, snapshot-at-time (`src/graph/temporal.rs`)
- **Hierarchical Memory Scoping** — 5-level scope: Global > Org > User > Session > Agent (`src/storage/scoping.rs`)
- **Standardized Benchmark Suite** — LOCOMO, LongMemEval, MemBench frameworks (`src/bench/`)
- New binary: `engram-bench` (LOCOMO/LongMemEval/MemBench suites)
- MCP tools: `memory_temporal_create`, `memory_temporal_invalidate`, `memory_temporal_snapshot`, `memory_temporal_contradictions`, `memory_temporal_evolve`, `memory_scope_set`, `memory_scope_get`, `memory_scope_list`, `memory_scope_inherit`, `memory_scope_isolate`

### Changed

- Schema: v17 → v30 (14 additive migrations)
- Feature flags: 12 new (`compression`, `agentic-evolution`, `advanced-graph`, `autonomous-agent`, `retrieval-excellence`, `ollama`, `cohere`, `voyage`, `onnx-embed`, `neural-rerank`, `context-engineering`, `temporal-graph`)
- Binaries: 2 new (`engram-agent`, `engram-bench`)
- MCP tools: 161+ → 207+ (46 new tools across 7 phases)
- Tests: 300+ → 672+

---

## [0.8.1] - 2026-03-09

### Added — Reactive Infrastructure (Phase 11)

- **Emergent Graph** — Auto-generated semantic, temporal, and co-occurrence links with community detection
- **Document Ingestion** — Markdown and PDF ingestion with chunking and metadata
- Bumped from 0.7.0 with Round 1 infrastructure additions

---

## [0.7.0] - 2026-03-09

### Added

- **SSE event streaming** — `GET /v1/events` endpoint for real-time push notifications via Server-Sent Events
  - Subscribe to memory create/update/delete events over HTTP
  - Filter by `event_types` and `workspace` query parameters
  - Bearer token authentication, 30s keep-alive
  - Supports `Last-Event-Id` resume (future)
  - 17 unit tests
- **Agent registry** — Multi-agent federation foundation with namespace isolation
  - Schema v17: `agents` table with capabilities, namespaces, heartbeat, lifecycle status
  - 7 storage queries: register (upsert), deregister (soft delete), heartbeat, get, list, update capabilities, get by namespace
  - 6 MCP tools: `agent_register`, `agent_deregister`, `agent_heartbeat`, `agent_list`, `agent_get`, `agent_capabilities`
  - Namespace-based isolation for multi-tenant agent environments
  - 18 unit tests + 9 integration tests
- **MCP dispatch benchmark** — Criterion benchmark suite measuring dispatch latency for 5 representative tool paths
- **Benchmark baseline scripts** — `scripts/bench-baseline.sh` and `scripts/bench-compare.sh` for managing Criterion baselines

### Changed

- Schema: v16 → v17 (additive: `agents` table with indexes)
- `serve_http()` now accepts `Option<RealtimeManager>` for SSE support
- `RealtimeManager` always created in server (not gated on WebSocket port)
- MCP tools: 155+ → 161+ (6 new agent registry tools)

---

## [0.6.0] - 2026-03-09

### Added

- **MCP 2025-11-25 protocol upgrade** — Updated from 2024-11-05 to 2025-11-25 with backward compatibility
- **Tool annotations** — All 155+ MCP tools classified with readOnlyHint, destructiveHint, idempotentHint per MCP spec
- **MCP Resources** — 5 resource URI templates: `engram://memory/{id}`, `engram://workspace/{name}`, `engram://workspace/{name}/memories`, `engram://stats`, `engram://entities`
- **MCP Prompts** — 4 guided workflow prompts: create-knowledge-base, daily-review, search-and-organize, seed-entity
- **Streamable HTTP transport** — Axum-based HTTP transport with `--transport http|stdio|both`, bearer token auth, CORS
- **Server modularization** — Extracted 6200-line server.rs into 11 domain handler modules
- **Semantic duplicate detection** — `memory_find_semantic_duplicates` MCP tool
  - Cosine similarity over embeddings for LLM-powered dedup
  - Configurable threshold, workspace scoping, bounded by limit
- **Procedural memory tracking** — Phase 1 complete
  - `memory_get_timeline`: query episodic memories by time range
  - `memory_get_procedures`: query procedural memories by trigger pattern/success rate
  - `memory_record_procedure_outcome`: increment success/failure counters
- **Retention policies** — automated memory lifecycle management
  - Schema v16: `retention_policies` table
  - 5 MCP tools: `retention_policy_set/get/list/delete/apply`
  - 3-phase apply: auto-compress → max memory enforcement → hard age limit
  - Background compression scheduler (configurable interval)
  - Dry-run mode for previewing policy effects
- **Python SDK** (`sdks/python/`) — `engram-client` 0.1.0 for PyPI
- **TypeScript SDK** (`sdks/typescript/`) — `engram-client` 0.1.0 for npm

### Changed

- MCP protocol: v2024-11-05 → v2025-11-25
- CI: Criterion benchmark tracking with regression alerts (15% PR threshold, 20% nightly)
- Schema: v15 → v16 (additive: `retention_policies` table)

---

## [0.5.0] - 2026-02-13

### Added - Meilisearch Integration (Phase 7)

All 10 planned phases (0-9) are now complete.

#### Phase 7: Meilisearch Backend (ENG-58)

**MeilisearchBackend** - Full `StorageBackend` implementation backed by Meilisearch:
- All 18 core trait methods implemented (CRUD, batch, search, tags, workspaces, stats)
- Meilisearch filter syntax for scope, workspace, tier, tags, lifecycle state
- Facet distribution for tag/workspace listing
- Configurable via `--meilisearch-url` and `--meilisearch-api-key` CLI args
- Feature-gated behind `--features meilisearch` (not in defaults)
- Graph operations intentionally unsupported (Meilisearch has no graph model)

**MeilisearchIndexer** - SQLite → Meilisearch sync engine:
- Full sync with paginated reads (100 items/batch)
- Incremental sync using `updated_at` timestamp tracking
- Configurable interval via `--meilisearch-sync-interval` (default: 60s)
- Background thread with automatic startup

**MCP Tools** (4 new, feature-gated):
- `meilisearch_search` - Search via Meilisearch backend directly
- `meilisearch_reindex` - Trigger full re-sync from SQLite
- `meilisearch_status` - Index stats and health check
- `meilisearch_config` - Current Meilisearch configuration

**Environment Variables:**
- `MEILISEARCH_URL` - Meilisearch server URL
- `MEILISEARCH_API_KEY` - API key (optional)
- `MEILISEARCH_INDEXER` - Enable background sync (default: false)
- `MEILISEARCH_SYNC_INTERVAL` - Sync interval in seconds (default: 60)

### Fixed
- `count_memories` now applies all filters (tags, type, metadata, scope, workspace, tier, archived, expired) instead of only workspace
- `metadata_value_to_param` visibility changed to `pub(crate)` for reuse in count query
- Resolved 20 clippy `await_holding_lock` warnings in Turso backend by switching to `tokio::sync::RwLock`

### Changed
- Published to crates.io as `engram-core` (lib name remains `engram` for API compatibility)
- 144+ MCP tools total (4 new Meilisearch tools)

## [0.4.0] - 2026-02-12

### Added - Salience & Context Quality (Phases 8-9)

This release adds intelligent memory prioritization through salience scoring and comprehensive context quality management.

#### Phase 8: Salience & Session Memory (ENG-66 to ENG-77)

**Salience Scoring** - Dynamic memory relevance based on multiple signals:
- `salience_get` - Get salience score with component breakdown (recency, frequency, importance, feedback)
- `salience_set_importance` - Set user importance score
- `salience_boost` - Boost memory salience temporarily/permanently
- `salience_demote` - Demote memory salience
- `salience_decay_run` - Run temporal decay, update lifecycle states (Active → Stale → Archived)
- `salience_stats` - Get salience distribution statistics
- `salience_history` - Get salience score history for a memory
- `salience_top` - Get top memories by salience score

**Salience Formula:**
```
Salience = (Recency * 0.3) + (Frequency * 0.2) + (Importance * 0.3) + (Feedback * 0.2)
```

**Session Context** - Conversation-scoped memory tracking:
- `session_context_create` - Create a new session context
- `session_context_add_memory` - Add memory to session with relevance score and role
- `session_context_remove_memory` - Remove memory from session
- `session_context_get` - Get session with linked memories
- `session_context_list` - List all sessions with filtering
- `session_context_search` - Search within a specific session
- `session_context_update_summary` - Update session summary
- `session_context_end` - End session context
- `session_context_export` - Export session for archival

**Schema v14:**
- `salience_history` table with component scores for trend tracking
- `session_memories` table with relevance scoring and context roles
- `sessions` table extended with `summary`, `context`, and `ended_at`

#### Phase 9: Context Quality (ENG-48 to ENG-66)

**Quality Scoring** - 5-component weighted quality assessment:
- `quality_score` - Get quality breakdown (clarity, completeness, freshness, consistency, source_trust)
- `quality_report` - Generate comprehensive workspace quality report
- `quality_improve` - Get actionable suggestions to improve quality

**Quality Formula:**
```
Quality = (Clarity * 0.25) + (Completeness * 0.20) + (Freshness * 0.20) + 
          (Consistency * 0.20) + (Source_Trust * 0.15)
```

**Near-Duplicate Detection** - Text similarity using character n-gram Jaccard index:
- `quality_find_duplicates` - Find near-duplicate memories above threshold
- `quality_get_duplicates` - Get pending duplicate candidates for review

**Conflict Detection** - Identify contradictions, staleness, and semantic overlaps:
- `quality_find_conflicts` - Detect conflicts for a memory
- `quality_get_conflicts` - Get unresolved conflicts
- `quality_resolve_conflict` - Resolve conflicts (keep_a, keep_b, merge, keep_both, delete_both, false_positive)

**Source Trust** - Credibility scoring by origin:
- `quality_source_trust` - Get/update trust score for source types
- Default trust: user (0.9) > seed (0.7) > extraction (0.6) > inference (0.5) > external (0.5)

**Schema v15:**
- `quality_score`, `validation_status` columns on memories
- `quality_history` table with component breakdown
- `memory_conflicts` table for contradiction tracking
- `source_trust_scores` table for credibility management
- `duplicate_candidates` table for deduplication cache

### Changed
- Schema version updated to v15
- 26 new MCP tools (17 Phase 8 + 9 Phase 9)

---

## [0.3.0] - 2026-01-30

### Added - Context Engineering Platform (Phases 1-5)

This release transforms Engram from a memory storage system into a **context engineering platform** with cognitive memory types, compression, observability, and lifecycle management.

#### Phase 1: Cognitive Memory Types (ENG-33)
- `memory_create_episodic` - Create event-based memories with temporal context
- `memory_create_procedural` - Create workflow/how-to pattern memories
- `memory_get_timeline` - Query memories by time range
- `memory_get_procedures` - List learned procedures
- New memory types: `Episodic`, `Procedural`, `Summary`, `Checkpoint`
- Schema fields: `event_time`, `event_duration_seconds`, `trigger_pattern`, `procedure_success_count`, `procedure_failure_count`, `summary_of_id`

#### Phase 2: Context Compression Engine (ENG-34)
- `memory_summarize` - Create summary from multiple memories
- `memory_soft_trim` - Trim content preserving head (60%) + tail (30%)
- `context_budget_check` - Check token usage against budget with tiktoken-rs
- `memory_get_full` - Get original content from summary memory
- `memory_archive_old` - Batch archive old low-importance memories
- Token counting with explicit error handling (no silent fallbacks)
- Support for GPT-4, GPT-4o, Claude model encodings

#### Phase 3: Langfuse Integration (ENG-35) - Feature-gated
Requires `--features langfuse` to compile.
- `langfuse_connect` - Configure Langfuse API credentials
- `langfuse_sync` - Background sync traces to memories (returns task_id)
- `langfuse_sync_status` - Check async task status
- `langfuse_extract_patterns` - Extract patterns without saving (preview mode)
- `memory_from_trace` - Create memory from specific trace ID
- Dedicated Tokio runtime for async Langfuse operations
- Pattern extraction: successful prompts, error patterns, user preferences, tool usage

#### Phase 4: Search Result Caching (ENG-36)
- `search_cache_stats` - Cache hit rate, entry count, current threshold
- `search_cache_clear` - Clear cache with optional workspace filter
- `search_cache_feedback` - Report positive/negative result quality
- Adaptive similarity threshold (0.85-0.98) based on feedback
- Embedding-based similarity lookup for semantically similar queries
- TTL-based expiration (default 5 minutes)
- Automatic invalidation on memory create/update/delete

#### Phase 5: Memory Lifecycle Management (ENG-37)
- `lifecycle_status` - Get active/stale/archived counts by workspace
- `lifecycle_run` - Trigger lifecycle cycle (dry_run supported)
- `memory_set_lifecycle` - Manually set lifecycle state
- `lifecycle_config` - Get/set lifecycle configuration
- Lifecycle states: `Active`, `Stale` (30 days), `Archived` (90 days)
- Archived memories excluded from search/list by default

### Changed
- Schema version updated to v13
- 21 new MCP tools (110+ total)
- Updated ROADMAP.md with completion status
- Updated README.md with new tool documentation

### Fixed
- Enabled Phase 1 cognitive memory tools in MCP router (were commented out)
- Fixed 6 compiler warnings (unused imports/variables)

## [0.2.0] - 2026-01-28

### Added - Memora Feature Parity

This release brings Engram to full feature parity with [Memora](https://github.com/limaronaldo/memora), adding 24 new MCP tools.

#### Batch Operations
- `memory_create_batch` - Create multiple memories in a single transaction
- `memory_delete_batch` - Delete multiple memories efficiently

#### Tag Utilities
- `memory_tags` - List all tags with usage counts and timestamps
- `memory_tag_hierarchy` - View tags as hierarchical tree (slash-separated paths)
- `memory_validate_tags` - Check tag consistency, find duplicates and orphans

#### Import/Export
- `memory_export` - Export memories to JSON format for backup/migration
- `memory_import` - Import memories from JSON with deduplication support

#### Maintenance Tools
- `memory_rebuild_embeddings` - Regenerate missing embeddings
- `memory_rebuild_crossrefs` - Rebuild cross-reference links

#### Special Memory Types
- `memory_create_section` - Create hierarchical section memories
- `memory_checkpoint` - Create session state checkpoints
- `memory_boost` - Temporarily increase memory importance

#### Event System
- `memory_events_poll` - Poll for change events (create, update, delete, etc.)
- `memory_events_clear` - Clean up old events
- New `memory_events` table for tracking all changes

#### Advanced Sync
- `sync_version` - Get current sync version and checksum
- `sync_delta` - Get changes since a specific version
- `sync_state` - Get/update per-agent sync state
- `sync_cleanup` - Clean up old sync data
- New `agent_sync_state` table for tracking agent sync progress

#### Multi-Agent Sharing
- `memory_share` - Share a memory with another agent
- `memory_shared_poll` - Poll for memories shared with this agent
- `memory_share_ack` - Acknowledge receipt of shared memory
- New `shared_memories` table for tracking shares

#### Search Variants
- `memory_search_by_identity` - Search by entity name or alias
- `memory_session_search` - Search within session transcript chunks

#### Image Handling
- `memory_upload_image` - Upload image file and attach to memory
- `memory_migrate_images` - Migrate base64 images to file storage
- New `image_storage` module for local file management

#### Content Utilities
- `memory_soft_trim` - Intelligent content truncation preserving context
- `memory_list_compact` - Compact memory listing with previews
- `memory_content_stats` - Get content statistics (words, sentences, etc.)
- New `content_utils` module for text processing

### Changed
- Schema version updated to 10
- Added `dirs` dependency for cross-platform data directories

### Documentation
- Added `IMPROVEMENTS.md` with detailed feature documentation
- Full comparison with Memora feature set

## [0.1.0] - 2025-01-23

### Added

#### Core Infrastructure
- SQLite storage with WAL mode for crash recovery
- Connection pooling with read/write separation
- Database migrations system
- Memory versioning and history tracking

#### Search
- Hybrid search combining BM25 keyword and vector semantic search
- BM25 full-text search with relevance scoring
- Vector similarity search using sqlite-vec
- Fuzzy search with typo tolerance (Levenshtein distance)
- Search result explanations
- Advanced metadata query syntax
- Aggregation queries (count, group by, date histograms)
- Adaptive search strategy selection

#### Embeddings
- TF-IDF embeddings (default, no external dependencies)
- OpenAI embeddings support
- Async embedding queue with batch processing
- Background embedding computation

#### Cloud Sync
- S3-compatible cloud storage (AWS S3, Cloudflare R2, MinIO)
- Background sync with debouncing
- AES-256 encryption for cloud storage
- Conflict resolution with three-way merge
- Cloud-safe storage mode

#### Authentication & Authorization
- Multi-user support with namespace isolation
- API key authentication
- Permission-based access control
- Audit logging

#### Knowledge Graph
- Automatic cross-reference discovery
- Confidence decay for stale relationships
- Rich relation metadata
- Point-in-time graph queries
- Graph visualization with vis.js
- DOT and GEXF export formats
- Community detection (label propagation)
- Graph statistics and centrality metrics

#### AI-Powered Features
- Smart memory suggestions from conversation context
- Automatic memory consolidation (duplicate detection)
- Memory quality scoring
- Natural language command parsing
- Auto-capture mode for proactive memory

#### Real-time
- WebSocket support for live updates

#### Protocol Support
- MCP (Model Context Protocol) server
- HTTP REST API
- CLI interface

### Security
- Encrypted cloud storage
- API key management
- Permission system

---

## Version History

- **0.11.0** - Cognitive Evolution & Platform Excellence (Phases E-K) — 46+ new MCP tools, schema v30
- **0.8.1** - Reactive Infrastructure (Phase 11) — Emergent graph, document ingestion
- **0.7.0** - SSE Event Streaming, Agent Registry (Phase 11)
- **0.6.0** - MCP Modernization (Phase 10) — Resources, Prompts, HTTP transport
- **0.5.0** - Meilisearch Integration (Phase 7)
- **0.4.1** - Published as engram-core on crates.io
- **0.4.0** - Salience & Context Quality (Phases 8-9)
- **0.3.0** - Context Engineering Platform (Phases 1-5)
- **0.2.0** - Memora Feature Parity (24 new tools)
- **0.1.0** - Initial release with full feature set

[0.11.0]: https://github.com/limaronaldo/engram/compare/v0.8.1...v0.11.0
[0.8.1]: https://github.com/limaronaldo/engram/compare/v0.7.0...v0.8.1
[0.7.0]: https://github.com/limaronaldo/engram/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/limaronaldo/engram/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/limaronaldo/engram/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/limaronaldo/engram/releases/tag/v0.4.1
[0.4.0]: https://github.com/limaronaldo/engram/releases/tag/v0.4.0
[0.3.0]: https://github.com/limaronaldo/engram/releases/tag/v0.3.0
[0.2.0]: https://github.com/limaronaldo/engram/releases/tag/v0.2.0
[0.1.0]: https://github.com/limaronaldo/engram/releases/tag/v0.1.0
