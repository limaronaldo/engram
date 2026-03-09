# Engram Roadmap

## Phase Overview

| Phase | Name | Status | Version | Description |
|-------|------|--------|---------|-------------|
| 0 | Storage Abstraction | Done | v0.1.0 | `StorageBackend` trait, SQLite + WAL, connection pooling, hybrid search (BM25 + vector + fuzzy + RRF) |
| 1 | Cognitive Memory Types | Done | v0.3.0 | Episodic, procedural, summary, checkpoint types with temporal queries |
| 2 | Context Compression | Done | v0.3.0 | Summarization, soft-trim, token budgeting (tiktoken-rs), batch archival |
| 3 | Langfuse Integration | Done | v0.3.0 | Trace sync, pattern extraction, async runtime (feature-gated) |
| 4 | Search Caching | Done | v0.3.0 | Embedding-based LRU cache, adaptive similarity threshold, feedback loop |
| 5 | Memory Lifecycle | Done | v0.3.0 | Active/Stale/Archived states, configurable thresholds, dry-run support |
| 6 | Turso/libSQL | Done | v0.3.0 | Distributed SQLite via Turso, full `StorageBackend` implementation |
| 7 | Meilisearch | Done | v0.5.0 | Full `StorageBackend` + background indexer, 4 MCP tools (feature-gated) |
| 8 | Salience Scoring | Done | v0.4.0 | Multi-signal salience (recency, frequency, importance, feedback), session context |
| 9 | Context Quality | Done | v0.4.0 | 5-component quality scoring, near-duplicate detection, conflict resolution, source trust |
| 10 | MCP Modernization | Done | v0.6.0 | MCP 2025-11-25 protocol, Resources, Prompts, tool annotations, HTTP transport, server modularization |
| 11 | Reactive Infrastructure | Done | v0.7.0 | SSE event streaming, agent registry, MCP dispatch benchmark |
| E | Memory Compression & Consolidation | Done | v0.11.0 | Semantic compression (30x), online synthesis, sleep-time consolidation, context compression |
| F | Agentic Memory Evolution | Done | v0.11.0 | Historical memory update, retrieval utility scoring, emotional & reflective memory |
| G | Advanced Graph Intelligence | Done | v0.11.0 | Conflict detection/resolution, Hebbian coactivation, semantic triplet matching |
| H | Autonomous Memory Agent | Done | v0.11.0 | Proactive acquisition, autonomous gardening, observe→decide→act agent loop |
| I | Retrieval Excellence | Done | v0.11.0 | Multi-provider embeddings, MMR diversity, semantic cache, neural reranking, explainability, feedback |
| J | Context Engineering | Done | v0.11.0 | Fact extraction, prompt construction, self-editing memory blocks |
| K | Temporal Graph & Platform Maturity | Done | v0.11.0 | Temporal knowledge graph, hierarchical scoping, benchmark suite |

All 19 phases complete. Published as v0.11.0.

---

## Phase Details

### Phase 0: Storage Abstraction (v0.1.0)

Foundation layer. Defined the `StorageBackend` trait (18 methods) so future backends (Turso, Meilisearch) can be swapped in without touching business logic.

- SQLite + WAL mode for crash recovery
- Connection pooling with read/write separation
- Hybrid search: BM25 (FTS5) + vector (sqlite-vec) + fuzzy (Levenshtein) + RRF fusion
- Knowledge graph with entity extraction, traversal, shortest-path
- Cloud sync (S3/R2) with AES-256 encryption
- MCP, REST, WebSocket, CLI interfaces
- 80+ MCP tools

### Phase 1: Cognitive Memory Types (v0.3.0)

Typed memory schema inspired by cognitive science.

- `Episodic`: event-based memories with temporal context
- `Procedural`: workflow/how-to patterns with success/failure tracking
- `Summary`: compressed knowledge from multiple memories
- `Checkpoint`: stable session state snapshots
- Schema v11: `event_time`, `event_duration_seconds`, `trigger_pattern`, `procedure_success_count`

### Phase 2: Context Compression (v0.3.0)

Keep context windows manageable without losing important information.

- `memory_summarize`: create summary from multiple memories
- `memory_soft_trim`: preserve head (60%) + tail (30%) with ellipsis
- `context_budget_check`: token counting via tiktoken-rs
- `memory_archive_old`: batch archive low-importance memories

### Phase 3: Langfuse Integration (v0.3.0)

Observability pipeline. Feature-gated behind `--features langfuse`.

- Connect to Langfuse API, sync traces to memories
- Extract patterns: successful prompts, errors, preferences, tool usage
- Async background sync with dedicated Tokio runtime

### Phase 4: Search Caching (v0.3.0)

Reduce latency for repeated/similar queries.

- Embedding-based similarity lookup (not just exact match)
- Adaptive threshold (0.85-0.98) tuned by quality feedback
- TTL expiration (5 min default), auto-invalidation on writes

### Phase 5: Memory Lifecycle (v0.3.0)

Automatic memory aging: Active -> Stale (30d) -> Archived (90d).

- `lifecycle_run`: trigger cycle with dry-run support
- Archived memories excluded from search/list by default
- Configurable thresholds per workspace

### Phase 6: Turso/libSQL (v0.3.0)

Distributed SQLite for edge deployments.

- Full `StorageBackend` implementation via libSQL
- Embedded replicas with sync to Turso cloud
- Feature-gated behind `--features turso`

### Phase 7: Meilisearch (v0.5.0)

Offload search to Meilisearch for larger deployments.

- Full `StorageBackend` implementation (1,285 lines, 33 tests)
- `MeilisearchIndexer`: background sync from SQLite (full + incremental)
- 4 MCP tools: `meilisearch_search`, `meilisearch_reindex`, `meilisearch_status`, `meilisearch_config`
- SQLite remains source of truth; Meilisearch is a read-optimized mirror
- Feature-gated behind `--features meilisearch`

### Phase 8: Salience Scoring (v0.4.0)

Dynamic memory prioritization based on multiple signals.

- Formula: `Salience = (Recency * 0.3) + (Frequency * 0.2) + (Importance * 0.3) + (Feedback * 0.2)`
- Temporal decay transitions memories through lifecycle states
- Session context: conversation-scoped memory tracking with relevance scoring
- 17 new MCP tools
- Schema v14: `salience_history`, `session_memories` tables

### Phase 9: Context Quality (v0.4.0)

Ensure memory quality doesn't degrade over time.

- Quality = (Clarity * 0.25) + (Completeness * 0.20) + (Freshness * 0.20) + (Consistency * 0.20) + (Source Trust * 0.15)
- Near-duplicate detection via character n-gram Jaccard similarity
- Conflict detection for contradictions between memories
- Source trust scoring by origin type
- 9 new MCP tools
- Schema v15: `quality_history`, `memory_conflicts`, `source_trust_scores`, `duplicate_candidates`

### Phase 10: MCP Modernization (v0.6.0)

Modernize to MCP 2025-11-25 spec with improved tooling and server architecture.

- **MCP Protocol Upgrade**: v2024-11-05 → v2025-11-25 with full backward compatibility
- **Tool Annotations**: All 155+ MCP tools classified with `readOnlyHint`, `destructiveHint`, `idempotentHint` per MCP spec
- **MCP Resources**: 5 resource URI templates exposing memory/workspace/stats as queryable resources
- **MCP Prompts**: 4 guided workflow prompts for common agent patterns (create-knowledge-base, daily-review, search-and-organize, seed-entity)
- **Streamable HTTP Transport**: Axum-based HTTP server with `--transport http|stdio|both` CLI flag, bearer token auth, CORS
- **Server Modularization**: Refactored 6200-line server.rs into 11 focused domain handler modules for maintainability

### Phase 11: Reactive Infrastructure (v0.7.0)

Real-time event delivery, multi-agent federation foundation, and performance benchmarking.

- **SSE Event Streaming**: `GET /v1/events` for real-time push notifications (memory CRUD, sync events)
- **Agent Registry**: Schema v17 with `agents` table — register, heartbeat, namespace isolation, capability tracking
- **Agent MCP Tools**: 6 new tools for agent lifecycle management (`agent_register/deregister/heartbeat/list/get/capabilities`)
- **MCP Dispatch Benchmark**: Criterion suite covering 5 tool dispatch paths (create, search, list, stats, error)
- **Benchmark Baseline Scripts**: `bench-baseline.sh` and `bench-compare.sh` for local performance tracking

### Phase E: Memory Compression & Consolidation (v0.11.0)

Cognitive-inspired memory compression and consolidation. Feature flag: `compression`.

- **Semantic Structured Compression**: SimpleMem-inspired 30x token reduction via filler removal, SVO extraction, dedup
- **Online Semantic Synthesis**: Intra-session dedup with Jaccard overlap detection
- **Sleep-time Consolidation**: LightMem-inspired offline batch consolidation
- **Active Context Compression**: Token-budget aware adaptive compression
- Schema v26: `compressed_content`, `compression_ratio`, `compression_method` columns
- 5 MCP tools: `memory_compress`, `memory_decompress`, `memory_compress_for_context`, `memory_consolidate`, `memory_synthesis`

### Phase F: Agentic Memory Evolution (v0.11.0)

Self-improving memory with utility scoring and emotional awareness. Feature flag: `agentic-evolution`.

- **Historical Memory Update**: A-Mem-inspired auto-update with contradiction/supplement detection
- **Retrieval Utility Scoring**: MemRL-inspired Q-value with `Q(m) = Q(m) + α * (reward - Q(m))` and temporal decay
- **Emotional & Reflective Memory**: Rule-based sentiment analysis + reflection engine
- Schema v27-28: `utility_score`, `utility_feedback`, `update_log`, `sentiment_score`, `sentiment_label`, `reflections` tables
- 5 MCP tools: `memory_detect_updates`, `memory_utility_score`, `memory_sentiment_analyze`, `memory_sentiment_timeline`, `memory_reflect`

### Phase G: Advanced Graph Intelligence (v0.11.0)

Advanced knowledge graph capabilities. Feature flag: `advanced-graph` (depends on `emergent-graph`).

- **Conflict Detection & Resolution**: Mem0g-inspired contradiction, cycle, and orphan detection
- **Temporal Coactivation / Hebbian Learning**: "Neurons that fire together wire together" edge strengthening
- **Semantic Triplet Matching**: SPARQL-like SPO pattern matching with transitive inference
- Schema v29: `coactivation_edges`, `graph_conflicts`, `knowledge_triplets` tables
- 5 MCP tools: `memory_detect_conflicts`, `memory_resolve_conflict`, `memory_coactivation_report`, `memory_query_triplets`, `memory_add_knowledge`

### Phase H: Autonomous Memory Agent (v0.11.0)

Full autonomous agent loop. Feature flag: `autonomous-agent` (depends on compression + agentic-evolution + advanced-graph).

- **Proactive Memory Acquisition**: Gap detection + interest tracking
- **Autonomous Pruning & Gardening**: 4-pass pipeline (dedup → compress → prune → link)
- **Memory Agent Loop**: Observe→decide→act tick-based agent
- Schema v30: `garden_log`, `query_log` tables
- New binary: `engram-agent`
- 10 MCP tools: `memory_agent_start/stop/status/metrics/configure`, `memory_garden/garden_preview/garden_undo`, `memory_suggest_acquisition`, `memory_proactive_scan`

### Phase I: Retrieval Excellence (v0.11.0)

State-of-the-art retrieval with multi-provider embeddings. Feature flags: `retrieval-excellence`, `ollama`, `cohere`, `voyage`, `onnx-embed`, `neural-rerank`.

- **Multi-Provider Embeddings**: EmbeddingProvider trait + registry (Ollama, Cohere, Voyage AI, ONNX)
- **MMR Diversity-Aware Retrieval**: Maximal Marginal Relevance for result diversity
- **Semantic Query Cache**: Cosine-similarity DashMap cache (threshold 0.92, TTL, LRU)
- **Cross-Encoder Neural Reranking**: ONNX Runtime ms-marco-MiniLM
- **Search Explainability**: Per-result scoring breakdown with signal contributions
- **Relevance Feedback Loop**: Useful/irrelevant signals with Laplace-smoothed boost
- Schema v20-21: `embedding_model` column, `search_feedback` table
- 7 MCP tools: `memory_cache_stats`, `memory_cache_clear`, `memory_embedding_providers`, `memory_embedding_migrate`, `memory_explain_search`, `memory_feedback`, `memory_feedback_stats`

### Phase J: Context Engineering (v0.11.0)

LLM context optimization pipeline. Feature flag: `context-engineering`.

- **Automatic Fact Extraction**: Rule-based SPO triple extraction (80% compression target)
- **Memory-Aware Prompt Construction**: 3 strategies (Greedy/Balanced/Recency) with token counting
- **Self-Editing Memory Blocks**: Letta-inspired 3-tier blocks (system/persona/human) with edit log
- Schema v22-23: `facts`, `memory_blocks`, `block_edit_log` tables
- 10 MCP tools: `memory_extract_facts`, `memory_list_facts`, `memory_fact_graph`, `memory_build_context`, `memory_prompt_template`, `memory_token_estimate`, `memory_block_get/edit/list/create`

### Phase K: Temporal Graph & Platform Maturity (v0.11.0)

Temporal knowledge graphs and hierarchical scoping. Feature flag: `temporal-graph`.

- **Temporal Knowledge Graph**: Zep/Graphiti-inspired edges with validity periods, auto-invalidation, snapshot-at-time
- **Hierarchical Memory Scoping**: 5-level scope: Global > Org > User > Session > Agent
- **Standardized Benchmark Suite**: LOCOMO, LongMemEval, MemBench frameworks
- Schema v24-25: `temporal_edges` table, `scope_path` column
- New binary: `engram-bench`
- 10 MCP tools: `memory_temporal_create/invalidate/snapshot/contradictions/evolve`, `memory_scope_set/get/list/inherit/isolate`

---

## What's Next

All 19 phases complete. Future directions under consideration:

- **OpenClaw integration**: Memory plugin for the 192K-star AI assistant platform
- **Multi-agent memory sharing**: Cross-agent memory federation with shared workspaces
- **Agent-to-agent messaging**: Event-driven communication via the agent registry
- **WASM target**: Run Engram in the browser
- **Advanced SSE**: Resumable streams with `Last-Event-Id`, event buffering

---

## Version History

| Version | Date | Phases | MCP Tools |
|---------|------|--------|-----------|
| v0.1.0 | 2025-01-23 | 0 | 80+ |
| v0.2.0 | 2026-01-28 | 0 | 110+ |
| v0.3.0 | 2026-01-30 | 0-6 | 130+ |
| v0.4.0 | 2026-02-12 | 0-6, 8-9 | 140+ |
| v0.4.1 | 2026-02-13 | 0-6, 8-9 | 140+ |
| v0.5.0 | 2026-02-13 | 0-9 (all) | 144+ |
| v0.6.0 | 2026-03-09 | 0-10 (all) | 155+ |
| v0.7.0 | 2026-03-09 | 0-11 (all) | 161+ |
| v0.8.1 | 2026-03-09 | 0-11, Round 1 | 161+ |
| v0.11.0 | 2026-03-09 | All (0-K) | 207+ |
