# Engram

**Persistent memory for AI agents.** Hybrid search, knowledge graphs, cloud sync — in a single Rust binary.
Start free, scale later.

[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

## Get Started

**Self-host (30 seconds):**

```bash
# Clone + install
git clone https://github.com/limaronaldo/engram.git
cd engram
cargo install --path .

# Run as MCP server
engram-server --mcp
```

**Cloud (early access):**
If you want a hosted MCP endpoint, open an issue with the tag `cloud` and I’ll add you to the list.

---

## Why Engram

AI agents forget everything between sessions. Context windows overflow. Knowledge scatters across chat logs. **Engram fixes this.**

- **Hybrid search**: BM25 + semantic vectors + fuzzy correction
- **Knowledge graph**: cross-references, confidence decay, consolidation
- **Multiple interfaces**: MCP, REST, WebSocket, CLI
- **Cloud sync**: S3-compatible with optional AES-256-GCM encryption
- **Intelligence layer**: auto-capture, quality scoring, NL commands

## Editions

| Edition | Who it’s for | What you get |
|---------|--------------|--------------|
| **Community (MIT)** | Developers, self-hosters | Single-tenant engine, MCP/REST/CLI, hybrid search, project context |
| **Cloud (Hosted)** | Teams, no-ops users | Managed MCP/API, team workspaces, backups, usage limits |
| **Enterprise (Self-hosted)** | Regulated orgs | SSO/SAML, audit trails, governance, SLA |

**Plan ladder:** Community → Cloud → Enterprise.

## CLI Demo (90 seconds)

```bash
# Store a memory
engram-cli create "Use async/await for I/O-bound work" --type learning --tags rust,async

# Find it with fuzzy search
engram-cli search "asynch awiat"

# Show recent memories
engram-cli list --limit 5
```

## MCP Quick Start

Add to your MCP config (e.g., `~/.config/claude/mcp.json`):

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

## Cloud Edition (Planned)

The hosted edition will expose a managed MCP/API endpoint with an API key.

```bash
export ENGRAM_SERVER_URL="https://your-engram-cloud-endpoint"
export ENGRAM_API_KEY="..."
```

Want early access? Open an issue with the tag `cloud`.

## Project Context Discovery

Engram can ingest AI instruction files and convert them into searchable memories.

Supported core files (Phase 1):

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

## Configuration (Essentials)

| Variable | Description | Default |
|----------|-------------|---------|
| `ENGRAM_DB_PATH` | Path to SQLite DB | `~/.local/share/engram/memories.db` |
| `ENGRAM_STORAGE_MODE` | `local` or `cloud-safe` | `local` |
| `ENGRAM_CLOUD_URI` | S3 URI for cloud sync | - |
| `ENGRAM_CLOUD_ENCRYPT` | Enable encryption | `false` |

## Status & Roadmap

**Status:** Actively maintained.

**Roadmap ideas:** file watching for project context, wildcard patterns, richer YAML sections, optional parent/child graph edges.

## License

MIT License - see [LICENSE](LICENSE) for details.
