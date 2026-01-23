# Engram

**Persistent memory for AI agents.** Hybrid search, knowledge graphs, cloud sync — in a single Rust binary.

[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Get Started

**Self-host (30 seconds):**
```bash
cargo install engram
engram-server
```

**Engram Cloud** *(coming soon)*: Hosted MCP endpoint, no infrastructure required.
[→ Request early access](https://github.com/limaronaldo/engram/issues/new?title=Cloud%20Early%20Access&body=I%27d%20like%20early%20access%20to%20Engram%20Cloud)

## Why Engram

AI agents forget everything between sessions. Context windows overflow. Knowledge scatters across chat logs. **Engram fixes this.**

- **Hybrid search that just works**: BM25 + semantic vectors + fuzzy correction in one call
- **Knowledge graph built in**: cross-references, confidence decay, automatic consolidation
- **Multiple interfaces**: MCP, REST, WebSocket, CLI — pick what fits your stack
- **Cloud sync**: S3-compatible with optional AES-256-GCM encryption
- **Intelligence layer**: auto-capture patterns, quality scoring, natural language queries

## Editions

| Edition | Who it's for | What you get |
|---------|--------------|--------------|
| **Community** | Developers, self-hosters | Full engine, MCP/REST/CLI, hybrid search, BYO storage |
| **Cloud** | Teams, no-ops users | Hosted API, team workspaces, cross-device sync, backups |
| **Enterprise** | Regulated orgs | SSO/SAML, audit logs, governance policies, SLA |

Community is MIT-licensed and **available now**. Cloud and Enterprise are planned.

## 30-Second Demo

```bash
# Create a memory
engram-cli create "Always use async/await for I/O-bound work in Rust" \
  --type learning --tags rust,async

# Search (handles typos)
engram-cli search "asynch awiat"
# → Returns the memory with fuzzy matching

# List recent memories
engram-cli list --limit 10
```

## Built For

- Long-running agents that need reliable memory across sessions
- Team knowledge bases and architectural decision logs
- Project context ingestion from instruction files (CLAUDE.md, AGENTS.md, etc.)
- LLM tools that need fast semantic + keyword retrieval

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
git clone https://github.com/limaronaldo/engram.git
cd engram
cargo build --release
cargo install --path .
```

### Requirements

- Rust 1.75+
- SQLite 3.35+ (bundled)

## Quick Start

### MCP Server

Add to your MCP configuration:

```json
{
  "mcpServers": {
    "memory": {
      "command": "engram-server",
      "env": {
        "ENGRAM_DB_PATH": "~/.local/share/engram/memories.db"
      }
    }
  }
}
```

### HTTP Server

```bash
engram-server --http --port 8080

# Create a memory
curl -X POST http://localhost:8080/memories \
  -H "Content-Type: application/json" \
  -d '{"content": "Rust ownership prevents data races", "type": "learning"}'

# Search
curl "http://localhost:8080/search?q=rust%20ownership"
```

## Cloud Edition

Want managed hosting without self-hosting? Cloud includes:

- Hosted MCP/API endpoint with API key auth
- Team workspaces and shared memory
- Cross-device sync, backups, monitoring
- Usage dashboards and rate limits

```bash
# Coming soon
export ENGRAM_API_URL="https://api.engram.dev"
export ENGRAM_API_KEY="ek_..."
engram-cli --cloud search "project context"
```

[→ Request early access](https://github.com/limaronaldo/engram/issues/new?title=Cloud%20Early%20Access&body=I%27d%20like%20early%20access%20to%20Engram%20Cloud)

## Project Context Discovery

Ingest AI instruction files as searchable memories:

| File | Platform |
|------|----------|
| `CLAUDE.md` | Claude Code |
| `AGENTS.md` | Various agents |
| `.cursorrules` | Cursor IDE |
| `.github/copilot-instructions.md` | GitHub Copilot |
| `GEMINI.md` | Gemini |
| `.aider.conf.yml` | Aider |
| `CONVENTIONS.md` | General |
| `.windsurfrules` | Windsurf IDE |

```bash
# Scan and ingest
engram-cli scan-project .

# Or via MCP
{"tool": "memory_scan_project", "arguments": {"path": ".", "extract_sections": true}}
```

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `ENGRAM_DB_PATH` | SQLite database path | `~/.local/share/engram/memories.db` |
| `ENGRAM_STORAGE_MODE` | `local` or `cloud-safe` | `local` |
| `ENGRAM_CLOUD_URI` | S3 URI for sync | - |
| `ENGRAM_CLOUD_ENCRYPT` | Enable AES-256 encryption | `false` |

### BYO Cloud Storage (S3/R2)

```bash
export AWS_PROFILE=engram
export AWS_ENDPOINT_URL=https://ACCOUNT.r2.cloudflarestorage.com
export ENGRAM_CLOUD_URI=s3://bucket/memories.db
export ENGRAM_CLOUD_ENCRYPT=true
```

## Memory Types

| Type | Use Case |
|------|----------|
| `note` | General observations |
| `todo` | Action items |
| `issue` | Bugs, problems |
| `decision` | Architectural choices |
| `preference` | User preferences |
| `learning` | TIL, insights |
| `context` | Background info |
| `credential` | Secrets (handle carefully) |

## MCP Tools

| Tool | Description |
|------|-------------|
| `memory_create` | Create a memory |
| `memory_search` | Hybrid search |
| `memory_list` | List with filters |
| `memory_link` | Cross-reference memories |
| `memory_scan_project` | Ingest instruction files |
| `memory_get_project_context` | Get project memories |

Full list: `memory_get`, `memory_update`, `memory_delete`, `memory_unlink`, `memory_related`, `memory_stats`, `memory_versions`, `memory_export_graph`, `memory_create_todo`, `memory_create_issue`, `memory_sync_status`

## Performance

| Operation | Latency | Notes |
|-----------|---------|-------|
| Create | 125 µs | With FTS5 indexing |
| Get | 45 µs | Primary key lookup |
| Keyword search | 1.2 ms | BM25 |
| Semantic search | 2.5 ms | sqlite-vec |
| Hybrid search | 3.2 ms | RRF fusion |

*Benchmarks on 10K memories, M1 Mac. Run `cargo bench` for your setup.*

## Development

```bash
cargo test                 # Run all tests
cargo test storage::       # Run module tests
cargo bench                # Run benchmarks
cargo fmt && cargo clippy  # Lint
```

## License

MIT License - see [LICENSE](LICENSE)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). PRs welcome!
