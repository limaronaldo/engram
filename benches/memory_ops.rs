//! Performance benchmarks for memory operations (RML-902)

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use engram::storage::queries::*;
use engram::storage::Storage;
use engram::types::*;

fn bench_memory_create(c: &mut Criterion) {
    let storage = Storage::open_in_memory().unwrap();

    let mut group = c.benchmark_group("memory_create");
    group.throughput(Throughput::Elements(1));

    // Benchmark without embedding
    group.bench_function("no_embedding", |b| {
        b.iter(|| {
            storage
                .with_transaction(|conn| {
                    let input = CreateMemoryInput {
                        content: "Test content for benchmarking purposes".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec!["benchmark".to_string()],
                        metadata: Default::default(),
                        importance: Some(0.5),
                        defer_embedding: true,
                        scope: MemoryScope::Global,
                        ttl_seconds: None,
                        dedup_mode: DedupMode::Allow,
                        dedup_threshold: None,
                        workspace: Some("default".to_string()),
                        tier: MemoryTier::Permanent,
                    };
                    create_memory(conn, &input)
                })
                .unwrap()
        })
    });

    group.finish();
}

fn bench_memory_get(c: &mut Criterion) {
    let storage = Storage::open_in_memory().unwrap();

    // Create some memories first
    let mut ids = Vec::new();
    for i in 0..1000 {
        let memory = storage
            .with_transaction(|conn| {
                let input = CreateMemoryInput {
                    content: format!("Memory content number {}", i),
                    memory_type: MemoryType::Note,
                    tags: vec![format!("tag{}", i % 10)],
                    metadata: Default::default(),
                    importance: Some(0.5),
                    defer_embedding: true,
                    scope: MemoryScope::Global,
                    ttl_seconds: None,
                    dedup_mode: DedupMode::Allow,
                    dedup_threshold: None,
                    workspace: Some("default".to_string()),
                    tier: MemoryTier::Permanent,
                };
                create_memory(conn, &input)
            })
            .unwrap();
        ids.push(memory.id);
    }

    let mut group = c.benchmark_group("memory_get");
    group.throughput(Throughput::Elements(1));

    group.bench_function("by_id", |b| {
        let mut i = 0;
        b.iter(|| {
            let id = ids[i % ids.len()];
            i += 1;
            storage
                .with_connection(|conn| get_memory(conn, black_box(id)))
                .unwrap()
        })
    });

    group.finish();
}

fn bench_memory_list(c: &mut Criterion) {
    let storage = Storage::open_in_memory().unwrap();

    // Create memories with various tags
    for i in 0..1000 {
        storage
            .with_transaction(|conn| {
                let input = CreateMemoryInput {
                    content: format!(
                        "Memory content number {} with some longer text to simulate real usage",
                        i
                    ),
                    memory_type: if i % 3 == 0 {
                        MemoryType::Todo
                    } else {
                        MemoryType::Note
                    },
                    tags: vec![format!("tag{}", i % 10), format!("category{}", i % 5)],
                    metadata: Default::default(),
                    importance: Some((i % 10) as f32 / 10.0),
                    defer_embedding: true,
                    scope: MemoryScope::Global,
                    ttl_seconds: None,
                    dedup_mode: DedupMode::Allow,
                    dedup_threshold: None,
                    workspace: Some("default".to_string()),
                    tier: MemoryTier::Permanent,
                };
                create_memory(conn, &input)
            })
            .unwrap();
    }

    let mut group = c.benchmark_group("memory_list");

    for limit in [10, 50, 100].iter() {
        group.throughput(Throughput::Elements(*limit as u64));

        group.bench_with_input(BenchmarkId::new("limit", limit), limit, |b, &limit| {
            b.iter(|| {
                let options = ListOptions {
                    limit: Some(limit),
                    ..Default::default()
                };
                storage
                    .with_connection(|conn| list_memories(conn, black_box(&options)))
                    .unwrap()
            })
        });

        group.bench_with_input(
            BenchmarkId::new("with_tag_filter", limit),
            limit,
            |b, &limit| {
                b.iter(|| {
                    let options = ListOptions {
                        limit: Some(limit),
                        tags: Some(vec!["tag5".to_string()]),
                        ..Default::default()
                    };
                    storage
                        .with_connection(|conn| list_memories(conn, black_box(&options)))
                        .unwrap()
                })
            },
        );
    }

    group.finish();
}

fn bench_crossref_operations(c: &mut Criterion) {
    let storage = Storage::open_in_memory().unwrap();

    // Create memories
    let mut ids = Vec::new();
    for i in 0..100 {
        let memory = storage
            .with_transaction(|conn| {
                let input = CreateMemoryInput {
                    content: format!("Memory {}", i),
                    memory_type: MemoryType::Note,
                    tags: vec![],
                    metadata: Default::default(),
                    importance: None,
                    defer_embedding: true,
                    scope: MemoryScope::Global,
                    ttl_seconds: None,
                    dedup_mode: DedupMode::Allow,
                    dedup_threshold: None,
                    workspace: Some("default".to_string()),
                    tier: MemoryTier::Permanent,
                };
                create_memory(conn, &input)
            })
            .unwrap();
        ids.push(memory.id);
    }

    // Create some cross-references
    for i in 0..50 {
        storage
            .with_transaction(|conn| {
                let input = CreateCrossRefInput {
                    from_id: ids[i],
                    to_id: ids[i + 1],
                    edge_type: EdgeType::RelatedTo,
                    strength: None,
                    source_context: None,
                    pinned: false,
                };
                create_crossref(conn, &input)
            })
            .unwrap();
    }

    let mut group = c.benchmark_group("crossref");

    group.bench_function("create", |b| {
        let mut i = 60;
        b.iter(|| {
            let from = ids[i % 40];
            let to = ids[(i + 50) % 100];
            i += 1;

            storage
                .with_transaction(|conn| {
                    let input = CreateCrossRefInput {
                        from_id: from,
                        to_id: to,
                        edge_type: EdgeType::References,
                        strength: None,
                        source_context: None,
                        pinned: false,
                    };
                    create_crossref(conn, black_box(&input))
                })
                .unwrap()
        })
    });

    group.bench_function("get_related", |b| {
        let mut i = 0;
        b.iter(|| {
            let id = ids[i % 50];
            i += 1;
            storage
                .with_connection(|conn| get_related(conn, black_box(id)))
                .unwrap()
        })
    });

    group.finish();
}

fn bench_stats(c: &mut Criterion) {
    let storage = Storage::open_in_memory().unwrap();

    // Populate with data
    for i in 0..500 {
        storage
            .with_transaction(|conn| {
                let input = CreateMemoryInput {
                    content: format!("Memory {}", i),
                    memory_type: MemoryType::Note,
                    tags: vec![format!("tag{}", i % 20)],
                    metadata: Default::default(),
                    importance: None,
                    defer_embedding: true,
                    scope: MemoryScope::Global,
                    ttl_seconds: None,
                    dedup_mode: DedupMode::Allow,
                    dedup_threshold: None,
                    workspace: Some("default".to_string()),
                    tier: MemoryTier::Permanent,
                };
                create_memory(conn, &input)
            })
            .unwrap();
    }

    c.bench_function("get_stats", |b| {
        b.iter(|| storage.with_connection(get_stats).unwrap())
    });
}

criterion_group!(
    benches,
    bench_memory_create,
    bench_memory_get,
    bench_memory_list,
    bench_crossref_operations,
    bench_stats,
);

criterion_main!(benches);
