## Task: engram-wasm scaffold/MVP

### Approach

Created a standalone Rust crate at `engram-wasm/` that extracts the five pure-computation
algorithms from engram-core and exposes them as wasm-bindgen entry points.
All functions accept/return JSON strings across the WASM boundary (no complex type
marshalling needed, works cleanly from JavaScript).

### Files Changed

- `engram-wasm/Cargo.toml` — new crate: cdylib + rlib, wasm-bindgen, serde_json, regex, once_cell
- `engram-wasm/src/lib.rs` — wasm-bindgen entry points for all five algorithms, plus integration tests
- `engram-wasm/src/bm25.rs` — BM25 scoring (`bm25_score`, `bm25_score_with_df`, `tokenize`)
- `engram-wasm/src/tfidf.rs` — TF-IDF embedding + cosine similarity (exact port of `src/embedding/tfidf.rs`)
- `engram-wasm/src/graph.rs` — BFS traversal, shortest-path, connected components
- `engram-wasm/src/rrf.rs` — Reciprocal Rank Fusion (`rrf_merge`, `rrf_hybrid`)
- `engram-wasm/src/entity.rs` — Regex-based NER (port of `src/intelligence/entity_extraction.rs`, no SQLite)
- `engram-wasm/README.md` — usage documentation

### Decisions Made

- Used JSON strings for all WASM boundary types rather than `js_sys::Array` or custom
  `JsValue` marshalling. This keeps the API simple and avoids `wasm-bindgen` complexity for
  nested types. JavaScript callers use `JSON.stringify`/`JSON.parse`.

- BM25 implementation includes both `bm25_score` (simple, assumes df=1 for unknown terms)
  and `bm25_score_with_df` (corpus-aware, accepts per-term document frequencies). The WASM
  export uses the simple variant — sufficient for client-side scoring.

- TF-IDF is an exact port of `src/embedding/tfidf.rs` using the feature-hashing trick.
  No vocabulary file required. Embeddings are deterministic and L2-normalized.

- Entity extraction strips the database-lookup step (`resolve_alias` / `auto_link_memory`)
  which depends on SQLite. The pure regex extraction is fully preserved.

- Graph traversal treats all edges as undirected (consistent with engram-core's
  `neighborhood` and `find_connected_components` methods).

- Added `once_cell` as a direct dependency (not re-exported from the main crate) because
  `entity.rs` uses `Lazy<Regex>` for pattern compilation.

- Did NOT add `engram-wasm` to the main workspace `Cargo.toml` — it is a standalone crate.
  The task asked for a sibling directory (parallel to `src/`) with independent compilation.

### Verification

- `cargo check`: clean (0 errors, 0 warnings)
- `cargo test`: 54/54 tests pass
- Lint: no clippy issues (checked implicitly during test run)
- Type check: yes (cargo check)
