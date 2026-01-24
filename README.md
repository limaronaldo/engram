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
# Scan project context
engram-cli scan .

# Search decisions
engram-cli search "why did we choose postgres"
```

**Key features:**
- Project Context Discovery (CLAUDE.md, .cursorrules, etc.)
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

**Cloud (early access):** [Request access](https://github.com/limaronaldo/engram/issues/new?title=Cloud%20Early%20Access&labels=cloud)

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
# Cross-references with confidence decay
engram-cli related 42 --depth 2
```

### Multiple Interfaces
- **MCP**: Native Model Context Protocol for Claude Code, Cursor
- **REST**: Standard HTTP API for any client
- **WebSocket**: Real-time updates
- **CLI**: Developer-friendly commands

### Project Context Discovery
```bash
# Ingest AI instruction files into searchable memory
engram-cli scan . --extract-sections

# Supported: CLAUDE.md, AGENTS.md, .cursorrules, 
# .github/copilot-instructions.md, .aider.conf.yml, etc.
```

---

## Editions

| | Community | Cloud | Enterprise |
|--|-----------|-------|------------|
| **Price** | Free | $29/mo | Contact us |
| **Deploy** | Self-host | Managed | Self-host |
| **Tenancy** | Single | Multi | Multi |
| **Search** | Hybrid | Hybrid | Hybrid |
| **Project Context** | Yes | Yes | Yes |
| **Team Workspaces** | - | Yes | Yes |
| **SSO/SAML** | - | - | Yes |
| **Audit Logs** | - | - | Yes |
| **SLA** | - | 99.9% | Custom |

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

## Roadmap

See [docs/CLOUD_ARCHITECTURE.md](docs/CLOUD_ARCHITECTURE.md) for the full Cloud roadmap.

**Current focus:**
- M1: Gateway & Auth (API gateway, tenant isolation, MCP-over-HTTP)
- Core: Project Context Discovery (file parsing, MCP tools, search boost)

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
