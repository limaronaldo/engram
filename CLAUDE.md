# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
# Build
cargo build                    # Debug build
cargo build --release          # Release build (LTO enabled)

# Test
cargo test                     # Run all 150 tests
cargo test storage::           # Run storage module tests
cargo test search::            # Run search module tests
cargo test intelligence::      # Run intelligence module tests
cargo test -- --nocapture      # Show println! output

# Single test
cargo test test_name           # Run specific test by name

# Lint & Format
cargo fmt                      # Format code
cargo fmt --check              # Check formatting (CI uses this)
cargo clippy --all-targets     # Run linter

# Benchmarks
cargo bench                    # Run all benchmarks
cargo bench search             # Run search benchmarks only

# Binaries produced
target/release/engram-server   # MCP/HTTP server
target/release/engram-cli      # Command-line interface
```

## Architecture

Engram is an AI memory system with four layers:

```
┌─────────────────────────────────────────────────────────────────┐
│                    Interfaces (MCP/REST/WS/CLI)                 │
├─────────────────────────────────────────────────────────────────┤
│                      Intelligence Layer                          │
│  Entity Extraction │ Project Context │ Auto-capture │ Quality   │
├─────────────────────────────────────────────────────────────────┤
│                         Search Layer                             │
│  Hybrid (BM25+Vec) │ Semantic │ Fuzzy │ Reranking (Multi-sig)   │
├─────────────────────────────────────────────────────────────────┤
│                        Storage Layer                             │
│  SQLite (WAL) │ Graph (Cross-refs) │ Entities │ Cloud Sync (S3) │
└─────────────────────────────────────────────────────────────────┘
```

### Key Data Flow

1. **Memory creation**: `server.rs` → `storage/queries.rs::create_memory()` → SQLite + FTS5 index
2. **Search**: `search/hybrid.rs::hybrid_search()` combines BM25 (`bm25.rs`) and vector search, fuses with RRF, then reranks
3. **Entity extraction**: `intelligence/entities.rs::EntityExtractor::extract()` → stores in `entities` + `memory_entities` tables
4. **Project context**: `intelligence/project_context.rs` scans for instruction files, creates memories with `project-context` tag
5. **Graph traversal**: `storage/graph_queries.rs::get_related_multi_hop()` uses BFS with entity connections

### Core Types (src/types.rs)

- `Memory` - Main entity with content, type, tags, metadata, importance, scope
- `MemoryType` - Enum: Note, Todo, Issue, Decision, Preference, Learning, Context, Credential
- `MemoryScope` - Enum: Global, User(id), Session(id), Agent(id)
- `EdgeType` - Cross-reference types: RelatedTo, Supersedes, Contradicts, DependsOn, etc.
- `EntityType` - Enum: Person, Organization, Project, Technology, Concept, Location, Event
- `SearchResult` - Memory + score + optional match explanation

### Database Schema (v4)

```sql
-- Core tables
memories (id, content, memory_type, tags, metadata, importance, 
          scope, owner_id, visibility, created_at, updated_at)
          
cross_references (source_id, target_id, edge_type, weight, 
                  confidence, created_at, updated_at)

-- Entity tables
entities (id, name, normalized_name, entity_type, mention_count, 
          first_seen_at, last_seen_at, metadata)
          
memory_entities (memory_id, entity_id, relationship_type, 
                 confidence, context_snippet, created_at)

-- Search indexes
memories_fts (FTS5 virtual table)
memory_vectors (sqlite-vec)
```

### Database Patterns

All DB access uses connection wrappers in `storage/connection.rs`:

```rust
// Read-only operation
storage.with_connection(|conn| {
    get_memory(conn, id)
})?;

// Write operation (auto-commits on success, rolls back on error)
storage.with_transaction(|conn| {
    create_memory(conn, &input)
})?;
```

### Search Strategy Selection (search/mod.rs)

- Short queries (≤2 words) → KeywordOnly (BM25)
- Long queries (≥8 words) → SemanticOnly (vectors)
- Medium queries → Hybrid (RRF fusion of both)
- Queries with quotes/operators → KeywordOnly

### MCP Tool Implementation

Tools are defined in `mcp/tools.rs` and handled in `bin/server.rs::EngramHandler::handle_tool_call()`. Each tool maps to a method like `tool_memory_create()`, `tool_memory_search()`, etc.

## Environment Configuration

### Required Files

- `.env.local` - Local secrets (not committed)
- `.env.example` - Template for configuration

### Key Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ENGRAM_DB_PATH` | SQLite database path | `~/.local/share/engram/memories.db` |
| `ENGRAM_STORAGE_MODE` | `local` or `cloud-safe` | `local` |
| `ENGRAM_CLOUD_URI` | S3 URI for sync | - |
| `ENGRAM_CLOUD_ENCRYPT` | Enable AES-256 encryption | `false` |
| `DATABASE_URL` | PostgreSQL connection (future use) | - |
| `NEON_DATABASE_URL` | Neon PostgreSQL connection | - |
| `LINEAR_API_KEY` | Linear API for project management | - |

### Cloud Storage (Cloudflare R2)

```bash
CLOUDFLARE_ACCOUNT_ID=<account_id>
R2_ACCESS_KEY_ID=<access_key>
R2_SECRET_ACCESS_KEY=<secret_key>
R2_ENDPOINT_URL=https://<account_id>.r2.cloudflarestorage.com
AWS_PROFILE=memora
```

## Code Conventions

- Error handling: Return `Result<T, EngramError>`, use `thiserror` for error types
- Synchronization: Use `parking_lot::Mutex` over std
- Serialization: `serde` with `#[serde(rename_all = "lowercase")]` for enums
- Tests: Co-located in `#[cfg(test)]` modules within each file
- Metadata queries: Use `json_extract()` SQL function for filtering by metadata fields
- Idempotency: Operations like entity extraction and project scan are safe to re-run

## Feature Implementation Status

| Feature | Module | Status |
|---------|--------|--------|
| Memory Scoping (RML-924) | `src/types.rs`, `src/storage/` | ✅ Complete |
| Entity Extraction (RML-925) | `src/intelligence/entities.rs` | ✅ Complete |
| Multi-hop Graph (RML-926) | `src/storage/graph_queries.rs` | ✅ Complete |
| Search Reranking (RML-927) | `src/search/rerank.rs` | ✅ Complete |
| Project Context Scanning | `src/intelligence/project_context.rs` | ✅ Complete |
| Document Ingestion (PDF/MD) | - | ❌ Not implemented |

**Note:** Project Context Scanning ingests AI instruction files (CLAUDE.md, .cursorrules, etc.) via `memory_scan_project`. Full document ingestion (PDF parsing, chunking) is planned for a future release.

## Key Files Reference

| File | Purpose |
|------|---------|
| `src/bin/server.rs` | MCP server with all tool handlers |
| `src/storage/mod.rs` | Core CRUD operations |
| `src/storage/queries.rs` | Memory queries (create, get, update, delete) |
| `src/storage/graph_queries.rs` | Multi-hop traversal, path finding |
| `src/storage/entity_queries.rs` | Entity storage and linking |
| `src/storage/migrations.rs` | Schema migrations (currently v4) |
| `src/search/hybrid.rs` | Hybrid search with RRF fusion |
| `src/search/rerank.rs` | Multi-signal result reranking |
| `src/search/fuzzy.rs` | Typo-tolerant search |
| `src/intelligence/entities.rs` | NER extraction engine |
| `src/intelligence/project_context.rs` | Instruction file parsing |
| `src/mcp/tools.rs` | MCP tool definitions |
| `src/types.rs` | Core type definitions |
| `src/error.rs` | Error types |
