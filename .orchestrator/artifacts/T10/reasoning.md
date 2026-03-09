## Task: T10 — Semantic Triplet Matching (RML-1219)

### Approach

Created `src/graph/triplets.rs` with `TripletMatcher` — a static struct with four methods that
operate directly on a `rusqlite::Connection` against the `facts` table from Round 3 (migration v21).

Used `crate::intelligence::fact_extraction::Fact` directly — no local duplicate needed since the
module is already pub and the type is compatible.

### Files Changed

- `src/graph/triplets.rs` — new file implementing `TripletPattern`, `InferenceStep`,
  `InferencePath`, `KnowledgeStats`, and `TripletMatcher` with all four query methods + 16 tests.
- `src/graph/mod.rs` — added `pub mod triplets;` declaration.

### Decisions Made

- **Used `Fact` from `fact_extraction`** rather than a local compatible struct — avoids duplication
  and ensures type compatibility with callers that already hold `Fact` values.
- **Dynamic SQL for `match_pattern`** — builds WHERE clause at runtime based on which pattern
  fields are `Some`. Uses explicit positional params (1-3) instead of rusqlite::params_from_iter
  to stay on safe ground with the bundled rusqlite API.
- **BFS for transitive inference** — clear, predictable semantics; cycle detection via a visited
  set built from the current path. Branches are explored independently so all reachable paths are
  returned, not just the longest.
- **Entity extraction via capital-letter heuristic** — simple, no regex dependency, mirrors what
  the spec describes ("extract capitalized words").
- **HashSet dedup in query_knowledge** — prevents duplicate facts when two entities both
  appear in the same fact.

### Verification

- Tests pass: yes (16/16)
- Lint clean: yes (cargo clippy -- -D warnings produces no warnings in triplets.rs)
- Type check: yes (part of successful cargo build)
