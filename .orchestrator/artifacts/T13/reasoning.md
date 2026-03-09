## Task: T13 — Standardized Benchmark Suite

### Approach

Implemented a benchmark framework and three benchmark modules, plus a CLI binary. Used `Storage::open_in_memory()` instead of direct `rusqlite::Connection` to stay within the public API (the `migrations` module is private). Each benchmark generates synthetic data at runtime and cleans up temporary files after running.

### Files Changed

- `src/bench/mod.rs` — Core framework: `BenchmarkResult`, `Benchmark` trait, `BenchmarkSuite` with JSON/Markdown/CSV reporting, `default_suite()` factory. 5 tests.
- `src/bench/locomo.rs` — LOCOMO benchmark: synthetic multi-session conversations, keyword-based retrieval, precision/recall/F1. 3 tests.
- `src/bench/longmemeval.rs` — LongMemEval benchmark: 5 dimensions (retention, temporal, knowledge update, multi-hop, contradiction detection) with configurable weights. 4 tests.
- `src/bench/membench.rs` — MemBench: CRUD throughput (create/get/search per-sec) and search quality (NDCG@10, MRR) with a synthetic corpus. 5 tests.
- `src/lib.rs` — Added `pub mod bench;` registration.
- `Cargo.toml` — Added `[[bin]] name = "engram-bench" path = "src/bin/bench.rs"`.
- `src/bin/bench.rs` — CLI binary with `run` and `list` subcommands, supports suite selection, output format (json/md/csv), and size overrides.

### Decisions Made

- Used `Storage::open_in_memory()` over raw `rusqlite::Connection` because the `migrations` module is private — this keeps the benchmark code within the public API.
- Used file-based storage with a `.{benchmark}_bench.db` suffix and cleanup for non-memory paths, to isolate benchmark data from production databases.
- `UpdateMemoryInput` fields set explicitly (no `Default` derive on it) — constructed all `None` fields inline.
- NDCG and MRR computed purely in Rust without external dependencies.
- Benchmarks use simple `LIKE` queries rather than hybrid search to keep them dependency-free and deterministic.

### Verification

- Tests pass: yes (19/19)
- Lint clean: yes (0 warnings after fixes)
- Type check: yes (builds cleanly)
- Binary smoke tests: `engram-bench list` and `engram-bench run --suite membench` both work correctly
