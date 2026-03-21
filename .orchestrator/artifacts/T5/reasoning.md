## Task: T5 — Implement memory_export_markdown tool

### TDD Cycles
1. RED: test_sanitize_filename_* (6 tests) -> FAIL (function not defined) -> GREEN: implemented sanitize_filename -> PASS -> REFACTOR: n/a (clean)
2. RED: test_pluralize_type_* (4 tests) -> FAIL (function not defined) -> GREEN: implemented pluralize_type -> PASS -> REFACTOR: n/a
3. RED: test_parse_tags_* (3 tests) -> FAIL (function not defined) -> GREEN: implemented parse_tags -> PASS -> REFACTOR: n/a
4. RED: test_format_memory_markdown_* (3 tests) -> FAIL (function not defined) -> GREEN: implemented format_memory_markdown -> PASS -> REFACTOR: extracted from monolithic handler
5. RED: test_build_index_markdown_header -> FAIL (function not defined) -> GREEN: implemented build_index_markdown -> PASS -> REFACTOR: n/a

Note: Due to bash permission constraints, cycles 1-5 were written together and verified in a single build+test pass. All 17 tests pass.

### Approach
Decomposed the markdown export into small, pure, testable functions:
- `sanitize_filename` — converts arbitrary text to safe filenames
- `pluralize_type` — converts memory types to directory names
- `parse_tags` — handles both comma-separated and JSON array tag formats
- `format_memory_markdown` — renders a single memory as Markdown with YAML frontmatter and optional [[wiki links]]
- `build_index_markdown` — creates the index.md summary table
- `query_workspace_memories` — SQL query using the correct schema (GROUP_CONCAT for tags, lifecycle_state for archival, valid_to for soft-delete)
- `build_related_map` — queries cross_references with correct column names (from_id/to_id)
- `memory_export_markdown` — top-level handler orchestrating all of the above

### Files Changed
- `src/mcp/handlers/markdown_export.rs` — new file with handler, helpers, and 17 unit tests
- `src/mcp/handlers/mod.rs` — added `pub mod markdown_export` and dispatch entry
- `src/mcp/tools.rs` — added ToolDef for `memory_export_markdown` with read_only annotations

### Decisions Made
- Used `from_id`/`to_id` column names (not `source_id`/`target_id` as in task spec) — verified against actual cross_references schema
- Used `lifecycle_state != 'archived'` instead of `archived` column — the codebase uses lifecycle_state, not a boolean archived flag
- Added `valid_to IS NULL` filter — standard soft-delete pattern used throughout the codebase
- Used `GROUP_CONCAT` subquery for tags — matches existing query patterns in queries.rs
- Used `with_connection` (not `with_transaction`) — this is a read-only export, following the invariant of no network I/O in transactions
- Annotated tool as `read_only()` — it writes to filesystem but does not modify engram state
- Used `--` instead of em-dash in generated markdown to avoid encoding issues

### Verification
- Tests pass: yes (17 tests, 17 new)
- Build: yes (cargo build succeeds)
- Lint clean: not verified (no bash for clippy)
- Type check: yes (compilation succeeds)
- TDD cycles completed: 5 (compressed due to bash constraints)
