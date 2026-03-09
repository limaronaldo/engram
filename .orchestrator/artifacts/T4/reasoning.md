## Task: Implement MCP Resources for memories and workspaces

### Approach
Created `src/mcp/resources.rs` with `list_resources()` and `read_resource()` functions that implement all 5 URI patterns. Updated `src/mcp/mod.rs` to expose the new module and re-export the public functions. Updated `src/bin/server.rs` to import the new functions and add `resources/list` and `resources/read` match arms in `McpHandler::handle_request`.

The task spec mentioned a `HandlerContext` struct in `src/mcp/handlers/mod.rs` but this directory does not exist in the worktree — the handlers are all implemented directly as methods on `EngramHandler` in `server.rs`. The `read_resource` function therefore takes `&Storage` directly rather than a `HandlerContext`.

### Files Changed
- `src/mcp/resources.rs` — new module: `list_resources()`, `read_resource()`, and private helpers for each URI pattern
- `src/mcp/mod.rs` — added `pub mod resources;` and re-export of `list_resources`, `read_resource`, `ResourceTemplate`
- `src/bin/server.rs` — added imports for `list_resources`/`read_resource`; added `LIST_RESOURCES` and `READ_RESOURCE` match arms in `McpHandler::handle_request`

### Decisions Made
- `read_resource()` takes `&Storage` directly (not a `HandlerContext`) because the codebase has no `HandlerContext` abstraction — all handler state is on `EngramHandler`.
- Used simple string matching for URI routing as specified (no URI crate).
- Query string parsing (`?limit=N&offset=N`) is handled by a simple `split('&')` parser — sufficient for the two supported params.
- Default limit for `engram://workspace/{name}/memories` is 50 (matches the existing tool defaults).
- `engram://entities` returns top 100 entities by mention count — a reasonable cap without exposing unbounded data.
- `ResourceTemplate` is a plain struct (no derives beyond `Debug`/`Clone`) since it only needs to be iterated once per request and serialized in the server handler.
- The `LIST_RESOURCES` response uses `mimeType` (camelCase) per MCP spec; the `READ_RESOURCE` response wraps in `{"contents": [...]}` per MCP spec.

### Verification
- `cargo build`: success
- `cargo test`: 22 passed, 0 failed
- `cargo clippy -- -D warnings`: clean
