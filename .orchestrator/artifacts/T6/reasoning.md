## Task: T6 - Add Streamable HTTP transport for MCP

### Approach
Added an axum-based HTTP transport layer that reuses the existing `McpHandler` trait. The handler is wrapped in `Arc` so it can be shared between stdio and HTTP transports. A blanket `McpHandler` impl for `Arc<T>` enables this without changing existing code. Transport mode is selected via CLI args.

### Files Changed
- `src/mcp/http_transport.rs` -- New module: axum router with `POST /mcp` (JSON-RPC), `GET /health` (liveness probe), Bearer token auth, CORS via tower-http
- `src/mcp/mod.rs` -- Added `pub mod http_transport;`
- `src/mcp/protocol.rs` -- Added blanket `McpHandler` impl for `Arc<T>` to support shared ownership
- `src/bin/server.rs` -- Added `TransportMode` enum (Stdio/Http/Both), `--transport`, `--http-port`, `--http-api-key` CLI args, transport dispatch in `main()`

### Decisions Made
- Auth is done inline in the handler rather than as axum middleware layer, keeping the code simpler and avoiding extra type complexity
- `Arc<dyn McpHandler>` used for the HTTP state rather than generic `H: McpHandler` to keep axum routing simple
- Blanket impl `McpHandler for Arc<T>` added to protocol.rs so `McpServer::new(handler.clone())` works without changing `McpServer`
- HTTP mode creates its own tokio runtime since `main()` is synchronous; "Both" mode spawns HTTP in a background thread

### Verification
- Tests pass: yes (22 passed, 0 failed)
- Lint clean: yes (clippy -D warnings passes)
- Type check: yes (cargo build clean, no warnings)
