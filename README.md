# Engram

**The memory layer for production LLM apps.** Hybrid search, knowledge graphs, cloud sync — in a single Rust binary.

[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

---

## Choose Your Path

<table>
<tr>
<td width="50%" valign="top">

### For LLM Apps (Primary)

Persistent memory that works in production.

```bash
# Store a memory
curl -X POST localhost:8080/v1/memories \
  -d '{"content": "User prefers dark mode"}'

# Hybrid search
curl localhost:8080/v1/search?q=user+preferences
```

**Key features:**
- Hybrid search (BM25 + vectors + fuzzy)
- MCP / REST / WebSocket / CLI
- Predictable latency, no reindexing

</td>
<td width="50%" valign="top">

### Bonus: Dev Workflow

Helps engineers building LLM apps by capturing project context and decisions.

```bash
# Search decisions
engram-cli search "why did we choose postgres"
```

**Key features:**
- Project Context Discovery (CLAUDE.md, .cursorrules, etc.) via MCP tools
- Decision trails with metadata + tags
- Local-first, sync optional

</td>
</tr>
</table>

---

## Quick Start

```bash
# Install
git clone https://github.com/limaronaldo/engram.git
cd engram && cargo install --path .

# Run as MCP server (for Claude Code, Cursor, etc.)
engram-server --mcp

# Or run as HTTP API
engram-server --http --port 8080
```

---

## Why Engram

AI agents forget everything between sessions. Context windows overflow. Knowledge scatters across chat logs.

| Problem | Engram Solution |
|---------|-----------------|
| Vector search misses keywords | **Hybrid search**: BM25 + vectors + fuzzy in one call |
| Context lost between sessions | **Persistent memory** with SQLite + WAL |
| Cloud-only options | **Local-first** with optional S3/R2 sync |
| Python runtime required | **Single Rust binary**, no dependencies |
| No project awareness | **Project Context Discovery** (CLAUDE.md, AGENTS.md, etc.) |

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
| Edge Deploy | Yes (SQLite) | No | No | No |

---

## Core Features

### Hybrid Search
```bash
# Handles typos, finds semantic matches, ranks by relevance
engram-cli search "asynch awiat rust"
# → Returns "Use async/await for I/O-bound work in Rust"
```

### Knowledge Graph
```bash
# Export the graph (CLI)
engram-cli graph --format json --output graph.json
```
Entity extraction (`memory_extract_entities`) links memories via shared entities. Multi-hop traversal and shortest-path are available via MCP tools `memory_traverse` and `memory_find_path`.

### Multiple Interfaces
- **MCP**: Native Model Context Protocol for Claude Code, Cursor
- **REST**: Standard HTTP API for any client
- **WebSocket**: Real-time updates
- **CLI**: Developer-friendly commands

### Project Context Discovery
Use MCP tools `memory_scan_project` and `memory_get_project_context` to ingest and query instruction files.
Supported: CLAUDE.md, AGENTS.md, .cursorrules, .github/copilot-instructions.md, .aider.conf.yml, etc.

---

## MCP Configuration

Add to your MCP config (`~/.config/claude/mcp.json` or similar):

```json
{
  "mcpServers": {
    "engram": {
      "command": "engram-server",
      "args": ["--mcp"],
      "env": {
        "ENGRAM_DB_PATH": "~/.local/share/engram/memories.db"
      }
    }
  }
}
```

### Available MCP Tools

| Tool | Description |
|------|-------------|
| `memory_create` | Store a new memory |
| `memory_search` | Hybrid search with typo tolerance |
| `memory_get` | Retrieve by ID |
| `memory_update` | Update content or metadata |
| `memory_delete` | Remove a memory |
| `memory_list` | List with filters |
| `memory_related` | Find cross-references |
| `memory_scan_project` | Ingest project context files |
| `memory_get_project_context` | Retrieve project context memories |
| `memory_extract_entities` | Extract named entities from a memory |
| `memory_get_entities` | List entities for a memory |
| `memory_search_entities` | Search entities by name |
| `memory_entity_stats` | Entity statistics |
| `memory_traverse` | Multi-hop graph traversal |
| `memory_find_path` | Shortest path between memories |
| `memory_stats` | Usage statistics |

---

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `ENGRAM_DB_PATH` | SQLite database path | `~/.local/share/engram/memories.db` |
| `ENGRAM_STORAGE_MODE` | `local` or `cloud-safe` | `local` |
| `ENGRAM_CLOUD_URI` | S3/R2 URI for sync | - |
| `ENGRAM_CLOUD_ENCRYPT` | AES-256-GCM encryption | `false` |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Engram Server                           │
├─────────────────────────────────────────────────────────────────┤
│  MCP (stdio)  │  REST (HTTP)  │  WebSocket  │  CLI              │
├─────────────────────────────────────────────────────────────────┤
│                    Intelligence Layer                           │
│  • Auto-capture  • Suggestions  • Quality scoring  • NL commands│
├─────────────────────────────────────────────────────────────────┤
│                      Search Layer                               │
│  • BM25 (FTS5)  • Vectors (sqlite-vec)  • Fuzzy  • RRF fusion  │
├─────────────────────────────────────────────────────────────────┤
│                     Storage Layer                               │
│  • SQLite + WAL  • Connection pooling  • Optional S3/R2 sync   │
└─────────────────────────────────────────────────────────────────┘
```

---

## Contributing

Contributions welcome! Please read the codebase conventions in [CLAUDE.md](CLAUDE.md).

```bash
cargo test           # Run all tests
cargo clippy         # Lint
cargo fmt            # Format
```

---

## License

MIT License - see [LICENSE](LICENSE) for details.
