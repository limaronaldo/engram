# Engram - AI Agent Instructions

## Project Overview

Engram is a high-performance AI memory infrastructure written in Rust. It provides persistent memory for AI agents with semantic search, knowledge graphs, and cloud sync capabilities.

## Architecture

- **Storage Layer**: SQLite with WAL mode, sqlite-vec for vectors
- **Search Layer**: Hybrid search (BM25 + semantic + fuzzy)
- **Intelligence Layer**: Auto-capture, quality scoring, consolidation
- **Sync Layer**: S3-compatible cloud storage with AES-256 encryption
- **Interfaces**: MCP protocol, REST API, WebSocket, CLI

## Key Design Decisions

1. **Single Binary**: All functionality in one Rust binary for easy deployment
2. **SQLite**: Embedded database for zero-configuration setup
3. **Hybrid Search**: Combines keyword (BM25) and semantic (vector) search with RRF fusion
4. **MCP-First**: Primary interface is Model Context Protocol for AI agent integration

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

## MCP Tools

Primary tools for AI agents:
- `memory_create` / `memory_update` / `memory_delete` - CRUD operations
- `memory_search` - Hybrid search with fuzzy correction
- `memory_link` / `memory_unlink` - Cross-reference management
- `memory_scan_project` - Ingest instruction files as memories
- `memory_get_project_context` - Retrieve project-specific memories

## Testing

```bash
# Run all tests
cargo test

# Run specific module
cargo test storage::
cargo test search::

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

## Environment Variables

| Variable | Description |
|----------|-------------|
| `ENGRAM_DB_PATH` | SQLite database path |
| `ENGRAM_STORAGE_MODE` | `local` or `cloud-safe` |
| `ENGRAM_CLOUD_URI` | S3 URI for sync |
| `ENGRAM_CLOUD_ENCRYPT` | Enable encryption |

## Performance Targets

- Create: < 200 µs
- Get by ID: < 100 µs
- Keyword search: < 5 ms
- Hybrid search: < 10 ms
- Support 100K+ memories with sub-10ms search
