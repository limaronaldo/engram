# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
# Build
cargo build                    # Debug build
cargo build --release          # Release build (LTO enabled)

# Test
cargo test                     # Run all 127 tests
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
Interfaces (MCP/REST/WebSocket/CLI)
         ↓
Intelligence Layer (auto-capture, suggestions, consolidation)
         ↓
Search Layer (BM25 + vectors + fuzzy → RRF fusion)
         ↓
Storage Layer (SQLite WAL + sqlite-vec)
```

### Key Data Flow

1. **Memory creation**: `server.rs` → `storage/queries.rs::create_memory()` → SQLite + FTS5 index
2. **Search**: `search/hybrid.rs::hybrid_search()` combines BM25 (`bm25.rs`) and vector search, fuses with RRF
3. **Project context**: `intelligence/project_context.rs` scans for instruction files, creates memories with `project-context` tag

### Core Types (src/types.rs)

- `Memory` - Main entity with content, type, tags, metadata, importance
- `MemoryType` - Enum: Note, Todo, Issue, Decision, Preference, Learning, Context, Credential
- `EdgeType` - Cross-reference types: RelatedTo, Supersedes, Contradicts, DependsOn, etc.
- `SearchResult` - Memory + score + optional match explanation

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

## Code Conventions

- Error handling: Return `Result<T, EngramError>`, use `thiserror` for error types
- Synchronization: Use `parking_lot::Mutex` over std
- Serialization: `serde` with `#[serde(rename_all = "lowercase")]` for enums
- Tests: Co-located in `#[cfg(test)]` modules within each file
- Metadata queries: Use `json_extract()` SQL function for filtering by metadata fields
