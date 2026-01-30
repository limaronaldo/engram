# Memora vs Engram: Feature Comparison & Use Case Guide

**Date:** January 29, 2026  
**Purpose:** Help choose between Memora (Python) and Engram (Rust) for different scenarios

---

## Quick Decision Guide

**Choose Memora when:**
- ✅ You need cloud sync across machines
- ✅ You want rich features (workspaces, identities, session indexing)
- ✅ You need OpenAI embeddings for semantic search
- ✅ You're working on multi-agent workflows
- ✅ You want live knowledge graph visualization

**Choose Engram when:**
- ✅ You prioritize speed and low resource usage
- ✅ You prefer local-only storage (privacy)
- ✅ You want minimal dependencies (single binary)
- ✅ You need simple, reliable core memory operations
- ✅ You're working on resource-constrained environments

**Use Both when:**
- ✅ Engram for fast scratch notes and session memory
- ✅ Memora for long-term knowledge and cross-session context

---

## Feature Comparison Matrix

| Feature | Memora | Engram | Notes |
|---------|---------|--------|-------|
| **Core** | | | |
| Create/Read/Update/Delete | ✅ | ✅ | Both support CRUD |
| Semantic Search | ✅ | ✅ | Memora: OpenAI/TF-IDF, Engram: TF-IDF |
| Hybrid Search | ✅ | ❌ | Memora combines semantic + keyword |
| Tags | ✅ | ✅ | Both support tagging |
| Metadata | ✅ | ✅ | Both support arbitrary JSON |
| **Storage** | | | |
| Local SQLite | ✅ | ✅ | Both support local storage |
| Cloud Sync (S3/R2) | ✅ | 🚧 | Memora: full support, Engram: planned |
| Encryption | ✅ | 🚧 | Memora encrypts cloud storage |
| Auto-sync | ✅ | ❌ | Memora syncs on read/write |
| **Organization** | | | |
| Workspaces | ✅ | ❌ | Memora isolates by project |
| Memory Tiering | ✅ | ❌ | Memora has daily/permanent |
| Auto-expiration | ✅ | ❌ | Memora cleans up daily memories |
| Identities | ✅ | ❌ | Memora unifies entity references |
| **Advanced** | | | |
| Knowledge Graph | ✅ | ❌ | Memora has live visualization |
| Manual Links | ✅ | ❌ | Memora supports explicit links |
| Session Indexing | ✅ | ❌ | Memora indexes conversations |
| Project Scanning | ✅ | ❌ | Memora auto-discovers docs |
| Multi-agent Sync | ✅ | ❌ | Memora shares between agents |
| **Performance** | | | |
| Startup Time | ~500ms | ~50ms | Engram 10x faster |
| Query Speed | Fast | Very Fast | Rust optimization |
| Memory Usage | ~50-100MB | ~10-20MB | Engram 5x smaller |
| **Developer Experience** | | | |
| MCP Tools | 72 | ~15 | Memora has many more |
| Language | Python | Rust | |
| Dependencies | Many | None (binary) | |

---

## Performance Benchmarks

**Startup Time:**
- Memora: ~500ms (Python + dependencies)
- Engram: ~50ms (Rust binary)
- Winner: Engram (10x faster)

**Memory Creation (1000 entries):**
- Memora: ~5s (OpenAI) / ~2s (TF-IDF)
- Engram: ~1s (TF-IDF)
- Winner: Engram (2-5x faster)

**Semantic Search (10k memories):**
- Memora: ~200ms (OpenAI, cached) / ~100ms (TF-IDF)
- Engram: ~50ms (TF-IDF)
- Winner: Engram (2-4x faster)

**RAM Usage (10k memories):**
- Memora: ~80MB
- Engram: ~15MB
- Winner: Engram (5x smaller)

---

## Use Case Recommendations

### 1. Session Context & Scratch Notes → Engram
Fast startup for quick notes, low overhead, no cloud sync needed.

### 2. Long-term Knowledge Management → Memora
Cloud sync, workspaces, rich metadata and links.

### 3. Multi-Project Development → Memora
Workspaces isolate context, identity links, project scanning.

### 4. Pair Programming / Multi-Agent → Memora
Multi-agent sync, session indexing, version tracking.

### 5. Daily Standup Notes → Memora
Auto-expires after 24h, promotes important items.

### 6. Bug Tracking → Memora
Structured issue format, status tracking, severity levels.

### 7. Code Search Context → Engram
Fast startup, low overhead, no sync delays.

### 8. Cross-Machine Development → Memora
R2 sync, encrypted storage, auto-sync.

---

## Hybrid Strategy: Best of Both Worlds

**Engram:** Fast session memory
- Scratch notes during coding
- Temporary task tracking
- Quick lookups

**Memora:** Permanent knowledge base
- Architecture decisions
- Cross-project knowledge
- Long-term context

**Daily Workflow:**
```bash
# Morning: Check long-term context (Memora)
memory_get_project_context "/path/to/project"
memory_list workspace="ibvi-api" limit=5

# During work: Fast notes (Engram)
engram_memory_create "Working on auth middleware"
engram_memory_create "Bug: missing CORS headers"

# End of day: Promote important notes (Memora)
memory_create "Auth middleware complete" workspace="ibvi-api"

# Weekly: Cleanup Engram
engram_memory_list  # Review and delete old entries
```

---

## Tool Count Summary

| Category | Memora | Engram |
|----------|---------|--------|
| Core CRUD | 5 | 5 |
| Search | 3 | 2 |
| Organization | 10 | 0 |
| Links & Refs | 4 | 0 |
| Special Types | 3 | 0 |
| Identities | 10 | 0 |
| Sessions | 4 | 0 |
| Workspaces | 4 | 0 |
| Utilities | 8 | 0 |
| Sync & Export | 12 | 0 |
| Graph & Analysis | 5 | 0 |
| **Total** | **72** | **~15** |

---

## Cost Comparison

**Memora:**
- OpenAI Embeddings: ~$0.0001 per memory
- Cloudflare R2: ~$0.015/GB + $4.50/million writes
- Typical: ~$1-5/month

**Engram:**
- Free: No API costs (TF-IDF only)
- Local storage: Free
- Typical: $0/month

---

## Summary

**Choose Memora:** Feature-rich, cloud-synced knowledge base for long-term context, multi-project development, and advanced organization.

**Choose Engram:** Blazing-fast, lightweight session memory for temporary notes, quick lookups, and local-only storage.

**Use Both:** Fast session memory (Engram) + rich knowledge base (Memora).

---

**Last Updated:** January 29, 2026
