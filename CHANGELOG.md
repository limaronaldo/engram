# Changelog

All notable changes to Engram will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
