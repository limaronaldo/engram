# Engram

**Persistent memory for AI agents.** Hybrid search, knowledge graphs, cloud sync — in a single Rust binary.

[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Why Engram

AI agents forget everything between sessions. Context windows overflow. Knowledge scatters across chat logs. **Engram fixes this.**

- **Hybrid search that just works**: BM25 + semantic vectors + fuzzy correction in one call
- **Knowledge graph built in**: cross-references, confidence decay, automatic consolidation
- **Multiple interfaces**: MCP, REST, WebSocket, CLI — pick what fits your stack
- **Cloud sync**: S3-compatible with optional AES-256-GCM encryption
- **Intelligence layer**: auto-capture patterns, quality scoring, natural language queries

## 30-Second Demo

MCP (recommended):

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "memory_create",
    "arguments": {
      "content": "Use async/await for I/O-bound work",
      "type": "learning",
      "tags": ["rust", "async"]
    }
  }
}
```

HTTP (REST):

```bash
# Store a memory
curl -X POST http://localhost:8080/memories \
  -H "Content-Type: application/json" \
  -d '{"content": "Use async/await for I/O-bound work"}'

# Find it with fuzzy search (handles typos)
curl "http://localhost:8080/search?q=asynch+awiat"
# → Returns the memory with match explanation
```

## Built For

- Long-running agents that need reliable memory across sessions
- Team knowledge bases and architectural decision logs
- Project context ingestion from instruction files (CLAUDE.md, AGENTS.md, etc.)
- LLM tools that need fast semantic + keyword retrieval

## Overview

Engram is a high-performance memory system designed for AI agents and assistants. It provides persistent storage for memories, facts, decisions, and learnings with powerful semantic search capabilities and automatic relationship discovery.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Engram Server                           │
├─────────────────────────────────────────────────────────────────┤
│  MCP Protocol (stdio)  │  REST API (HTTP)  │  WebSocket (WS)   │
├─────────────────────────────────────────────────────────────────┤
│                      Intelligence Layer                         │
│  ┌─────────────┐ ┌──────────────┐ ┌─────────────┐ ┌──────────┐ │
│  │ Suggestions │ │ Consolidate  │ │   Quality   │ │ Auto-    │ │
│  │   Engine    │ │    Engine    │ │   Scorer    │ │ Capture  │ │
│  └─────────────┘ └──────────────┘ └─────────────┘ └──────────┘ │
├─────────────────────────────────────────────────────────────────┤
│                        Search Layer                             │
│  ┌─────────────┐ ┌──────────────┐ ┌─────────────┐ ┌──────────┐ │
│  │  Hybrid     │ │    BM25      │ │   Vector    │ │  Fuzzy   │ │
│  │  Search     │ │   Search     │ │   Search    │ │  Search  │ │
│  └─────────────┘ └──────────────┘ └─────────────┘ └──────────┘ │
├─────────────────────────────────────────────────────────────────┤
│                       Storage Layer                             │
│  ┌─────────────┐ ┌──────────────┐ ┌─────────────┐ ┌──────────┐ │
│  │   SQLite    │ │ sqlite-vec   │ │   Cloud     │ │  Version │ │
│  │    WAL      │ │  Vectors     │ │   Sync      │ │  History │ │
│  └─────────────┘ └──────────────┘ └─────────────┘ └──────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/limaronaldo/engram.git
cd engram

# Build release binary
cargo build --release

# Install binaries
cargo install --path .
```

### Dependencies

- Rust 1.75 or later
- SQLite 3.35+ (bundled)

## Quick Start

### As MCP Server

Add to your MCP configuration (e.g., `~/.config/claude/mcp.json`):

```json
{
  "mcpServers": {
    "memory": {
      "command": "/path/to/engram-server",
      "args": ["--mcp"],
      "env": {
        "ENGRAM_DB_PATH": "~/.local/share/engram/memories.db"
      }
    }
  }
}
```

### As HTTP Server

```bash
# Start the server
engram-server --http --port 8080

# Create a memory
curl -X POST http://localhost:8080/memories \
  -H "Content-Type: application/json" \
  -d '{"content": "Rust is great for systems programming", "type": "learning", "tags": ["rust", "programming"]}'

# Search memories
curl "http://localhost:8080/search?q=rust%20programming&limit=10"
```

### CLI Usage

```bash
# Create a memory
engram-cli create "Always use Result<T, E> for error handling in Rust" --type preference --tags rust,errors

# Search memories
engram-cli search "error handling"

# List recent memories
engram-cli list --limit 20

# Show statistics
engram-cli stats
```

## Project Context Discovery

Engram can ingest AI instruction files and convert them into searchable memories.

Supported core files (Phase 1): see [Supported file patterns](#supported-file-patterns).

### Supported file patterns

| File | Tool/Platform | Format |
|------|---------------|--------|
| `CLAUDE.md` | Claude Code | Markdown |
| `AGENTS.md` | Various agents | Markdown |
| `.cursorrules` | Cursor IDE | Plain text/YAML |
| `.github/copilot-instructions.md` | GitHub Copilot | Markdown |
| `GEMINI.md` | Gemini tools | Markdown |
| `.aider.conf.yml` | Aider | YAML |
| `CONVENTIONS.md` | General | Markdown |
| `.windsurfrules` | Windsurf IDE | Plain text |

Example MCP calls:

```json
{"tool": "memory_scan_project", "arguments": {"path": ".", "extract_sections": true}}
{"tool": "memory_get_project_context", "arguments": {"path": ".", "include_sections": true}}
```

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ENGRAM_DB_PATH` | Path to SQLite database | `~/.local/share/engram/memories.db` |
| `ENGRAM_STORAGE_MODE` | `local` or `cloud-safe` | `local` |
| `ENGRAM_CLOUD_URI` | S3 URI for cloud sync | - |
| `ENGRAM_CLOUD_ENCRYPT` | Enable encryption | `false` |
| `ENGRAM_LOG_LEVEL` | Log level (trace/debug/info/warn/error) | `info` |
| `AWS_PROFILE` | AWS profile for S3 credentials | - |
| `AWS_ENDPOINT_URL` | Custom S3 endpoint (for R2, MinIO) | - |

### Cloud Sync (Cloudflare R2 Example)

```bash
export AWS_PROFILE=engram
export AWS_ENDPOINT_URL=https://YOUR_ACCOUNT_ID.r2.cloudflarestorage.com
export ENGRAM_CLOUD_URI=s3://your-bucket/memories.db
export ENGRAM_CLOUD_ENCRYPT=true
```

## Memory Types

| Type | Description | Use Case |
|------|-------------|----------|
| `note` | General notes and observations | Default for unclassified content |
| `todo` | Action items and tasks | Tracking work to be done |
| `issue` | Problems and bugs | Bug tracking, issues to resolve |
| `decision` | Decisions and their rationale | Recording architectural choices |
| `preference` | User preferences and patterns | Remembering how user likes things |
| `learning` | Insights and learnings | TIL, discoveries, realizations |
| `context` | Contextual information | Background info for tasks |
| `credential` | Sensitive credentials | API keys, tokens (treat as secrets) |

## Edge Types (Cross-References)

| Type | Description |
|------|-------------|
| `related_to` | General relationship |
| `supersedes` | Newer version replaces older |
| `contradicts` | Conflicting information |
| `implements` | Implementation of a decision |
| `extends` | Builds upon existing memory |
| `references` | Cites or mentions |
| `depends_on` | Requires another memory |
| `blocks` | Prevents progress on |
| `follows_up` | Follow-up to previous |

## MCP Tools

When used as an MCP server, Engram provides these tools:

| Tool | Description |
|------|-------------|
| `memory_create` | Create a new memory |
| `memory_get` | Retrieve a memory by ID |
| `memory_update` | Update an existing memory |
| `memory_delete` | Delete a memory |
| `memory_list` | List memories with filters |
| `memory_search` | Hybrid search across memories |
| `memory_semantic_search` | Pure vector similarity search |
| `memory_search_suggest` | Search suggestions / typo correction |
| `memory_link` | Create cross-reference between memories |
| `memory_unlink` | Remove a cross-reference |
| `memory_related` | Find related memories |
| `memory_stats` | Get storage statistics |
| `memory_versions` | Version history for a memory |
| `memory_export_graph` | Export graph data |
| `memory_create_todo` | Convenience TODO creator |
| `memory_create_issue` | Convenience ISSUE creator |
| `memory_sync_status` | Sync status |
| `memory_scan_project` | Ingest instruction files as memories |
| `memory_get_project_context` | Fetch project context memories |

## Project Structure

```
engram/
├── src/
│   ├── lib.rs              # Library root
│   ├── types.rs            # Core type definitions
│   ├── error.rs            # Error types
│   ├── bin/
│   │   ├── server.rs       # MCP/HTTP server binary
│   │   └── cli.rs          # CLI binary
│   ├── storage/
│   │   ├── mod.rs          # Storage module
│   │   ├── connection.rs   # Connection pooling
│   │   ├── migrations.rs   # Schema migrations
│   │   ├── confidence.rs   # Confidence decay
│   │   └── temporal.rs     # Point-in-time queries
│   ├── search/
│   │   ├── mod.rs          # Search module
│   │   ├── bm25.rs         # BM25 full-text search
│   │   ├── hybrid.rs       # Hybrid search fusion
│   │   ├── fuzzy.rs        # Fuzzy/typo-tolerant search
│   │   └── aggregation.rs  # Aggregation queries
│   ├── embedding/
│   │   ├── mod.rs          # Embedding module
│   │   ├── tfidf.rs        # TF-IDF embeddings
│   │   └── queue.rs        # Async embedding queue
│   ├── sync/
│   │   ├── mod.rs          # Sync module
│   │   ├── cloud.rs        # S3 cloud storage
│   │   ├── worker.rs       # Background sync worker
│   │   └── conflict/       # Conflict resolution
│   ├── auth/
│   │   ├── mod.rs          # Auth module
│   │   ├── users.rs        # User management
│   │   ├── tokens.rs       # API key management
│   │   └── permissions.rs  # Permission system
│   ├── intelligence/
│   │   ├── mod.rs          # AI features module
│   │   ├── suggestions.rs  # Smart suggestions
│   │   ├── consolidation.rs# Auto-consolidation
│   │   ├── quality.rs      # Quality scoring
│   │   ├── natural_language.rs # NL commands
│   │   ├── auto_capture.rs # Auto-capture mode
│   │   └── project_context.rs # Project context ingestion
│   ├── graph/
│   │   └── mod.rs          # Knowledge graph visualization
│   ├── realtime/
│   │   └── mod.rs          # WebSocket support
│   └── mcp/
│       └── mod.rs          # MCP protocol handler
├── benches/                # Performance benchmarks
├── tests/                  # Integration tests
├── Cargo.toml
└── README.md
```

## Performance

Engram is designed for high performance:

- **SQLite WAL mode** for concurrent reads and crash recovery
- **Connection pooling** with read/write separation
- **Async embedding queue** with batch processing
- **Incremental cross-reference updates**
- **Confidence decay** using efficient half-life formula

### Benchmarks (10K memories, M1 Mac)

| Operation | Latency | Notes |
|-----------|---------|-------|
| Create | 125 µs | With FTS5 indexing |
| Get | 45 µs | Primary key lookup |
| Keyword search | 1.2 ms | BM25 |
| Semantic search | 2.5 ms | sqlite-vec |
| Hybrid search | 3.2 ms | BM25 + vector fusion |

Benchmarks are indicative; run `cargo bench` on your hardware and dataset size for accurate numbers.

## Status & Roadmap

**Status:** Actively maintained and evolving.

**Roadmap ideas:**
- File watching for project context (notify-based)
- Wildcard instruction file patterns
- Rich YAML section parsing
- Optional parent/child graph edges for context sections

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific module tests
cargo test storage::
cargo test search::
cargo test intelligence::
```

### Running Benchmarks

```bash
cargo bench
```

### Code Coverage

```bash
cargo tarpaulin --out Html
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Acknowledgments

- [sqlite-vec](https://github.com/asg017/sqlite-vec) - Vector search for SQLite
- [MCP Protocol](https://modelcontextprotocol.io) - Model Context Protocol specification
- [Axum](https://github.com/tokio-rs/axum) - Web framework for Rust

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request
