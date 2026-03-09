## Task: T8 — Graph Conflict Detection & Resolution (RML-1217)

### Approach

Created a single new file `src/graph/conflicts.rs` implementing the full conflict
detection and resolution pipeline, then registered it as a submodule in
`src/graph/mod.rs`. Followed the exact same patterns used in the adjacent
`temporal.rs` module (rusqlite, params!, Result alias, pub const DDL, unit tests
with in-memory SQLite).

### Files Changed

- `src/graph/conflicts.rs` — new file with all types, detectors, resolver, helpers,
  and 9 unit tests.
- `src/graph/mod.rs` — added `pub mod conflicts;` declaration.

### Decisions Made

- `ConflictType::from_str` / `Severity::from_str` / `ResolutionStrategy::from_str`
  implemented as associated fns rather than `TryFrom<&str>` to keep the pattern
  consistent with the rest of the codebase (no trait import needed at call sites).

- `detect_temporal_inconsistencies` queries the `cross_references` table directly
  via a self-join on `(from_id, to_id, relation_type)` — this is lighter than
  loading all edges into memory and grouping in Rust.

- Cycle detection uses iterative DFS with an explicit stack to avoid stack
  overflow on large graphs (no recursion).

- `KeepNewer` resolution sorts by `created_at` TEXT — valid because all timestamps
  are RFC3339 UTC (lexicographic sort == chronological sort, per project invariant).

- `resolve_merge` keeps the highest-strength edge and merges all other edges'
  metadata JSON into it via `entry().or_insert()` (first-write wins for
  conflicting keys, i.e. the keeper's values take precedence).

- `EdgeRow.created_at` is annotated `#[allow(dead_code)]` because it is fetched
  for SQL ordering purposes but not accessed in Rust code.

- Pre-existing compile errors in `src/intelligence/emotional.rs` are unrelated
  to this task and existed before this change (verified by stashing our changes
  and observing the same errors).

### Verification

- Tests pass: yes — 9/9 (cargo test graph::conflicts::tests)
- Lint clean: yes — cargo clippy --lib produces no warnings from our file
- Type check: yes — compiles cleanly modulo pre-existing emotional.rs errors
