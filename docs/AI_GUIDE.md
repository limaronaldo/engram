# Engram — Complete AI Agent Guide

> **Version:** 0.20.0 | **Protocol:** MCP 2025-11-25 | **Tools:** 220+ | **Schema:** v34

This guide teaches AI agents how to use every Engram capability — from basic CRUD to cloud sync, multimodal memory, knowledge packaging, and multi-agent coordination.

---

## Table of Contents

1. [Connecting to Engram](#1-connecting-to-engram)
2. [Core Concepts](#2-core-concepts)
3. [Memory CRUD](#3-memory-crud)
4. [Search](#4-search)
5. [Cognitive Memory Types](#5-cognitive-memory-types)
6. [Knowledge Graph](#6-knowledge-graph)
7. [Identity & Cross-Reference](#7-identity--cross-reference)
8. [Session Management](#8-session-management)
9. [Workspace Organization](#9-workspace-organization)
10. [Cloud Sync](#10-cloud-sync)
11. [Multimodal Memory](#11-multimodal-memory)
12. [Snapshots & Portability](#12-snapshots--portability)
13. [Attestation Chain](#13-attestation-chain)
14. [Retention Policies](#14-retention-policies)
15. [Project Context Scanning](#15-project-context-scanning)
16. [Entity Extraction](#16-entity-extraction)
17. [Semantic Deduplication](#17-semantic-deduplication)
18. [Advanced Filtering](#18-advanced-filtering)
19. [Multi-Agent Sync (Cloud)](#19-multi-agent-sync-cloud)
20. [Transport Options](#20-transport-options)
21. [Watcher Daemon](#21-watcher-daemon)
22. [Recipes & Patterns](#22-recipes--patterns)
23. [Tool Reference](#23-tool-reference)
24. [Progressive Tool Discovery](#24-progressive-tool-discovery)
25. [Session Handoff Protocol](#25-session-handoff-protocol)
26. [Markdown Export](#26-markdown-export)
27. [Recent Activity](#27-recent-activity)

---

## 1. Connecting to Engram

### Via MCP (stdio — default)

Add to your MCP client configuration:

```json
{
  "mcpServers": {
    "engram": {
      "command": "engram-server",
      "args": []
    }
  }
}
```

### Via HTTP Transport

```bash
engram-server --transport http --http-port 3000 --http-api-key sk_my_secret
```

Call tools via JSON-RPC 2.0 at `POST /v1/mcp`:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "memory_create",
    "arguments": {
      "content": "The user prefers dark mode",
      "type": "preference",
      "workspace": "settings"
    }
  }
}
```

### Via gRPC Transport

```bash
engram-server --transport grpc --grpc-port 50051 --grpc-api-key my-secret
```

Uses `proto/mcp.proto` — params and results are JSON strings inside protobuf messages.

### Via Engram Cloud (multi-tenant SaaS)

```bash
POST https://engram-cloud-gateway.fly.dev/v1/mcp
Authorization: Bearer eg_live_YOUR_API_KEY
X-Tenant: your-tenant-slug
Content-Type: application/json
```

Batch endpoint: `POST /v1/mcp/batch` (up to 100 requests per call).

---

## 2. Core Concepts

### Memory Types

| Type | Purpose | Example |
|------|---------|---------|
| `note` | General information | "User's favorite color is blue" |
| `todo` | Action items | "Refactor the auth module" |
| `issue` | Bugs and problems | "Login fails on Safari" |
| `decision` | Architectural decisions | "We chose PostgreSQL over MySQL" |
| `preference` | User preferences | "Prefers TypeScript over JavaScript" |
| `learning` | Learned patterns | "This API requires pagination" |
| `context` | Session/project context | "Working on the payment module" |
| `credential` | Sensitive data | "API key stored in Vault" |
| `episodic` | Events with timestamps | "Deployed v2.0 at 14:30 UTC" |
| `procedural` | How-to patterns | "To deploy: run CI then approve PR" |
| `summary` | Compressed knowledge | "Summary of Q1 architecture decisions" |
| `checkpoint` | Stable reference points | "Sprint 5 state snapshot" |
| `image` | Image descriptions | "Architecture diagram of auth flow" |
| `audio` | Audio transcripts | "Meeting recording summary" |
| `video` | Video descriptions | "Demo walkthrough of onboarding" |

### Memory Tiers

| Tier | Behavior | `expires_at` |
|------|----------|-------------|
| `permanent` | Never expires, persists forever | Always `null` |
| `daily` | Auto-expires after TTL (default 24h) | Always set |

### Memory Scope

| Scope | Visibility | Use Case |
|-------|-----------|----------|
| `global` | System-wide | Shared knowledge |
| `user` | Persists across sessions for a user | User preferences |
| `session` | One conversation only | Temporary context |
| `agent` | Specific agent instance | Agent-private state |

### Lifecycle States

| State | In Search? | Meaning |
|-------|-----------|---------|
| `active` | Yes | Normal, recently accessed |
| `stale` | Yes | Not accessed recently |
| `archived` | No (unless `include_archived: true`) | Compressed or summarized |

---

## 3. Memory CRUD

### Create a Memory

```json
{
  "name": "memory_create",
  "arguments": {
    "content": "The client prefers communication via WhatsApp",
    "type": "preference",
    "tags": ["client", "communication"],
    "workspace": "crm",
    "importance": 0.8,
    "metadata": {
      "client_id": "12345",
      "source": "onboarding-call"
    }
  }
}
```

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `content` | string | Yes | — | Memory text (non-empty) |
| `type` | string | No | `note` | Memory type (see table above) |
| `tags` | string[] | No | `[]` | Searchable labels |
| `workspace` | string | No | `default` | Namespace (lowercase, `[a-z0-9_-]`, max 64 chars) |
| `importance` | float | No | `0.5` | 0.0–1.0 priority weight |
| `metadata` | object | No | `{}` | Arbitrary JSON key-value pairs |
| `scope` | string | No | `global` | Visibility scope |
| `tier` | string | No | `permanent` | `permanent` or `daily` |
| `ttl_seconds` | integer | No | — | Override TTL for daily tier |
| `defer_embedding` | boolean | No | `false` | Skip immediate embedding (batch later) |
| `dedup_mode` | string | No | `allow` | `allow`, `reject`, `skip`, `merge` |
| `dedup_threshold` | float | No | `0.95` | Similarity threshold for dedup |
| `media_url` | string | No | — | URL/path for multimodal memories |

### Create Daily (Auto-Expiring) Memory

```json
{
  "name": "memory_create_daily",
  "arguments": {
    "content": "Cached lookup: João Silva, CPF 123.456.789-00",
    "tags": ["person", "cpf:12345678900"],
    "workspace": "crm",
    "ttl_seconds": 86400
  }
}
```

Creates a `daily` tier memory that auto-expires. Perfect for caching API responses.

### Create Batch

```json
{
  "name": "memory_create_batch",
  "arguments": {
    "memories": [
      {"content": "Fact 1", "type": "note", "tags": ["import"]},
      {"content": "Fact 2", "type": "note", "tags": ["import"]},
      {"content": "Fact 3", "type": "note", "tags": ["import"]}
    ],
    "workspace": "knowledge"
  }
}
```

### Get a Memory

```json
{
  "name": "memory_get",
  "arguments": {"id": 42}
}
```

Returns the full memory including content, metadata, tags, timestamps, and importance.

### Get Public (Strips Private Sections)

```json
{
  "name": "memory_get_public",
  "arguments": {"id": 42}
}
```

Removes any `<private>...</private>` sections from content. Use when sharing memory externally.

### Update a Memory

```json
{
  "name": "memory_update",
  "arguments": {
    "id": 42,
    "content": "Updated preference: prefers Telegram over WhatsApp",
    "tags": ["client", "communication", "updated"],
    "importance": 0.9,
    "media_url": null
  }
}
```

All fields except `id` are optional. Pass `null` to clear nullable fields like `event_time`, `trigger_pattern`, or `media_url`.

### Delete a Memory

```json
{
  "name": "memory_delete",
  "arguments": {"id": 42}
}
```

### Delete Batch

```json
{
  "name": "memory_delete_batch",
  "arguments": {"ids": [42, 43, 44]}
}
```

### List Memories

```json
{
  "name": "memory_list",
  "arguments": {
    "workspace": "crm",
    "limit": 20,
    "offset": 0,
    "tags": ["client"],
    "type": "preference",
    "tier": "permanent"
  }
}
```

### List Compact (IDs + Titles Only)

```json
{
  "name": "memory_list_compact",
  "arguments": {
    "workspace": "crm",
    "limit": 50
  }
}
```

Returns lightweight entries — ideal for building UI lists or quick scans.

### Promote to Permanent

```json
{
  "name": "memory_promote_to_permanent",
  "arguments": {"id": 42}
}
```

Converts a `daily` memory to `permanent`. Clears `expires_at`.

### Boost Importance

```json
{
  "name": "memory_boost",
  "arguments": {
    "id": 42,
    "boost": 0.1
  }
}
```

Increases importance score. Capped at 1.0.

---

## 4. Search

### Hybrid Search (Default)

```json
{
  "name": "memory_search",
  "arguments": {
    "query": "client communication preferences",
    "workspace": "crm",
    "limit": 10,
    "min_score": 0.3
  }
}
```

Engram combines **4 search signals** automatically:

1. **BM25** — keyword matching via SQLite FTS5
2. **Vector similarity** — semantic embeddings via sqlite-vec
3. **Fuzzy matching** — typo tolerance
4. **Reciprocal Rank Fusion (RRF)** — merges all signals
5. **Multi-signal reranking** — recency, importance, quality

**Search Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `query` | string | — | Search text |
| `limit` | integer | `10` | Max results |
| `min_score` | float | `0.0` | Minimum relevance threshold |
| `tags` | string[] | — | Filter by tags (AND logic) |
| `type` | string | — | Filter by memory type |
| `workspace` | string | — | Single workspace filter |
| `workspaces` | string[] | — | Multiple workspaces (OR) |
| `strategy` | string | `hybrid` | `hybrid`, `keyword_only`, `semantic_only` |
| `explain` | boolean | `false` | Include match explanations |
| `scope` | string | — | Scope filter |
| `tier` | string | — | Tier filter |
| `include_archived` | boolean | `false` | Include archived memories |
| `include_transcripts` | boolean | `false` | Include `transcript_chunk` type |
| `filter` | object | — | Advanced metadata filters |

### Search with Explanations

```json
{
  "name": "memory_search",
  "arguments": {
    "query": "authentication flow",
    "explain": true,
    "limit": 5
  }
}
```

Each result includes a breakdown of why it matched (keyword score, semantic score, recency boost, etc.).

### Search Suggestions

```json
{
  "name": "memory_search_suggest",
  "arguments": {
    "query": "authentcation"
  }
}
```

Returns spelling corrections and query suggestions.

### Cross-Workspace Search

```json
{
  "name": "memory_search",
  "arguments": {
    "query": "João Silva",
    "workspaces": ["crm", "notes", "sessions"]
  }
}
```

---

## 5. Cognitive Memory Types

### Episodic Memory (Events)

Record events with temporal context:

```json
{
  "name": "memory_create_episodic",
  "arguments": {
    "content": "Deployed v2.0 to production. Zero downtime.",
    "event_time": "2026-03-19T14:30:00Z",
    "event_duration_seconds": 300,
    "tags": ["deployment", "v2.0"],
    "workspace": "ops"
  }
}
```

Query events by time range:

```json
{
  "name": "memory_get_timeline",
  "arguments": {
    "start": "2026-03-01T00:00:00Z",
    "end": "2026-03-31T23:59:59Z",
    "workspace": "ops",
    "limit": 50
  }
}
```

### Procedural Memory (How-To Patterns)

Record learned procedures:

```json
{
  "name": "memory_create_procedural",
  "arguments": {
    "content": "To deploy: 1) Run CI pipeline 2) Wait for green 3) Approve PR 4) Merge to main 5) Monitor Grafana for 15 min",
    "trigger_pattern": "deploy to production",
    "tags": ["deployment", "runbook"],
    "workspace": "ops"
  }
}
```

Track outcomes to build success rates:

```json
{
  "name": "record_procedure_outcome",
  "arguments": {
    "id": 99,
    "success": true
  }
}
```

Query procedures filtered by success rate:

```json
{
  "name": "memory_get_procedures",
  "arguments": {
    "trigger": "deploy",
    "min_success_rate": 0.8,
    "workspace": "ops"
  }
}
```

### Summary Memory (Compressed Knowledge)

```json
{
  "name": "memory_create",
  "arguments": {
    "content": "Q1 2026: Shipped auth rewrite, migrated to PostgreSQL, onboarded 3 new devs. Key decision: chose gRPC over REST for internal services.",
    "type": "summary",
    "tags": ["q1-2026", "architecture"],
    "workspace": "decisions"
  }
}
```

### Checkpoint Memory (State Snapshots)

```json
{
  "name": "memory_checkpoint",
  "arguments": {
    "content": "Sprint 12 complete. Auth module stable. Payment integration 80% done. Blocking: Stripe webhook reliability.",
    "tags": ["sprint-12"],
    "workspace": "project"
  }
}
```

---

## 6. Knowledge Graph

### Link Memories

```json
{
  "name": "memory_link",
  "arguments": {
    "source_id": 10,
    "target_id": 20,
    "relation": "depends_on",
    "weight": 0.9
  }
}
```

**Built-in relation types:** `related_to`, `depends_on`, `caused_by`, `derived_from`, `supersedes`, `contradicts`, `supports`, `part_of`, `example_of`

### Get Related Memories

```json
{
  "name": "memory_related",
  "arguments": {
    "id": 10,
    "relation": "depends_on",
    "limit": 10
  }
}
```

### Multi-Hop Graph Traversal (BFS)

```json
{
  "name": "memory_traverse",
  "arguments": {
    "start_id": 10,
    "max_depth": 3,
    "relation": "depends_on",
    "limit": 50
  }
}
```

Traverses the knowledge graph breadth-first from a starting memory.

### Find Shortest Path

```json
{
  "name": "memory_find_path",
  "arguments": {
    "source_id": 10,
    "target_id": 50,
    "max_depth": 5
  }
}
```

Returns the shortest path between two memories through the graph.

### Unlink Memories

```json
{
  "name": "memory_unlink",
  "arguments": {
    "source_id": 10,
    "target_id": 20,
    "relation": "depends_on"
  }
}
```

### Export Knowledge Graph

```json
{
  "name": "memory_export_graph",
  "arguments": {
    "workspace": "project",
    "format": "json"
  }
}
```

---

## 7. Identity & Cross-Reference

Unify multiple identifiers (CPF, phone, email) into a single identity:

### Create Identity

```json
{
  "name": "identity_create",
  "arguments": {
    "canonical_id": "person:12345678900",
    "display_name": "João Silva",
    "aliases": [
      "cpf:12345678900",
      "phone:+5511999887766",
      "email:joao@example.com"
    ]
  }
}
```

### Resolve Identity

```json
{
  "name": "identity_resolve",
  "arguments": {
    "alias": "phone:+5511999887766"
  }
}
```

Returns the canonical identity — look up any person by any known identifier.

### Add Alias

```json
{
  "name": "identity_add_alias",
  "arguments": {
    "canonical_id": "person:12345678900",
    "alias": "whatsapp:+5511999887766"
  }
}
```

---

## 8. Session Management

### Index a Conversation

```json
{
  "name": "session_index",
  "arguments": {
    "session_id": "chat-2026-03-19-001",
    "messages": [
      {"role": "user", "content": "What properties does João own?"},
      {"role": "assistant", "content": "João Silva owns 3 properties in São Paulo..."},
      {"role": "user", "content": "Show me the one in Pinheiros"},
      {"role": "assistant", "content": "The apartment at Rua dos Pinheiros..."}
    ],
    "workspace": "sessions"
  }
}
```

Creates searchable `transcript_chunk` memories from conversation turns.

### Search Past Sessions

```json
{
  "name": "memory_session_search",
  "arguments": {
    "query": "properties in Pinheiros",
    "workspace": "sessions",
    "limit": 5
  }
}
```

---

## 9. Workspace Organization

Workspaces are namespaces that isolate memories by domain.

**Rules:**
- Lowercase only: `[a-z0-9_-]`
- Max 64 characters
- Cannot start with `_` (reserved for system)
- Reserved names: `_system`, `_archive`

### Common Workspace Patterns

| Workspace | Purpose |
|-----------|---------|
| `default` | General-purpose |
| `crm` | Customer data, cached lookups |
| `notes` | User/broker notes |
| `sessions` | Conversation transcripts |
| `ops` | Operations, deployments, incidents |
| `decisions` | Architectural decisions |
| `knowledge` | Domain knowledge base |
| `agents` | Agent working state (M3) |

### Workspace Stats

```json
{
  "name": "memory_stats",
  "arguments": {
    "workspace": "crm"
  }
}
```

---

## 10. Cloud Sync

Engram can sync its SQLite database to S3-compatible cloud storage (AWS S3, Cloudflare R2, MinIO).

### Prerequisites

```bash
# Environment variables
export ENGRAM_STORAGE_URI=s3://your-bucket/memories.db
export ENGRAM_CLOUD_ENCRYPT=true  # AES-256 encryption

# For Cloudflare R2
export R2_ACCESS_KEY_ID=your-key
export R2_SECRET_ACCESS_KEY=your-secret
export AWS_ENDPOINT_URL=https://your-account.r2.cloudflarestorage.com
```

### Build with Cloud Feature

```bash
cargo build --release --features cloud
```

### Check Sync Status

```json
{
  "name": "memory_sync_status",
  "arguments": {}
}
```

Returns sync state: last sync time, pending changes, conflict count.

### Trigger Sync

Sync happens automatically on a debounced schedule. The sync layer:
- Detects local changes since last sync
- Uploads to S3/R2 with optional AES-256 encryption
- Handles conflict resolution (3-way merge)
- Supports bidirectional sync

### Media Asset Sync (Multimodal + Cloud)

```json
{
  "name": "memory_sync_media",
  "arguments": {
    "dry_run": true
  }
}
```

Uploads local media files (images, audio, video) from `media_assets` table to S3/R2. Set `dry_run: false` to actually upload.

**Requires:** `--features multimodal,cloud`

---

## 11. Multimodal Memory

Store and search across text, images, audio, and video.

### Create Image Memory

```json
{
  "name": "memory_create",
  "arguments": {
    "content": "Architecture diagram showing the auth flow: client -> gateway -> auth service -> database",
    "type": "image",
    "media_url": "local:///path/to/diagram.png",
    "tags": ["architecture", "auth"],
    "workspace": "docs"
  }
}
```

### Create Audio Memory

```json
{
  "name": "memory_create",
  "arguments": {
    "content": "Weekly standup recording. Key points: sprint on track, blocker on payments API",
    "type": "audio",
    "media_url": "https://storage.example.com/standup-2026-03-19.mp3",
    "tags": ["standup", "sprint-12"],
    "workspace": "meetings"
  }
}
```

### Create Video Memory

```json
{
  "name": "memory_create",
  "arguments": {
    "content": "Onboarding walkthrough demo: account creation, profile setup, first project",
    "type": "video",
    "media_url": "s3://media-bucket/onboarding-demo.mp4",
    "tags": ["onboarding", "demo"],
    "workspace": "docs"
  }
}
```

### Search by Image

```json
{
  "name": "memory_search_by_image",
  "arguments": {
    "image_url": "local:///path/to/query-image.png",
    "limit": 5,
    "workspace": "docs"
  }
}
```

Uses CLIP-style cross-modal embeddings. Falls back to vision model description + text search.

**Requires:** `--features multimodal`

---

## 12. Snapshots & Portability

Create portable `.egm` knowledge packages that can be shared between Engram instances.

### Create Snapshot

```json
{
  "name": "snapshot_create",
  "arguments": {
    "workspace": "knowledge",
    "output_path": "/tmp/broker-knowledge.egm",
    "encrypt": true,
    "passphrase": "my-secret-passphrase",
    "sign": true
  }
}
```

**Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `workspace` | string | — | Workspace to export |
| `output_path` | string | — | Where to save the `.egm` file |
| `encrypt` | boolean | `false` | AES-256-GCM encryption |
| `passphrase` | string | — | Required if `encrypt: true` |
| `sign` | boolean | `false` | Ed25519 digital signature |
| `tags` | string[] | — | Filter memories by tags |
| `include_graph` | boolean | `true` | Include knowledge graph edges |

### Load Snapshot

```json
{
  "name": "snapshot_load",
  "arguments": {
    "path": "/tmp/broker-knowledge.egm",
    "strategy": "merge",
    "passphrase": "my-secret-passphrase",
    "target_workspace": "imported-knowledge"
  }
}
```

**Load Strategies:**

| Strategy | Behavior |
|----------|----------|
| `merge` | Add new memories, skip duplicates (by content hash) |
| `replace` | Delete target workspace, then load all memories |
| `isolate` | Load into a separate workspace (no collision) |
| `dry_run` | Report what would happen without making changes |

### Inspect Snapshot

```json
{
  "name": "snapshot_inspect",
  "arguments": {
    "path": "/tmp/broker-knowledge.egm"
  }
}
```

Returns metadata: memory count, workspace, creation date, encryption status, signature status — without loading the data.

**Requires:** `--features agent-portability`

---

## 13. Attestation Chain

Track the provenance of knowledge — who ingested what, when, and with what hash.

### Log Attestation

```json
{
  "name": "attestation_log",
  "arguments": {
    "document_path": "/docs/api-spec.md",
    "content_hash": "sha256:abc123...",
    "source": "manual-import",
    "sign": true
  }
}
```

### Verify Document

```json
{
  "name": "attestation_verify",
  "arguments": {
    "content_hash": "sha256:abc123..."
  }
}
```

### Verify Chain Integrity

```json
{
  "name": "attestation_chain_verify",
  "arguments": {}
}
```

Verifies the entire attestation chain — each record links to its predecessor via SHA-256 hashes, forming a tamper-evident log.

### List Attestations

```json
{
  "name": "attestation_list",
  "arguments": {
    "format": "json",
    "limit": 20
  }
}
```

Formats: `json`, `csv`, `merkle_proof`.

**Requires:** `--features agent-portability`

---

## 14. Retention Policies

Automate memory lifecycle management — compress old memories, enforce limits, purge expired data.

### Set Retention Policy

```json
{
  "name": "memory_set_retention_policy",
  "arguments": {
    "workspace": "crm",
    "type": "note",
    "max_age_days": 90,
    "max_count": 1000,
    "compress_after_days": 30,
    "action": "archive"
  }
}
```

### Apply Retention Policies

```json
{
  "name": "memory_apply_retention_policies",
  "arguments": {}
}
```

Runs the 3-phase retention pipeline:
1. **Compress** — summarize old memories past `compress_after_days`
2. **Enforce max** — trim oldest memories when `max_count` exceeded
3. **Hard age** — delete/archive memories past `max_age_days`

### List Policies

```json
{
  "name": "memory_list_retention_policies",
  "arguments": {}
}
```

### Delete Policy

```json
{
  "name": "memory_delete_retention_policy",
  "arguments": {
    "id": 5
  }
}
```

---

## 15. Project Context Scanning

Automatically discover and ingest AI instruction files from project directories.

### Scan Project

```json
{
  "name": "memory_scan_project",
  "arguments": {
    "path": "/Users/dev/my-project",
    "scan_parents": true,
    "extract_sections": true
  }
}
```

**Detected files:** `CLAUDE.md`, `AGENTS.md`, `.cursorrules`, `.github/copilot-instructions.md`, `GEMINI.md`, `.aider.conf.yml`, `.windsurfrules`, `CONVENTIONS.md`, `CODING_GUIDELINES.md`

Content is hashed for idempotency — rescanning the same file with unchanged content is a no-op.

### Get Project Context

```json
{
  "name": "memory_get_project_context",
  "arguments": {
    "path": "/Users/dev/my-project",
    "include_sections": true
  }
}
```

### List Instruction Files

```json
{
  "name": "list_instruction_files",
  "arguments": {
    "path": "/Users/dev/my-project",
    "scan_parents": true
  }
}
```

Lists detected files without ingesting them.

---

## 16. Entity Extraction

Extract named entities (people, organizations, technologies, locations) from memory content.

### Extract Entities

```json
{
  "name": "memory_extract_entities",
  "arguments": {
    "id": 42
  }
}
```

Idempotent — re-extracting from the same memory is safe.

### Search by Entity

```json
{
  "name": "memory_search_entities",
  "arguments": {
    "entity": "PostgreSQL",
    "limit": 10
  }
}
```

---

## 17. Semantic Deduplication

Prevent duplicate memories using embedding-based similarity detection.

### Find Duplicates

```json
{
  "name": "memory_find_semantic_duplicates",
  "arguments": {
    "workspace": "crm",
    "threshold": 0.9,
    "limit": 20
  }
}
```

Returns pairs of memories with cosine similarity above the threshold.

### Merge Duplicates

```json
{
  "name": "memory_merge",
  "arguments": {
    "source_id": 42,
    "target_id": 43
  }
}
```

Merges `source` into `target` — combines tags and metadata, deletes source.

### Create with Dedup

```json
{
  "name": "memory_create",
  "arguments": {
    "content": "User prefers dark mode",
    "dedup_mode": "skip",
    "dedup_threshold": 0.95,
    "workspace": "settings"
  }
}
```

| Mode | Behavior on Duplicate |
|------|-----------------------|
| `allow` | Create anyway (default) |
| `reject` | Return error |
| `skip` | Return existing memory |
| `merge` | Update existing with new tags/metadata |

---

## 18. Advanced Filtering

Use structured filters for complex queries beyond text search.

### Metadata Filters

```json
{
  "name": "memory_list",
  "arguments": {
    "workspace": "crm",
    "filter": {
      "AND": [
        {"metadata.client_id": {"eq": "12345"}},
        {"metadata.source": {"eq": "api"}}
      ]
    }
  }
}
```

### Filter Operators

| Operator | Example | Description |
|----------|---------|-------------|
| `eq` | `{"field": {"eq": "value"}}` | Exact match |
| `ne` | `{"field": {"ne": "value"}}` | Not equal |
| `gt` / `gte` | `{"field": {"gt": 5}}` | Greater than |
| `lt` / `lte` | `{"field": {"lt": 10}}` | Less than |
| `contains` | `{"field": {"contains": "sub"}}` | String contains |
| `in` | `{"field": {"in": ["a", "b"]}}` | Value in list |

### Combining Filters

```json
{
  "filter": {
    "OR": [
      {"metadata.priority": {"eq": "high"}},
      {"AND": [
        {"metadata.priority": {"eq": "medium"}},
        {"importance": {"gte": 0.8}}
      ]}
    ]
  }
}
```

---

## 19. Multi-Agent Sync (Cloud)

When using Engram Cloud, multiple AI agents can coordinate through shared memory.

### List Sessions by Agent

```json
{
  "name": "session_list_by_agent",
  "arguments": {
    "agent_id": "sales-agent-001"
  }
}
```

### Global Session Search (All Agents)

```json
{
  "name": "session_search_global",
  "arguments": {
    "query": "pricing discussion",
    "limit": 10
  }
}
```

Searches across all agents' session transcripts.

### Set Agent Context

```json
{
  "name": "agent_set_context",
  "arguments": {
    "agent_id": "sales-agent-001",
    "context": {
      "current_task": "follow-up-call",
      "client_cpf": "12345678900",
      "notes": "Client interested in 3BR apartment in Pinheiros"
    }
  }
}
```

Persists agent working state. Upsert semantics (creates or updates).

### Get Agent Context

```json
{
  "name": "agent_get_context",
  "arguments": {
    "agent_id": "sales-agent-001"
  }
}
```

Returns the agent's saved context + last 5 session summaries.

---

## 20. Transport Options

Engram server supports multiple transport protocols simultaneously.

### stdio (Default — MCP Standard)

```bash
engram-server
```

Used by Claude Desktop, Cursor, and other MCP clients.

### HTTP

```bash
engram-server --transport http --http-port 3000 --http-api-key sk_my_secret
```

- Endpoint: `POST /v1/mcp`
- Auth: `Authorization: Bearer sk_my_secret`
- Protocol: JSON-RPC 2.0

### gRPC

```bash
engram-server --transport grpc --grpc-port 50051 --grpc-api-key my-secret
```

- Proto: `proto/mcp.proto`
- Auth: Bearer token in gRPC metadata
- Streaming: `Subscribe` RPC for real-time events

### Both (stdio + HTTP)

```bash
engram-server --transport both --http-port 3000 --http-api-key sk_my_secret
```

---

## 21. Watcher Daemon

The Engram Watcher is a separate binary that proactively captures context from your environment.

### Three Capture Sources

| Source | What it Captures |
|--------|-----------------|
| **File System** | File changes in watched directories (debounced) |
| **Browser History** | URLs visited in Chrome/Firefox/Safari (polling) |
| **App Focus** | Which app has focus and for how long (platform-specific) |

### Running the Watcher

```bash
# Dry-run mode (log events, don't send)
engram-watcher --dry-run --verbose

# Normal mode (sends to engram-server via HTTP)
engram-watcher

# Custom config
engram-watcher --config /path/to/watcher.toml
```

### Configuration (`watcher.toml`)

```toml
engram_url = "http://localhost:3000"
workspace = "watcher"

[file_watcher]
enabled = true
paths = ["/home/user/projects", "/home/user/notes"]
extensions = ["rs", "md", "txt", "py", "ts"]
debounce_ms = 500
ignore_patterns = [".git", "node_modules", "target"]

[browser]
enabled = true
browsers = ["chrome", "firefox"]
poll_interval_secs = 60
exclude_patterns = ["localhost", "127.0.0.1", "about:"]

[app_focus]
enabled = false
poll_interval_secs = 5
min_focus_secs = 1
exclude_apps = ["Finder", "Dock"]
```

**Config location:**
- macOS: `~/Library/Application Support/engram/watcher.toml`
- Linux: `~/.config/engram/watcher.toml`

**Requires:** `--features watcher`

---

## 22. Recipes & Patterns

### Pattern 1: Cold Start — Seed a Knowledge Base

```json
{
  "name": "context_seed",
  "arguments": {
    "facts": [
      {"content": "Our API uses REST with JSON responses", "type": "context", "confidence": 0.95},
      {"content": "PostgreSQL 15 is the primary database", "type": "context", "confidence": 0.9},
      {"content": "All endpoints require Bearer token auth", "type": "context", "confidence": 1.0}
    ],
    "workspace": "project"
  }
}
```

Higher confidence → longer TTL. Confidence 1.0 creates permanent memories.

### Pattern 2: CRM Lookup Caching

```
1. Search Engram for cached data:
   memory_search("CPF 12345678900", workspace: "crm")

2. If no results, fetch from API and cache:
   memory_create_daily(content: "Person summary...", tags: ["person", "cpf:12345678900"], workspace: "crm", ttl_seconds: 86400)

3. Create identity for cross-reference:
   identity_create(canonical_id: "person:12345678900", display_name: "João", aliases: ["cpf:12345678900", "phone:+55..."])
```

### Pattern 3: Conversation Memory

```
1. During chat, save important insights:
   memory_create(content: "User wants 3BR in Pinheiros under R$2M", type: "preference", workspace: "notes")

2. After 4+ messages, index the conversation:
   session_index(session_id: "chat-001", messages: [...], workspace: "sessions")

3. In future chats, recall context:
   memory_search("apartment Pinheiros", workspaces: ["notes", "sessions", "crm"])
```

### Pattern 4: Decision Log

```
1. Record decisions as they happen:
   memory_create(content: "Chose gRPC for internal services — 3x faster than REST for our payload sizes", type: "decision", tags: ["architecture", "grpc"])

2. Link related decisions:
   memory_link(source_id: 10, target_id: 20, relation: "supports")

3. Later, review decision chain:
   memory_traverse(start_id: 10, max_depth: 3, relation: "supports")
```

### Pattern 5: Runbook with Tracking

```
1. Record the procedure:
   memory_create_procedural(content: "Deployment steps: ...", trigger_pattern: "deploy to prod")

2. After each deployment, record outcome:
   record_procedure_outcome(id: 99, success: true)
   record_procedure_outcome(id: 99, success: false)

3. Query reliable procedures:
   memory_get_procedures(trigger: "deploy", min_success_rate: 0.9)
```

### Pattern 6: Knowledge Package Distribution

```
1. Build knowledge base in workspace:
   memory_create(..., workspace: "broker-onboarding")

2. Package for distribution:
   snapshot_create(workspace: "broker-onboarding", output_path: "broker-kit.egm", encrypt: true, passphrase: "secret")

3. New broker loads the package:
   snapshot_load(path: "broker-kit.egm", strategy: "merge", passphrase: "secret", target_workspace: "my-knowledge")
```

### Pattern 7: Graceful Degradation

Always handle Engram being unavailable:

```python
try:
    result = engram.search("query", workspace="crm")
except Exception:
    result = None  # Engram is optional — continue without cached data

# The app works with or without memory
if result:
    use_cached_data(result)
else:
    fetch_from_primary_api()
```

---

## 23. Tool Reference

### Quick Reference Table

| Category | Tools |
|----------|-------|
| **CRUD** | `memory_create`, `memory_get`, `memory_get_public`, `memory_update`, `memory_delete`, `memory_create_batch`, `memory_delete_batch` |
| **List** | `memory_list`, `memory_list_compact` |
| **Search** | `memory_search`, `memory_search_suggest`, `memory_search_by_image` |
| **Lifecycle** | `memory_create_daily`, `memory_promote_to_permanent`, `memory_boost`, `memory_checkpoint` |
| **Cognitive** | `memory_create_episodic`, `memory_create_procedural`, `memory_get_timeline`, `memory_get_procedures`, `record_procedure_outcome` |
| **Graph** | `memory_link`, `memory_unlink`, `memory_related`, `memory_traverse`, `memory_find_path`, `memory_export_graph` |
| **Identity** | `identity_create`, `identity_resolve`, `identity_add_alias` |
| **Session** | `session_index`, `memory_session_search` |
| **Dedup** | `memory_find_semantic_duplicates`, `memory_merge` |
| **Retention** | `memory_set_retention_policy`, `memory_get_retention_policy`, `memory_list_retention_policies`, `memory_delete_retention_policy`, `memory_apply_retention_policies` |
| **Entities** | `memory_extract_entities`, `memory_search_entities` |
| **Context** | `context_seed`, `memory_scan_project`, `memory_get_project_context`, `list_instruction_files` |
| **Snapshot** | `snapshot_create`, `snapshot_load`, `snapshot_inspect` |
| **Attestation** | `attestation_log`, `attestation_verify`, `attestation_chain_verify`, `attestation_list` |
| **Cloud** | `memory_sync_status`, `memory_sync_media` |
| **Admin** | `memory_stats` |
| **Multi-Agent** | `session_list_by_agent`, `session_search_global`, `agent_set_context`, `agent_get_context` |

### Tool Annotations

Every tool includes MCP 2025-11-25 annotations:

| Annotation | Meaning |
|------------|---------|
| `readOnlyHint: true` | Safe to call — no data changes |
| `destructiveHint: true` | Modifies or deletes data |
| `idempotentHint: true` | Safe to retry — same result on repeated calls |

### Feature Gates

| Feature Flag | Required For |
|-------------|-------------|
| `cloud` | `memory_sync_status`, `memory_sync_media` (with multimodal) |
| `multimodal` | `memory_search_by_image`, Image/Audio/Video types |
| `agent-portability` | `snapshot_*`, `attestation_*` |
| `watcher` | `engram-watcher` binary |
| `grpc` | gRPC transport |
| `openai` | OpenAI embeddings (vs default TF-IDF) |

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ENGRAM_DB_PATH` | SQLite database path | `~/.local/share/engram/memories.db` |
| `ENGRAM_STORAGE_URI` | S3 URI for cloud sync | — |
| `ENGRAM_CLOUD_ENCRYPT` | AES-256 encryption for cloud | `false` |
| `ENGRAM_EMBEDDING_MODEL` | `tfidf` or `openai` | `tfidf` |
| `OPENAI_API_KEY` | Required for OpenAI embeddings | — |
| `R2_ACCESS_KEY_ID` | Cloudflare R2 access key | — |
| `R2_SECRET_ACCESS_KEY` | Cloudflare R2 secret | — |
| `AWS_ENDPOINT_URL` | Custom S3 endpoint | — |
| `ENGRAM_S3_BUCKET` | S3 bucket for media sync | — |
| `ENGRAM_MEDIA_PUBLIC_DOMAIN` | CDN domain for media URLs | — |

---

## Performance Targets

| Operation | Target |
|-----------|--------|
| Create memory (no embedding) | < 200 µs |
| Get by ID | < 100 µs |
| Hybrid search (100K+ memories) | < 10 ms |
| Entity extraction | < 50 ms |
| Snapshot create (1K memories) | < 1 s |
| Snapshot load (1K memories) | < 2 s |

---

## 24. Progressive Tool Discovery

Engram exposes 220+ tools. To avoid overwhelming agents, tools are organized into three tiers:

| Tier | Count | Description |
|------|-------|-------------|
| **Essential** | ~20 | Core tools every agent needs: CRUD, search, stats, sessions |
| **Standard** | ~57 | Common operations: lifecycle, quality, identity, context engineering |
| **Advanced** | ~104 | Specialized: compression, evolution, attestation, multimodal |

### Controlling Exposure

Set `ENGRAM_TOOL_TIER` to control which tools are listed:

```bash
# Only essential tools (~20) — great for simple agents
ENGRAM_TOOL_TIER=essential cargo run --bin engram-server

# Essential + standard (~77) — recommended for most agents
ENGRAM_TOOL_TIER=standard cargo run --bin engram-server

# All tools (default, backward compatible)
ENGRAM_TOOL_TIER=all cargo run --bin engram-server
```

### Discovering Tools

The `discover_tools` tool is always available regardless of tier setting:

```json
{"tool": "discover_tools", "params": {"tier": "standard"}}
{"tool": "discover_tools", "params": {"category": "search"}}
{"tool": "discover_tools", "params": {"search": "graph"}}
```

Response includes tool names, descriptions, tiers, and summary counts. Agents can progressively discover capabilities as needed.

---

## 25. Session Handoff Protocol

Inspired by Beads' "land the plane" pattern, Engram provides a structured end-of-session handoff:

### session_land Tool

```json
{
  "tool": "session_land",
  "params": {
    "session_id": "coding-session-42",
    "workspace": "default",
    "summary": "Implemented auth middleware and wrote unit tests",
    "next_session_hints": [
      "Integration tests still needed",
      "Review rate limiting config"
    ]
  }
}
```

Returns a structured handoff with:
- **summary** — what was accomplished
- **open_items** — unfinished todos and issues
- **recent_decisions** — decisions made in the last 24h
- **bootstrap_prompt** — ready-to-use prompt to start the next session

A checkpoint memory is automatically created with the handoff data.

### session-handoff Prompt

Use the MCP prompt for a guided workflow:

```json
{"method": "prompts/get", "params": {"name": "session-handoff", "arguments": {"session_id": "my-session"}}}
```

This guides you through: summarize progress → capture open items → call session_land → report bootstrap prompt.

---

## 26. Markdown Export

Export memories as human-readable Markdown files, inspired by Basic Memory:

```json
{
  "tool": "memory_export_markdown",
  "params": {
    "workspace": "default",
    "output_dir": "./my-knowledge-base/",
    "include_links": true
  }
}
```

### Output Structure

```
my-knowledge-base/
├── index.md                    # Workspace overview + table of contents
├── notes/
│   ├── 42-project-architecture.md
│   └── 57-api-design-notes.md
├── decisions/
│   └── 63-chose-postgresql.md
└── todos/
    └── 71-add-rate-limiting.md
```

### File Format

Each file includes YAML frontmatter and optional wiki-style `[[links]]`:

```markdown
---
id: 42
type: note
tags: ["architecture", "backend"]
importance: 0.80
tier: permanent
created_at: "2026-03-20T10:30:00Z"
---

Project architecture uses a layered approach with...

## Related

- relates_to [[63-chose-postgresql]]
- supports [[57-api-design-notes]]
```

---

## 27. Recent Activity

Discover what changed recently with the `recent_activity` tool:

```json
{
  "tool": "recent_activity",
  "params": {
    "workspace": "default",
    "timeframe": "24h",
    "limit": 10
  }
}
```

Returns compact previews sorted by most recent activity:

```json
{
  "activities": [
    {
      "id": 42,
      "preview": "Project architecture uses a layered approach with...",
      "memory_type": "note",
      "tags": "architecture,backend",
      "created_at": "2026-03-20T10:30:00Z",
      "updated_at": "2026-03-20T14:15:00Z"
    }
  ],
  "count": 1,
  "timeframe": "24h"
}
```

Timeframe options: `"1h"`, `"24h"`, `"7d"`, `"30d"`.

### Enhanced Context Building

`memory_build_context` now supports depth traversal and timeframe filtering:

```json
{
  "tool": "memory_build_context",
  "params": {
    "query": "authentication",
    "depth": 2,
    "timeframe": "7d",
    "include_types": ["note", "decision"],
    "include_graph": true,
    "total_budget": 4096
  }
}
```

- **depth** (1-3): Follow related memory links for richer context
- **timeframe**: Filter to recent memories only
- **include_types**: Restrict to specific memory types
- **include_graph**: Include entity relationship edges in response

---

*Engram v0.20.0 — AI Memory Engine*
*220+ MCP tools | Hybrid search | Knowledge graphs | Cloud sync | Multimodal | Agent portability | Progressive discovery*
