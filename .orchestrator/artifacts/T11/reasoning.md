## Task: T11 — Add recent_activity MCP tool

### TDD Cycles

1. RED: `test_recent_activity_tool_is_defined_as_read_only` → FAIL (tool not in TOOL_DEFINITIONS)
   GREEN: Added `ToolDef` to `tools.rs` TOOL_DEFINITIONS array with `ToolAnnotations::read_only()`
   REFACTOR: No changes needed — definition was clean.

2. RED: `test_recent_activity_returns_activities_field` → FAIL (SQL error: no such column `tags` in memories table)
   GREEN: Fixed SQL to use subquery for tags via memory_tags join; corrected column aliases and indices
   REFACTOR: Extracted char count computation to avoid double iteration over content chars.

3. RED: `test_recent_activity_timeframe_1h`, `test_recent_activity_limit_enforced`,
   `test_recent_activity_preview_truncated_at_100_chars` → all FAIL (same SQL root cause)
   GREEN: All passed after the SQL fix.
   REFACTOR: Char count optimization applied (single iteration).

### Approach

The handler uses dynamic SQL building to support optional workspace/type filters and a
parameterized limit. Tags are fetched via a correlated subquery (GROUP_CONCAT from memory_tags
JOIN tags) matching the pattern used elsewhere in queries.rs. Parameters use
`Box<dyn rusqlite::ToSql>` with `params_from_iter` — the established pattern in the codebase.

The time filter uses SQLite's `datetime('now', '-N unit')` expression baked into the SQL string
(not parameterized) because the timeframe values come from a validated enum match, making
SQL injection impossible.

### Files Changed

- `src/mcp/handlers/search.rs` — Added `recent_activity` handler function (119 lines) before `memory_expand`
- `src/mcp/tools.rs` — Added `ToolDef` for `recent_activity` at the end of TOOL_DEFINITIONS; added unit test `test_recent_activity_tool_is_defined_as_read_only`
- `src/mcp/handlers/mod.rs` — Added dispatch route `"recent_activity" => search::recent_activity(ctx, params)` in Search section
- `tests/mcp_protocol_tests.rs` — Added 4 integration tests covering: activity field presence, timeframe echo, limit enforcement, preview truncation at 100 chars

### Decisions Made

- Used `Box<dyn rusqlite::ToSql>` pattern over `rusqlite::types::Value` to match existing codebase convention in queries.rs
- Included `m.valid_to IS NULL` filter to exclude soft-deleted memories, consistent with other memory queries
- Tags fetched via correlated subquery (not a JOIN) to avoid row multiplication for multi-tag memories
- Preview truncation at 100 chars (character count, not byte count) to handle Unicode safely
- `timeframe` string baked into SQL string (not parameterized) — safe by construction from validated match

### Verification

- Tests pass: yes (5 new tests — 1 unit + 4 integration)
- Lint clean: yes (cargo clippy — no warnings)
- Build clean: yes (cargo build succeeds)
- TDD cycles completed: 3
- Total test suite: 736 unit tests + 14 MCP protocol tests — all passing
