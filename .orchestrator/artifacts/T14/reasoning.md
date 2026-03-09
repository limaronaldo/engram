## Task: T14 — Integration task for engram-core Round 2

### Approach

Wired all 13 previously-committed modules (T1-T13) into the build by:
1. Adding feature flags to Cargo.toml and a new `[[bin]]` entry
2. Adding schema migrations v26-v30 to migrations.rs
3. Creating 3 new MCP handler files that bridge module APIs to JSON-RPC
4. Updating mod.rs with module declarations + 22 new dispatch entries
5. Creating the `engram-agent` binary

### Files Changed

- `Cargo.toml` — Added `compression`, `agentic-evolution`, `advanced-graph`, `autonomous-agent` feature flags; added all four to `full`; added `[[bin]] engram-agent` entry
- `src/storage/migrations.rs` — Bumped `SCHEMA_VERSION` from 25 to 30; added `migrate_v26` through `migrate_v30` functions; added dispatch calls in `run_migrations`; updated all test assertions from 25 to 30
- `src/mcp/handlers/compression.rs` — New file: 5 handlers using `SemanticCompressor`, `ContextCompressor::compress_for_context`, `OfflineConsolidator::consolidate_with_strategy`, `SynthesisEngine`
- `src/mcp/handlers/evolution.rs` — New file: 5 handlers using `UpdateDetector`, `UtilityTracker`, `SentimentAnalyzer`, `ReflectionEngine`
- `src/mcp/handlers/autonomous.rs` — New file: 12 handlers using `ConflictDetector`, `ConflictResolver`, `CoactivationTracker`, `TripletMatcher`, `GapDetector`, `MemoryGardener`, `MemoryAgent`
- `src/mcp/handlers/mod.rs` — Added 3 module declarations + 22 new dispatch match arms
- `src/bin/agent.rs` — New binary with `run`, `status`, `garden`, `suggest` subcommands

### Decisions Made

- Used actual module APIs after reading each module source (not invented wrapper APIs)
- `get_memory` returns `Result<Memory>` not `Result<Option<Memory>>` — adjusted handlers accordingly
- `ContextCompressor::compress_for_context` is a static method — no mutable state needed
- `OfflineConsolidator` uses `consolidate_with_strategy` not `run`
- `memory_agent_start/stop` return informational JSON since `MemoryAgent` is tick-based (not a background thread); `memory_agent_metrics` runs one actual tick
- Pre-existing doctest failure in `synthesis.rs` (wrong import path in doc comment) is not introduced by this task

### Verification

- `cargo build`: passes (clean)
- `cargo build --bin engram-agent`: passes
- `cargo test --lib`: 672 passed, 0 failed
- `cargo clippy`: no errors
- `cargo test` (all): 672 lib tests pass; 1 pre-existing doctest failure in `synthesis.rs` (line 115 import path bug, not introduced by this task)
