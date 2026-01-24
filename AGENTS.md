# Engram - AI Agent Instructions

## Project Overview

Engram is a high-performance AI memory infrastructure written in Rust. It provides persistent memory for AI agents with semantic search, knowledge graphs, entity extraction, and cloud sync capabilities.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         MCP Interface                           │
│              (Model Context Protocol for AI Agents)             │
├─────────────────────────────────────────────────────────────────┤
│                        Intelligence Layer                        │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌────────────┐ │
│  │ Entity      │ │ Project     │ │ Auto        │ │ Quality    │ │
│  │ Extraction  │ │ Context     │ │ Capture     │ │ Scoring    │ │
│  └─────────────┘ └─────────────┘ └─────────────┘ └────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│                          Search Layer                            │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌────────────┐ │
│  │ Hybrid      │ │ Semantic    │ │ Fuzzy       │ │ Reranking  │ │
│  │ (BM25+Vec)  │ │ (Embeddings)│ │ (Typo-tol)  │ │ (Multi-sig)│ │
│  └─────────────┘ └─────────────┘ └─────────────┘ └────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│                         Storage Layer                            │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌────────────┐ │
│  │ SQLite      │ │ Graph       │ │ Entities    │ │ Cloud Sync │ │
│  │ (WAL mode)  │ │ (Cross-refs)│ │ (NER store) │ │ (S3/R2)    │ │
│  └─────────────┘ └─────────────┘ └─────────────┘ └────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

- **Storage Layer**: SQLite with WAL mode, sqlite-vec for vectors
- **Search Layer**: Hybrid search (BM25 + semantic + fuzzy) with reranking
- **Intelligence Layer**: Entity extraction, project context, auto-capture, quality scoring
- **Sync Layer**: S3-compatible cloud storage with AES-256 encryption
- **Interfaces**: MCP protocol, REST API, WebSocket, CLI

## Key Design Decisions

1. **Single Binary**: All functionality in one Rust binary for easy deployment
2. **SQLite**: Embedded database for zero-configuration setup
3. **Hybrid Search**: Combines keyword (BM25) and semantic (vector) search with RRF fusion
4. **MCP-First**: Primary interface is Model Context Protocol for AI agent integration
5. **Idempotent Operations**: Re-running operations (entity extraction, project scan) is safe

## Code Conventions

### Rust Style
- Use `thiserror` for error types
- Use `parking_lot` for synchronization primitives
- Prefer `with_connection` and `with_transaction` patterns for database access
- All public APIs should return `Result<T, EngramError>`

### Module Organization
- Each major feature has its own module under `src/`
- Tests are co-located with implementation (`#[cfg(test)]` modules)
- Benchmarks in `benches/` directory

### Naming
- Memory types: `Note`, `Todo`, `Issue`, `Decision`, `Preference`, `Learning`, `Context`, `Credential`
- Edge types: `RelatedTo`, `Supersedes`, `Contradicts`, `Implements`, `Extends`, `References`, `DependsOn`, `Blocks`, `FollowsUp`
- Entity types: `Person`, `Organization`, `Project`, `Technology`, `Concept`, `Location`, `Event`

## MCP Tools Reference

### Core Memory Operations

| Tool | Description |
|------|-------------|
| `memory_create` | Create a new memory with content, type, tags, metadata |
| `memory_get` | Retrieve a memory by ID |
| `memory_update` | Update memory content, tags, or metadata |
| `memory_delete` | Delete a memory by ID |
| `memory_list` | List memories with optional filters |

### Search Tools

| Tool | Description |
|------|-------------|
| `memory_search` | Hybrid search (BM25 + semantic) with fuzzy correction and reranking |
| `memory_semantic_search` | Pure vector similarity search |
| `memory_suggest` | Spelling suggestions for search queries |

### Graph & Cross-Reference Tools

| Tool | Description |
|------|-------------|
| `memory_related` | Get related memories via cross-references. When `depth > 1` or `include_entities = true`, returns `TraversalResult` with `discovery_edges` instead of flat list |
| `memory_link` | Create explicit typed link between memories |
| `memory_unlink` | Remove link between memories |
| `memory_traverse` | Multi-hop graph traversal with BFS algorithm |
| `memory_find_path` | Find shortest path between two memories |

### Entity Extraction Tools (RML-925)

| Tool | Description |
|------|-------------|
| `memory_extract_entities` | Extract named entities (people, orgs, projects, concepts) from a memory. Idempotent: re-running does not inflate mention counts |
| `memory_get_entities` | Get all entities linked to a memory |
| `memory_search_entities` | Search entities by name pattern |
| `memory_entity_stats` | Get entity statistics (counts by type, top entities) |

### Project Context Tools (RML-928)

| Tool | Description |
|------|-------------|
| `memory_scan_project` | Scan directory for AI instruction files (CLAUDE.md, .cursorrules, AGENTS.md, etc.) and ingest as memories. Creates parent + section memories. Idempotent with content hashing |
| `memory_get_project_context` | Retrieve project context memories for a directory |

**Supported instruction files:**
- `CLAUDE.md` - Claude Code instructions
- `AGENTS.md` - Multi-agent system instructions
- `.cursorrules` - Cursor IDE rules
- `.github/copilot-instructions.md` - GitHub Copilot instructions
- `GEMINI.md` - Gemini tools instructions
- `.aider.conf.yml` - Aider configuration
- `CONVENTIONS.md`, `CODING_GUIDELINES.md` - General conventions
- `.windsurfrules` - Windsurf IDE rules

### Scoping Tools (RML-924)

| Tool | Description |
|------|-------------|
| `memory_create` (with `scope`) | Create memory with scope: `Global`, `User(id)`, `Session(id)`, `Agent(id)` |
| `memory_list` (with `scope`) | Filter memories by scope |

### Administrative Tools

| Tool | Description |
|------|-------------|
| `memory_stats` | Get storage statistics |
| `memory_rebuild_embeddings` | Regenerate all embeddings |
| `memory_rebuild_crossrefs` | Regenerate cross-reference graph |

## Feature Implementation Status

| Feature | Module | Status |
|---------|--------|--------|
| Memory Scoping | `src/types.rs`, `src/storage/` | ✅ Complete |
| Entity Extraction (NER) | `src/intelligence/entities.rs` | ✅ Complete |
| Multi-hop Graph Queries | `src/storage/graph_queries.rs` | ✅ Complete |
| Search Reranking | `src/search/rerank.rs` | ✅ Complete |
| Document Ingestion | `src/intelligence/project_context.rs` | ✅ Complete |
| Hybrid Search | `src/search/hybrid.rs` | ✅ Complete |
| Fuzzy Search | `src/search/fuzzy.rs` | ✅ Complete |
| Cloud Sync | `src/sync/` | ✅ Complete |
| Auto-capture | `src/intelligence/auto_capture.rs` | ✅ Complete |
| Quality Scoring | `src/intelligence/quality.rs` | ✅ Complete |

## Database Schema (v4)

```sql
-- Core tables
memories (id, content, memory_type, tags, metadata, importance, 
          scope, owner_id, visibility, created_at, updated_at)
          
cross_references (source_id, target_id, edge_type, weight, 
                  confidence, created_at, updated_at)

-- Entity tables (RML-925)
entities (id, name, normalized_name, entity_type, mention_count, 
          first_seen_at, last_seen_at, metadata)
          
memory_entities (memory_id, entity_id, relationship_type, 
                 confidence, context_snippet, created_at)

-- FTS and vector search
memories_fts (FTS5 virtual table for full-text search)
memory_vectors (sqlite-vec for semantic search)
```

## Testing

```bash
# Run all tests (150 tests)
cargo test

# Run with output
cargo test -- --nocapture

# Run specific module
cargo test storage::
cargo test search::
cargo test intelligence::

# Run benchmarks
cargo bench
```

## Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# The binaries are:
# - engram-server (MCP/HTTP server)
# - engram-cli (command-line interface)
```

## Environment Configuration

### Configuration Files

| File | Purpose |
|------|---------|
| `.env.local` | Local secrets and credentials (not committed) |
| `.env.example` | Template with all available options |

### Core Settings

| Variable | Description | Default |
|----------|-------------|---------|
| `ENGRAM_DB_PATH` | SQLite database path | `~/.local/share/engram/memories.db` |
| `ENGRAM_STORAGE_MODE` | `local` or `cloud-safe` | `local` |
| `ENGRAM_CLOUD_URI` | S3 URI for sync | - |
| `ENGRAM_CLOUD_ENCRYPT` | Enable AES-256 encryption | `false` |
| `ENGRAM_LOG_LEVEL` | Logging level | `info` |
| `ENGRAM_HTTP_PORT` | HTTP server port | `8080` |
| `ENGRAM_WS_PORT` | WebSocket server port | `8081` |

### Cloud Storage (Cloudflare R2)

| Variable | Description |
|----------|-------------|
| `CLOUDFLARE_ACCOUNT_ID` | Cloudflare account ID |
| `R2_ACCESS_KEY_ID` | R2 API access key |
| `R2_SECRET_ACCESS_KEY` | R2 API secret key |
| `R2_ENDPOINT_URL` | R2 endpoint (`https://<account>.r2.cloudflarestorage.com`) |
| `AWS_PROFILE` | AWS CLI profile for S3 operations |
| `AWS_ENDPOINT_URL` | Custom S3 endpoint (for R2/MinIO) |

### External Services

| Variable | Description |
|----------|-------------|
| `DATABASE_URL` | PostgreSQL connection string (future use) |
| `NEON_DATABASE_URL` | Neon PostgreSQL connection |
| `LINEAR_API_KEY` | Linear API for project management |
| `OPENAI_API_KEY` | OpenAI API for embeddings (if using openai model) |

### Embedding Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `ENGRAM_EMBEDDING_MODEL` | `tfidf` or `openai` | `tfidf` |
| `OPENAI_API_KEY` | Required if using openai embeddings | - |

## Performance Targets

| Operation | Target | Notes |
|-----------|--------|-------|
| Create | < 200 µs | Without embedding |
| Get by ID | < 100 µs | Direct lookup |
| Keyword search | < 5 ms | BM25 via FTS5 |
| Hybrid search | < 10 ms | BM25 + vector + rerank |
| Entity extraction | < 50 ms | Regex-based NER |
| Project scan | < 500 ms | Per instruction file |

Support 100K+ memories with sub-10ms search.

## Key Files

| File | Purpose |
|------|---------|
| `src/bin/server.rs` | MCP server with all tool handlers |
| `src/storage/mod.rs` | Core CRUD operations |
| `src/storage/graph_queries.rs` | Multi-hop traversal, path finding |
| `src/storage/entity_queries.rs` | Entity storage and linking |
| `src/search/hybrid.rs` | Hybrid search with RRF fusion |
| `src/search/rerank.rs` | Multi-signal result reranking |
| `src/intelligence/entities.rs` | NER extraction engine |
| `src/intelligence/project_context.rs` | Instruction file parsing |
| `src/mcp/tools.rs` | MCP tool definitions |

## Recent Changes

### v4 Migration (2025-01)
- Added `mention_count` backfill from `memory_entities` table
- Fixed entity extraction idempotency (no count inflation on re-extraction)
- Renamed `TraversalResult.edges` to `discovery_edges` for clarity
- Added `include_entities` parameter to `memory_related` (default: false)
