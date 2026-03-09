## Task: T12 — Autonomous Pruning & Gardening (RML-1222)

### Approach

Created a single new file `src/intelligence/gardening.rs` implementing the full
garden maintenance pipeline: score → prune → merge → archive → compress.
The scoring formula follows the spec exactly (importance * recency_factor * access_factor).
All operations are idempotent and safe to re-run.

### Files Changed

- `src/intelligence/gardening.rs` — NEW. Full implementation with types, DDL, engine, and 10 tests.
- `src/intelligence/mod.rs` — Added `pub mod gardening` and `pub use gardening::{...}` exports.

### Decisions Made

- Chose Jaccard over cosine for merge similarity: it needs no embeddings, runs in-process, and is
  interpretable. Matches the existing `context_quality` module pattern.
- `merge_content` deduplicates sentences rather than naively concatenating to avoid doubled text.
- `memories_merged` in the report counts pairs (not individual IDs) to match intuitive semantics.
- `garden_undo` restores archived memories to `memory_type = 'note'`; it cannot restore pruned
  memories since those rows are deleted.
- `#[allow(dead_code)]` on `MemoryRow.created_at` — field is fetched to allow future use without
  breaking the SELECT column alignment.

### Verification

- Tests pass: yes (10/10)
- Lint clean: yes (cargo clippy -D warnings — no errors in our file)
- Type check: yes (cargo build succeeds)
