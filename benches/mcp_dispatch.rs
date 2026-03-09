//! Performance benchmarks for MCP dispatch latency (v0.7.0 - T1)
//!
//! Measures end-to-end dispatch latency for common tool calls via the
//! HandlerContext and dispatch() function. This benchmark simulates real
//! MCP request handling without network overhead.
//!
//! Run with: cargo bench --bench mcp_dispatch

use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use parking_lot::Mutex;
use serde_json::json;

use engram::embedding::{create_embedder, EmbeddingCache};
use engram::mcp::handlers::{self, HandlerContext};
use engram::search::{AdaptiveCacheConfig, FuzzyEngine, SearchConfig, SearchResultCache};
use engram::storage::queries::*;
use engram::storage::Storage;
use engram::types::*;

// ---------------------------------------------------------------------------
// Benchmark Handler Setup
// ---------------------------------------------------------------------------

/// Create a configured HandlerContext for benchmarking.
fn create_benchmark_context(storage: Storage) -> HandlerContext {
    let embedder = create_embedder(&EmbeddingConfig::default()).expect("tfidf embedder");
    HandlerContext {
        storage,
        embedder,
        fuzzy_engine: Arc::new(Mutex::new(FuzzyEngine::new())),
        search_config: SearchConfig::default(),
        realtime: None,
        embedding_cache: Arc::new(EmbeddingCache::default()),
        search_cache: Arc::new(SearchResultCache::new(AdaptiveCacheConfig::default())),
        #[cfg(feature = "meilisearch")]
        meili: None,
        #[cfg(feature = "meilisearch")]
        meili_indexer: None,
        #[cfg(feature = "meilisearch")]
        meili_sync_interval: 60,
        #[cfg(feature = "langfuse")]
        langfuse_runtime: Arc::new(tokio::runtime::Runtime::new().expect("langfuse runtime")),
    }
}

/// Populate storage with ~100 memories for search/list benchmarks.
fn seed_memories(storage: &Storage, count: usize) {
    for i in 0..count {
        storage
            .with_transaction(|conn| {
                let input = CreateMemoryInput {
                    content: format!(
                        "Benchmark memory #{} - synthetic content for dispatch latency testing",
                        i
                    ),
                    memory_type: if i % 3 == 0 {
                        MemoryType::Todo
                    } else {
                        MemoryType::Note
                    },
                    tags: vec![
                        format!("tag{}", i % 10),
                        format!("category{}", i % 5),
                    ],
                    metadata: Default::default(),
                    importance: Some((i % 10) as f32 / 10.0),
                    defer_embedding: true,
                    scope: MemoryScope::Global,
                    ttl_seconds: None,
                    dedup_mode: DedupMode::Allow,
                    dedup_threshold: None,
                    workspace: Some("default".to_string()),
                    tier: MemoryTier::Permanent,
                    event_time: None,
                    event_duration_seconds: None,
                    trigger_pattern: None,
                    summary_of_id: None,
                };
                create_memory(conn, &input)
            })
            .expect("seed memory");
    }
}

// ---------------------------------------------------------------------------
// Benchmark: memory_create (write path)
// ---------------------------------------------------------------------------

fn bench_dispatch_memory_create(c: &mut Criterion) {
    let storage = Storage::open_in_memory().expect("in-memory storage");
    let ctx = create_benchmark_context(storage);

    let mut group = c.benchmark_group("mcp_dispatch_memory_create");
    group.throughput(Throughput::Elements(1));

    group.bench_function("memory_create", |b| {
        b.iter(|| {
            let params = black_box(json!({
                "content": "Dispatch benchmark memory",
                "type": "note",
                "tags": ["benchmark"],
                "workspace": "default",
            }));
            handlers::dispatch(&ctx, "memory_create", params)
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: memory_search (read + compute path)
// ---------------------------------------------------------------------------

fn bench_dispatch_memory_search(c: &mut Criterion) {
    let storage = Storage::open_in_memory().expect("in-memory storage");
    seed_memories(&storage, 100);

    let ctx = create_benchmark_context(storage);

    let mut group = c.benchmark_group("mcp_dispatch_memory_search");
    group.throughput(Throughput::Elements(1));

    group.bench_function("memory_search", |b| {
        b.iter(|| {
            let params = black_box(json!({
                "query": "benchmark memory",
                "limit": 10,
                "workspace": "default",
            }));
            handlers::dispatch(&ctx, "memory_search", params)
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: memory_list (read path)
// ---------------------------------------------------------------------------

fn bench_dispatch_memory_list(c: &mut Criterion) {
    let storage = Storage::open_in_memory().expect("in-memory storage");
    seed_memories(&storage, 100);

    let ctx = create_benchmark_context(storage);

    let mut group = c.benchmark_group("mcp_dispatch_memory_list");
    group.throughput(Throughput::Elements(1));

    group.bench_function("memory_list", |b| {
        b.iter(|| {
            let params = black_box(json!({
                "limit": 20,
                "workspace": "default",
            }));
            handlers::dispatch(&ctx, "memory_list", params)
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: memory_stats (metadata path)
// ---------------------------------------------------------------------------

fn bench_dispatch_memory_stats(c: &mut Criterion) {
    let storage = Storage::open_in_memory().expect("in-memory storage");
    seed_memories(&storage, 100);

    let ctx = create_benchmark_context(storage);

    let mut group = c.benchmark_group("mcp_dispatch_memory_stats");
    group.throughput(Throughput::Elements(1));

    group.bench_function("memory_stats", |b| {
        b.iter(|| {
            let params = black_box(json!({}));
            handlers::dispatch(&ctx, "memory_stats", params)
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark: unknown tool (error path)
// ---------------------------------------------------------------------------

fn bench_dispatch_unknown_tool(c: &mut Criterion) {
    let storage = Storage::open_in_memory().expect("in-memory storage");
    let ctx = create_benchmark_context(storage);

    let mut group = c.benchmark_group("mcp_dispatch_error_path");
    group.throughput(Throughput::Elements(1));

    group.bench_function("unknown_tool", |b| {
        b.iter(|| {
            let params = black_box(json!({}));
            handlers::dispatch(&ctx, "unknown_tool_12345", params)
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Criterion Setup
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_dispatch_memory_create,
    bench_dispatch_memory_search,
    bench_dispatch_memory_list,
    bench_dispatch_memory_stats,
    bench_dispatch_unknown_tool,
);

criterion_main!(benches);
