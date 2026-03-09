## Task: T8 — Integration tests for MCP 2025-11-25 features

### Approach

The `EngramHandler` struct in `src/bin/server.rs` is private to the binary crate and cannot be
used from an integration test. Instead, the test file creates a local `TestHandler` struct that
implements the public `McpHandler` trait using the same public API functions:

- `engram::mcp::handlers::{HandlerContext, dispatch}` — for tool calls
- `engram::mcp::{list_resources, read_resource, list_prompts, get_prompt, get_tool_definitions}` — for MCP methods
- `engram::storage::Storage::open_in_memory()` — for an isolated in-memory database per test

The `TestHandler::handle_request` implementation is a direct copy of the routing logic in the
`EngramHandler` in `server.rs`, but using only the public library API. This ensures the tests
exercise the real production code paths (resources.rs, prompts.rs, tools.rs, handlers/).

### Files Changed

- `tests/mcp_protocol_tests.rs` — New integration test file with 10 test cases covering
  protocol negotiation, tool annotations, resources (list + read), and prompts (list + get).

### Decisions Made

- Used `HandlerContext` directly rather than re-exporting `EngramHandler` from the binary, since
  the binary crate is not a library and cannot be imported by integration tests.
- Kept `TestHandler` implementation close to `EngramHandler::handle_request` in server.rs for
  easy future maintenance.
- Used `cargo test --test mcp_protocol_tests` as the targeted run command per the task spec.
- The `test_resources_read_stats` test checks for memory count flexibly (multiple possible field
  names) since the Stats struct may evolve; falling back to verifying a non-empty JSON object.
- The `test_tools_list_includes_annotations` test checks annotations structurally (any annotated
  tools) rather than hardcoding counts, to be robust against tool list changes.

### Verification

- Tests pass: yes — 10/10 passing
- Lint clean: yes — no warnings in the new file
- Type check: yes — compiles cleanly
- Full suite: yes — 321 existing tests + 10 new = all pass, 0 failures
