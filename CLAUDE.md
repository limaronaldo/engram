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
- `EntityType` - Enum: Person, Organization, Project, Concept, Location, DateTime, Reference, Other
- `SearchResult` - Memory + score + optional match explanation

### Database Schema (v4)

```sql
-- Core tables
memories (id, content, memory_type, importance, quality_score,
          scope, owner_id, source, created_at, updated_at)

memory_tags (memory_id, tag)

memory_metadata (memory_id, key, value)

crossrefs (source_id, target_id, edge_type, weight, 
           confidence, created_at, updated_at)

-- Entity tables
entities (id, name, normalized_name, entity_type, mention_count, 
          first_seen_at, last_seen_at, metadata)
          
memory_entities (memory_id, entity_id, relationship_type, 
                 confidence, context_snippet, created_at)

-- Search indexes
memories_fts (FTS5 virtual table)
embeddings (memory_id, embedding BLOB via sqlite-vec)
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
| `ENGRAM_STORAGE_URI` | S3 URI for cloud sync (e.g., `s3://bucket/path`) | - |
| `ENGRAM_CLOUD_ENCRYPT` | Enable AES-256 encryption for cloud sync | `false` |
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
| OpenRouter Support (RML-984) | `src/embedding/mod.rs`, `src/bin/server.rs` | ✅ Complete |
| memory_list_instruction_files (RML-985) | `src/bin/server.rs` | ✅ Complete |
| memory_get_identities (RML-986) | `src/bin/server.rs`, `src/storage/identity_links.rs` | ✅ Complete |
| Document Ingestion (PDF/MD) | `src/intelligence/document_ingest.rs`, `src/bin/server.rs` | ✅ Complete |

**Note:** Project Context Scanning ingests AI instruction files (CLAUDE.md, .cursorrules, etc.) via `memory_scan_project`. Document ingestion is available via `memory_ingest_document` for Markdown/PDF with chunking and metadata (`source_path`, `section_path`, `page`, `doc_id`).

## Recent Enhancements (January 29, 2026)

### OpenRouter Support (RML-984)

Engram now supports OpenRouter and other OpenAI-compatible embedding providers via configurable base URL, model, and dimensions.

**New Environment Variables:**

| Variable | Description | Default |
|----------|-------------|---------|
| `OPENAI_BASE_URL` | OpenAI-compatible API base URL | `https://api.openai.com/v1` |
| `OPENAI_EMBEDDING_MODEL` | Embedding model name | `text-embedding-3-small` |
| `OPENAI_EMBEDDING_DIMENSIONS` | Embedding dimensions (auto-detected if not set) | `1536` for OpenAI |

**Example MCP Configuration (OpenRouter):**

```json
{
  "mcpServers": {
    "engram": {
      "command": "/path/to/engram-server",
      "env": {
        "ENGRAM_DB_PATH": "/path/to/memories.db",
        "ENGRAM_EMBEDDING_MODEL": "openai",
        "OPENAI_API_KEY": "${OPENROUTER_API_KEY}",
        "OPENAI_BASE_URL": "https://openrouter.ai/api/v1",
        "OPENAI_EMBEDDING_MODEL": "openai/text-embedding-3-small",
        "OPENAI_EMBEDDING_DIMENSIONS": "1536"
      }
    }
  }
}
```

**Dimension Validation:** The embedder validates that returned embeddings match the configured dimensions and returns a helpful error message if they don't.

### New MCP Tools (RML-985, RML-986)

**memory_list_instruction_files** - List AI instruction files without ingesting:
```json
{
  "path": "/path/to/project",
  "scan_parents": false
}
// Returns: { files_found: 2, files: [{ path, filename, file_type, format, size, content_hash }] }
```

**memory_get_identities** - Get all identities linked to a memory with mention info:
```json
{
  "id": 42
}
// Returns: { identities_count: 3, identities: [{ canonical_id, display_name, entity_type, mention_text, mention_count, ... }] }
```

**Total MCP Tools:** 91 (vs Memora's 72)

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

---

## Project Roadmap & Linear Issues

### Enhancement Plan Overview

Engram is being transformed from a memory storage system into a **context engineering platform** with cognitive memory types, compression, observability, and quality management.

**Current State:** 91 MCP tools, schema v4, 10 memory types  
**Target State:** 126+ MCP tools, schema v15, 14 memory types + compression + trace ingestion

### Phases Summary

| Phase | Name | Status | Linear Issues |
|-------|------|--------|---------------|
| 0 | Storage Abstraction | In Progress | ENG-14 to ENG-32 (19 issues) |
| 1 | Cognitive Memory Types | Planned | ENG-33 |
| 2 | Context Compression | Planned | ENG-34 |
| 3 | Langfuse Integration | Planned | ENG-35 |
| 4 | Search Result Caching | Planned | ENG-36 |
| 5 | Memory Lifecycle | Planned | ENG-37 |
| 6 | Turso Support | Planned | - |
| 7 | Meilisearch Integration | Planned | - |
| 8 | Salience & Session Memory | Planned | ENG-66 to ENG-77 (12 issues) |
| 9 | Context Quality | Planned | ENG-48 to ENG-66 (19 issues) |

### Phase 0: Storage Abstraction (Foundation)

Create `StorageBackend` trait to support multiple backends (SQLite, Turso, Meilisearch):

```rust
pub trait StorageBackend: Send + Sync {
    fn create_memory(&self, input: CreateMemoryInput) -> Result<Memory>;
    fn get_memory(&self, id: i64) -> Result<Option<Memory>>;
    fn update_memory(&self, id: i64, input: UpdateMemoryInput) -> Result<Memory>;
    fn delete_memory(&self, id: i64) -> Result<()>;
    fn list_memories(&self, options: ListOptions) -> Result<Vec<Memory>>;
    fn search(&self, query: &str, options: SearchOptions) -> Result<Vec<SearchResult>>;
    // ... 40+ methods total
}
```

### Phase 1: Cognitive Memory Types (Schema v12)

New memory types mirroring human cognition:

| Type | Purpose | Example |
|------|---------|---------|
| Episodic | Events with temporal context | "User deployed v2.0 on Jan 15" |
| Procedural | Learned patterns/workflows | "When user asks about auth, check JWT first" |
| Summary | Compressed summaries | Auto-generated from large memories |
| Checkpoint | Conversation state snapshots | Session save points |

New columns:
- `event_time`, `event_duration_seconds` (episodic)
- `trigger_pattern`, `procedure_success_count`, `procedure_failure_count` (procedural)
- `summary_of_id` (summary reference)

### Phase 2: Context Compression

Summary-based compression with token counting:

```rust
pub fn count_tokens(text: &str, model: &str) -> Result<usize>;

pub struct ContextBudgetResult {
    pub total_tokens: usize,
    pub budget: usize,
    pub over_budget: bool,
    pub suggestions: Vec<String>,
}
```

New tools: `memory_summarize`, `memory_get_full`, `context_budget_check`, `memory_archive_old`

### Phase 8: Salience & Session Memory

Adaptive importance scoring based on access patterns:

```rust
salience = base_importance 
         × recency_boost(last_accessed) 
         × frequency_boost(access_count) 
         × feedback_signal(thumbs_up - thumbs_down)
```

Session transcript indexing with overlapping chunks for semantic search.

### Phase 9: Context Quality

Quality detectors and remediation:

| Detector | Purpose |
|----------|---------|
| Bloat | Identifies redundant/verbose memories |
| Stale | Finds outdated information |
| Poisoned | Detects contradictory or harmful content |

Quality dashboard with metrics and automated cleanup.

### Context Seeding (Implemented)

Solves cold start problem with `context_seed` tool:

```rust
// Dynamic TTL by confidence (Option C)
fn ttl_for_confidence(confidence: f32) -> Option<i64> {
    if confidence >= 0.85 { None }              // Permanent
    else if confidence >= 0.60 { Some(90 * 24 * 3600) }  // 90 days
    else { Some(30 * 24 * 3600) }               // 30 days
}
```

Categories: `fact`, `behavior_instruction`, `interest`, `persona`, `preference`

Seeds are tagged `origin:seed`, `status:unverified` with lower retrieval priority than organic memories.

### Documentation

Full documentation in `/docs/`:

| File | Content |
|------|---------|
| `docs/ROADMAP.md` | Complete 9-phase roadmap |
| `docs/SCHEMA.md` | Database schema v4 and migrations |
| `docs/LINEAR_ISSUES.md` | All Linear issues by phase |
| `docs/concepts/WHY_NOT_VECTOR_DB.md` | Why Engram > vector databases |
| `docs/concepts/CONTEXT_SEEDING.md` | Context seeding specification |
| `docs/MEMORA_VS_ENGRAM.md` | Comparison with Memora |

### Why Not Just a Vector DB?

Engram addresses three fundamental limitations of pure vector databases:

1. **The Similarity Trap** - Relevance ≠ Utility. A memory about "Python snake care" is semantically similar to "Python programming" but useless for coding.

2. **The Append-Only Graveyard** - Vector DBs never forget. Without decay, old irrelevant memories pollute results forever.

3. **The Feedback Loop Gap** - No learning from usage. If a memory is retrieved but never useful, vector DBs can't adapt.

**Engram's Utility Formula:**
```
Utility = f(Similarity, Recency, Access Frequency, Feedback, Source Trust)
```

---

**Roadmap Last Updated:** January 29, 2026
