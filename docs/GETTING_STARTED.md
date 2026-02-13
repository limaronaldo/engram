# Getting Started with Engram

A quick guide to installing Engram, connecting it to your AI tools, and storing your first memories.

---

## Installation

### From Source (Recommended)

```bash
git clone https://github.com/limaronaldo/engram.git
cd engram/engram
cargo install --path .
```

This installs two binaries: `engram-server` and `engram-cli`.

### Pre-built Binaries

Download from [GitHub Releases](https://github.com/limaronaldo/engram/releases):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/limaronaldo/engram/releases/latest/download/engram-server-macos-arm64 -o engram-server
curl -L https://github.com/limaronaldo/engram/releases/latest/download/engram-cli-macos-arm64 -o engram-cli
chmod +x engram-server engram-cli
sudo mv engram-server engram-cli /usr/local/bin/

# macOS (Intel)
curl -L https://github.com/limaronaldo/engram/releases/latest/download/engram-server-macos-x86_64 -o engram-server
curl -L https://github.com/limaronaldo/engram/releases/latest/download/engram-cli-macos-x86_64 -o engram-cli
chmod +x engram-server engram-cli
sudo mv engram-server engram-cli /usr/local/bin/

# Linux (x86_64)
curl -L https://github.com/limaronaldo/engram/releases/latest/download/engram-server-linux-x86_64 -o engram-server
curl -L https://github.com/limaronaldo/engram/releases/latest/download/engram-cli-linux-x86_64 -o engram-cli
chmod +x engram-server engram-cli
sudo mv engram-server engram-cli /usr/local/bin/
```

### Homebrew (macOS)

```bash
brew install limaronaldo/engram/engram
```

### Docker

```bash
docker run -v engram-data:/data ghcr.io/limaronaldo/engram-server:latest
```

---

## Configure MCP for AI Tools

Engram speaks the [Model Context Protocol](https://modelcontextprotocol.io/) (MCP), so it integrates with Claude Code, Cursor, VS Code MCP clients (like Cline/Roo Code), and other MCP-compatible tools.

### Claude Code (Example)

Add to `~/.claude/mcp.json`:

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

### Cursor

Add to `.cursor/mcp.json` in your project root:

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

### Other MCP Clients

Use the same `mcpServers.engram` JSON block in your client's MCP config location.

### Verify Connection

Once configured, your AI tool will have access to 136+ MCP tools. Ask it to run `memory_stats` to verify the connection is working.

---

## Create Your First Memory

### Using the CLI

```bash
# Create a simple note
engram-cli create "The API uses JWT tokens for authentication" --type note

# Create with tags
engram-cli create "Deploy to staging before production" --type decision --tags "deploy,process"

# Create in a specific workspace
engram-cli create "User auth flow uses OAuth2" --workspace my-project
```

### Using MCP (Any MCP Client)

In Claude Code, Cursor, VS Code MCP clients, or any MCP-enabled assistant, you can use prompts like:

> "Remember that our API uses JWT tokens for authentication"

> "Store this as a decision memory: deploy to staging before production"

> "Search my memories for authentication notes"

The AI will call `memory_create` automatically.

### Using the HTTP API

```bash
# Start the HTTP server
engram-server --http --port 8080

# Create a memory
curl -X POST http://localhost:8080/v1/memories \
  -H "Content-Type: application/json" \
  -d '{
    "content": "The API uses JWT tokens for authentication",
    "memory_type": "note",
    "tags": ["auth", "api"]
  }'
```

---

## Search Your Memories

Engram uses hybrid search combining BM25 keyword matching, vector similarity, and fuzzy matching in a single query.

### CLI Search

```bash
# Basic search
engram-cli search "authentication"

# Search handles typos
engram-cli search "authentcation"

# Search in a specific workspace
engram-cli search "deploy" --workspace my-project
```

### MCP Search

Ask your AI assistant:

> "Search my memories for anything about authentication"

### HTTP Search

```bash
curl "http://localhost:8080/v1/search?q=authentication&limit=10"
```

---

## Organize with Workspaces

Workspaces isolate memories by project or context:

```bash
# Create memories in different workspaces
engram-cli create "Use PostgreSQL for this project" --workspace backend-api
engram-cli create "React with TypeScript" --workspace frontend-app

# List all workspaces
engram-cli workspace list

# Search within a workspace
engram-cli search "database" --workspace backend-api
```

---

## Memory Tiers

Use tiers to control memory lifetime:

- **permanent** (default): Important knowledge that persists forever
- **daily**: Scratch notes that auto-expire after 24 hours

```bash
# Permanent memory (default)
engram-cli create "Architecture: microservices with event sourcing"

# Daily memory (auto-expires)
engram-cli create "Currently debugging the auth flow" --tier daily

# Promote a daily memory to permanent before it expires
engram-cli promote 42
```

---

## Cloud Sync (Optional)

Sync your memories to S3-compatible storage (AWS S3, Cloudflare R2, MinIO):

```bash
# Configure cloud sync
export ENGRAM_STORAGE_URI=s3://my-bucket/engram/memories.db
export ENGRAM_CLOUD_ENCRYPT=true  # AES-256-GCM encryption
export AWS_PROFILE=my-profile

# Start with cloud sync
engram-server
```

This enables cross-machine synchronization with encrypted storage.

---

## Key Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ENGRAM_DB_PATH` | SQLite database path | `~/.local/share/engram/memories.db` |
| `ENGRAM_STORAGE_URI` | S3 URI for cloud sync | (local only) |
| `ENGRAM_CLOUD_ENCRYPT` | Enable AES-256 encryption | `false` |
| `ENGRAM_EMBEDDING_MODEL` | `tfidf` (default) or `openai` | `tfidf` |
| `OPENAI_API_KEY` | Required for OpenAI embeddings | - |

---

## Next Steps

- Read the full [README](../README.md) for feature details
- See [AGENTS.md](../AGENTS.md) for the complete MCP tool reference
- Check [SCHEMA.md](SCHEMA.md) for database schema details
- Explore the [architecture overview](../README.md#architecture) to understand how components fit together
