# Changelog

All notable changes to Engram will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

- **0.4.0** - Salience & Context Quality (Phases 8-9)
- **0.3.0** - Context Engineering Platform (Phases 1-5)
- **0.2.0** - Memora Feature Parity (24 new tools)
- **0.1.0** - Initial release with full feature set

[Unreleased]: https://github.com/limaronaldo/engram/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/limaronaldo/engram/releases/tag/v0.4.0
[0.3.0]: https://github.com/limaronaldo/engram/releases/tag/v0.3.0
[0.2.0]: https://github.com/limaronaldo/engram/releases/tag/v0.2.0
[0.1.0]: https://github.com/limaronaldo/engram/releases/tag/v0.1.0
