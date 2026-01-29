# Changelog

All notable changes to Engram will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

- **0.1.0** - Initial release with full feature set

[Unreleased]: https://github.com/limaronaldo/engram/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/limaronaldo/engram/releases/tag/v0.1.0
