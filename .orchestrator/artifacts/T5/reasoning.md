## Task: Implement MCP Prompts for guided workflows

### Approach
Created `src/mcp/prompts.rs` with two public functions (`list_prompts` and `get_prompt`), wired them into `src/mcp/mod.rs` as re-exports, and replaced the stub handlers in `src/bin/server.rs` with real implementations.

### Files Changed
- `src/mcp/prompts.rs` — New file. Implements 4 guided workflow prompts (create-knowledge-base, daily-review, search-and-organize, seed-entity) with argument validation, template substitution, and 14 unit tests.
- `src/mcp/mod.rs` — Added `pub mod prompts;` and re-exported `list_prompts`/`get_prompt`.
- `src/bin/server.rs` — Added `get_prompt` and `list_prompts` to imports. Replaced `LIST_PROMPTS` stub (empty list) with `list_prompts()` call. Replaced `GET_PROMPT` stub (always error) with `get_prompt(name, &arguments)` dispatch that returns `{"messages": [...]}` on success or MCP error -32002 on unknown prompt / missing required argument.

### Decisions Made
- Used `Option<&str>` helper closure inside `get_prompt` to keep argument extraction concise without allocating.
- Required arguments return `Err(String)` which maps to MCP error code -32002 (same code used for "not found"), matching the existing resource error convention in server.rs.
- `daily-review` appends workspace context to the assistant message only when a workspace argument is provided, keeping the user message generic.

### Verification
- Tests pass: yes (14/14 in mcp::prompts)
- Lint clean: yes (cargo clippy -- -D warnings: no warnings)
- Build: yes (cargo build: 0 errors)
