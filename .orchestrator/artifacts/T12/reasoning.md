## Task: T12 — Integration (Round 3 wiring)

### Approach

Wired all 11 Round 3 modules together by:
1. Adding feature flags and optional dependencies to Cargo.toml
2. Adding feature-gated module declarations to mod.rs files
3. Adding schema migrations v20-v24 to migrations.rs
4. Creating three MCP handler files and registering all new tools in the dispatch table

### Files Changed

- `Cargo.toml` — Added 9 new feature flags (`ollama`, `cohere`, `voyage`, `onnx-embed`, `neural-rerank`, `retrieval-excellence`, `context-engineering`, `temporal-graph`) and optional deps `ort = "2.0.0-rc.12"` and `ndarray = "0.16"`. Added new flags to `full` feature list.

- `src/embedding/mod.rs` — Added feature-gated `pub mod` declarations for `ollama`, `cohere`, `voyage`, `onnx` modules.

- `src/search/mod.rs` — Added `#[cfg(feature = "neural-rerank")] pub mod neural_rerank;`.

- `src/storage/migrations.rs` — Bumped `SCHEMA_VERSION` from 19 to 24. Added dispatch calls for v20-v24. Added migration functions `migrate_v20` through `migrate_v24` covering: embedding_model column, facts table, memory_blocks + block_edit_log, temporal_edges, scope_path column. Updated existing tests to assert v24.

- `src/mcp/handlers/retrieval.rs` — New file. Handlers for: `memory_cache_stats`, `memory_cache_clear`, `memory_embedding_providers`, `memory_embedding_migrate`.

- `src/mcp/handlers/context.rs` — New file. Handlers for: `memory_extract_facts`, `memory_list_facts`, `memory_fact_graph`, `memory_build_context`, `memory_block_get`, `memory_block_edit`, `memory_block_list`, `memory_block_create`, `memory_block_archive`, `memory_block_history`.

- `src/mcp/handlers/temporal.rs` — New file. Handlers for: `temporal_add_edge`, `temporal_snapshot`, `temporal_timeline`, `temporal_contradictions`, `temporal_diff`, `scope_set`, `scope_get`, `scope_list`, `scope_search`, `scope_tree`.

- `src/mcp/handlers/mod.rs` — Registered `pub mod context`, `pub mod retrieval`, `pub mod temporal`. Added 24 new dispatch entries for all new tools.

### Decisions Made

- Used `ort = "2.0.0-rc.12"` (not `"2"`) because the stable v2 is not yet published on crates.io — only RC releases are available.
- `scope_tree` handler was named `scope_tree_handler` to avoid shadowing the imported `crate::storage::scoping::scope_tree` function in the same function body.
- `memory_embedding_migrate` uses inline SQL rather than a missing `list_all_memory_ids` query helper that was referenced in the task spec but doesn't exist in the codebase.
- The failing test `test_upgrade_from_v17_to_v19` had a hardcoded assertion `assert_eq!(version, 19)` that was updated to v24.

### Verification

- Tests pass: yes (480 passed, 0 failed)
- Lint clean: yes (cargo clippy — no errors or unused-import warnings)
- Type check: yes (cargo build succeeds)
