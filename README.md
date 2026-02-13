# Engram

**Memory for production AI agents — built for predictable latency.**  
Hybrid search, knowledge graphs, and optional cloud sync — shipped as a single Rust binary.

[![Crates.io](https://img.shields.io/crates/v/engram-core)](https://crates.io/crates/engram-core)
[![docs.rs](https://img.shields.io/docsrs/engram-core)](https://docs.rs/engram-core)
[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

---

## Choose Your Path

<table>
<tr>
<td width="50%" valign="top">

### Production LLM Apps (Primary)

A persistent memory layer designed for real deployments: fast, stable, and easy to ship.

```bash
# Store a memory
curl -X POST localhost:8080/v1/memories \
  -d '{"content": "User prefers dark mode"}'

# Hybrid search
curl localhost:8080/v1/search?q=user+preferences
```

**What you get:**
- Hybrid search (BM25 + vectors + fuzzy) in one call
- MCP / REST / WebSocket / CLI
- Predictable p95 latency (no runtime, no reindex loops)

</td>
<td width="50%" valign="top">

### Dev Workflow (Bonus)

Capture project context and decision trails so your coding agents stop repeating the same questions.

```bash
# Search decisions
engram-cli search "why did we choose postgres"
```

**What you get:**
- Project Context Discovery (CLAUDE.md, .cursorrules, etc.) via MCP tools
- Decision trails with tags + metadata
- Local-first by default, sync optional

</td>
</tr>
</table>

---

## Quick Start

```bash
# Install from crates.io
cargo install engram-core

# Or from source
git clone https://github.com/limaronaldo/engram.git
cd engram && cargo install --path .

# Run as MCP server (Claude Code, Cursor, VS Code MCP clients, etc.)
engram-server --mcp

# Or run as HTTP API
engram-server --http --port 8080
```

---

## Why Engram

Agents forget between sessions. Context windows overflow. Important knowledge gets buried in chat logs.

Engram turns that into a fast, queryable memory system with stable latency and zero runtime dependencies.

| Problem | Engram Solution |
|---------|-----------------|
| Vector search misses exact keywords | **Hybrid search**: BM25 + vectors + fuzzy, fused + ranked |
| Context disappears between sessions | **Persistent memory** on SQLite + WAL |
| Cloud-only products | **Local-first**, optional S3/R2 sync |
| Python/Docker required | **Single Rust binary** (no runtime stack) |
| No project awareness | **Project Context Discovery** (CLAUDE.md, AGENTS.md, .cursorrules, etc.) |

---

## How It Compares

| Feature | Engram | Mem0 | Zep | Letta |
|---------|--------|------|-----|-------|
| Language | Rust | Python | Python | Python |
| MCP Native | Yes | Plugin | No | No |
| Single Binary | Yes | No | No | No |
| Local-first | Yes | Optional | Cloud-first | Optional |
| Hybrid Search | BM25+Vec+Fuzzy | Vec+KV | Vec+Graph | Vec |
| Project Context | Yes | No | No | No |
| Edge-Native Latency | Yes | No | No | No |

> "Edge-native" here means runs beside the agent, with predictable p95 latency and no dependency chain.

---

## Core Features

### Hybrid Search

```bash
# Handles typos, semantic matches, and exact keywords in one query
engram-cli search "asynch awiat rust"
# → Returns: "Use async/await for I/O-bound work in Rust"
```

### Multi-Workspace Support

Isolate memories by project or context:

```bash
# Create memory in a specific workspace
engram-cli create "API keys stored in Vault" --workspace my-project

# List workspaces
engram-cli workspace list
```

### Memory Tiering

Two tiers for different retention needs:

- **Permanent**: Important knowledge, decisions (never expires)
- **Daily**: Session context, scratch notes (auto-expire after 24h)

```bash
# Create a daily memory (expires in 24h)
engram-cli create "Current debugging task" --tier daily
```

### Session Transcript Indexing

Store and search conversation transcripts:

```bash
# Index a conversation session
engram-cli session index --session-id chat-123 --messages messages.json

# Search within transcripts
engram-cli session search "error handling"
```

### Identity Links (Entity Unification)

Link different mentions to canonical identities:

```bash
# Create identity with aliases
engram-cli identity create user:ronaldo --alias "Ronaldo" --alias "@ronaldo"
```

### Knowledge Graph

```bash
# Export the graph
engram-cli graph --format json --output graph.json
```

Entity extraction (`memory_extract_entities`) links memories through shared entities.  
Multi-hop traversal and shortest-path are available via MCP tools:
- `memory_traverse`
- `memory_find_path`

### Multiple Interfaces

- **MCP**: Native Model Context Protocol for Claude Code, Cursor, VS Code MCP clients
- **REST**: Standard HTTP API for any client
- **WebSocket**: Real-time updates
- **CLI**: Developer-friendly commands

### Salience Scoring

Dynamic memory prioritization based on recency, frequency, importance, and feedback:

```bash
# Get top memories by salience
engram-cli salience top --limit 10

# Boost a memory's salience
engram-cli salience boost 42
```

Salience decays over time, transitioning memories through lifecycle states: Active -> Stale -> Archived.

### Context Quality

5-component quality assessment (clarity, completeness, freshness, consistency, source trust):

```bash
# Quality report for a workspace
engram-cli quality report --workspace my-project

# Find near-duplicate memories
engram-cli quality duplicates
```

Includes conflict detection for contradictions between memories and resolution workflows.

### Optional Meilisearch Backend

Offload search to Meilisearch for larger-scale deployments (feature-gated):

```bash
# Build with Meilisearch support
cargo build --features meilisearch

# Run with Meilisearch indexer
engram-server --meilisearch-url http://localhost:7700 --meilisearch-indexer
```

SQLite remains the source of truth. MeilisearchIndexer syncs changes in the background.

### Project Context Discovery

Ingest and query instruction and policy files using MCP tools:
- `memory_scan_project`
- `memory_get_project_context`

**Supported patterns:**
- CLAUDE.md
- AGENTS.md
- .cursorrules
- .github/copilot-instructions.md
- .aider.conf.yml
- CONVENTIONS.md, CODING_GUIDELINES.md, etc.

---

## MCP Configuration

Add to your MCP config (for example: `~/.claude/mcp.json`, `.cursor/mcp.json`, or your VS Code MCP extension config):

```json
{
  "mcpServers": {
    "engram": {
      "command": "engram-server",
      "args": [],
      "env": {
        "ENGRAM_DB_PATH": "~/.local/share/engram/memories.db"
      }
    }
  }
}
```

If you built from source instead of installing via Homebrew, use the full path to the binary (e.g. `/path/to/engram/target/release/engram-server`).

### Available MCP Tools

**Core Memory Operations:**
| Tool | Description |
|------|-------------|
| `memory_create` | Store a new memory |
| `memory_create_daily` | Create auto-expiring daily memory |
| `memory_get` | Retrieve by ID |
| `memory_update` | Update content or metadata |
| `memory_delete` | Remove a memory |
| `memory_list` | List with filters |
| `memory_search` | Hybrid search with typo tolerance |
| `memory_related` | Find cross-references |
| `memory_stats` | Usage statistics |

**Workspace Management:**
| Tool | Description |
|------|-------------|
| `workspace_list` | List all workspaces |
| `workspace_stats` | Get workspace statistics |
| `workspace_move` | Move memory to workspace |
| `workspace_delete` | Delete workspace (with migrate option) |

**Session Indexing:**
| Tool | Description |
|------|-------------|
| `session_index` | Index conversation transcript |
| `session_index_delta` | Incremental transcript update |
| `session_get` | Get session info |
| `session_list` | List sessions |
| `session_search` | Search within transcripts |
| `session_delete` | Delete session |

**Identity Links:**
| Tool | Description |
|------|-------------|
| `identity_create` | Create canonical identity |
| `identity_get` | Get identity details |
| `identity_add_alias` | Add alias to identity |
| `identity_link` | Link memory to identity |
| `identity_unlink` | Unlink memory from identity |
| `identity_resolve` | Resolve alias to canonical ID |

**Knowledge Graph:**
| Tool | Description |
|------|-------------|
| `memory_extract_entities` | Extract named entities from a memory |
| `memory_get_entities` | List entities for a memory |
| `memory_search_entities` | Search entities by name |
| `memory_entity_stats` | Entity statistics |
| `memory_traverse` | Multi-hop graph traversal |
| `memory_find_path` | Shortest path between memories |

**Project Context:**
| Tool | Description |
|------|-------------|
| `memory_scan_project` | Ingest project context files |
| `memory_get_project_context` | Retrieve project context memories |

**Salience:**
| Tool | Description |
|------|-------------|
| `salience_get` | Get salience score with component breakdown |
| `salience_boost` | Boost memory salience |
| `salience_top` | Get top memories by salience |
| `salience_decay_run` | Run temporal decay cycle |

**Quality:**
| Tool | Description |
|------|-------------|
| `quality_score` | Get quality breakdown |
| `quality_find_duplicates` | Find near-duplicate memories |
| `quality_find_conflicts` | Detect contradictions |
| `quality_resolve_conflict` | Resolve conflicts |
| `quality_report` | Workspace quality report |

**Lifecycle:**
| Tool | Description |
|------|-------------|
| `lifecycle_status` | Active/stale/archived counts |
| `lifecycle_run` | Trigger lifecycle cycle |
| `memory_set_lifecycle` | Manually set lifecycle state |

**Compression:**
| Tool | Description |
|------|-------------|
| `memory_summarize` | Create summary from multiple memories |
| `context_budget_check` | Check token usage against budget |
| `memory_archive_old` | Batch archive old memories |

**Meilisearch** (requires `--features meilisearch`):
| Tool | Description |
|------|-------------|
| `meilisearch_search` | Search via Meilisearch directly |
| `meilisearch_reindex` | Trigger full re-sync from SQLite |
| `meilisearch_status` | Index stats and health |
| `meilisearch_config` | Current configuration |

**Performance:**
| Tool | Description |
|------|-------------|
| `embedding_cache_stats` | Cache hit/miss statistics |
| `embedding_cache_clear` | Clear embedding cache |

**140+ MCP tools total.** See [CHANGELOG.md](CHANGELOG.md) for the full list.

---

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `ENGRAM_DB_PATH` | SQLite database path | `~/.local/share/engram/memories.db` |
| `ENGRAM_STORAGE_URI` | S3/R2 URI for cloud sync | - |
| `ENGRAM_CLOUD_ENCRYPT` | AES-256-GCM encryption | `false` |
| `ENGRAM_EMBEDDING_MODEL` | Embedding model (`tfidf`, `openai`) | `tfidf` |
| `ENGRAM_CLEANUP_INTERVAL` | Expired memory cleanup interval (seconds) | `3600` |
| `ENGRAM_WS_PORT` | WebSocket server port (0 = disabled) | `0` |
| `OPENAI_API_KEY` | OpenAI API key (for `openai` embeddings) | - |
| `MEILISEARCH_URL` | Meilisearch URL (requires `--features meilisearch`) | - |
| `MEILISEARCH_API_KEY` | Meilisearch API key | - |
| `MEILISEARCH_INDEXER` | Enable background sync to Meilisearch | `false` |
| `MEILISEARCH_SYNC_INTERVAL` | Sync interval in seconds | `60` |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Engram Server                           │
├─────────────────────────────────────────────────────────────────┤
│  MCP (stdio)  │  REST (HTTP)  │  WebSocket  │  CLI              │
├─────────────────────────────────────────────────────────────────┤
│                    Intelligence Layer                           │
│  • Salience scoring  • Quality assessment  • Entity extraction  │
│  • Context compression  • Lifecycle management                  │
├─────────────────────────────────────────────────────────────────┤
│                      Search Layer                               │
│  • BM25 (FTS5)  • Vectors (sqlite-vec)  • Fuzzy  • RRF fusion  │
│  • Optional Meilisearch backend for scaled deployments          │
├─────────────────────────────────────────────────────────────────┤
│                     Storage Layer                               │
│  • SQLite + WAL  • Turso/libSQL  • Connection pooling           │
│  • Optional S3/R2 sync with AES-256 encryption                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Contributing

Contributions welcome! See [CLAUDE.md](CLAUDE.md) for conventions.

```bash
cargo test           # Run all tests
cargo clippy         # Lint
cargo fmt            # Format
```

---

## License

MIT License — see [LICENSE](LICENSE) for details.
