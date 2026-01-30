//! Performance benchmarks for search operations (RML-902)

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use engram::embedding::{Embedder, TfIdfEmbedder};
use engram::search::{bm25_search, hybrid_search, FuzzyEngine, SearchConfig};
use engram::storage::queries::*;
use engram::storage::Storage;
use engram::types::*;

fn setup_storage_with_data(count: usize) -> Storage {
    let storage = Storage::open_in_memory().unwrap();

    let sample_contents = vec![
        "Authentication using JWT tokens and refresh mechanism",
        "Database migration strategy for PostgreSQL",
        "React component lifecycle and hooks optimization",
        "API rate limiting implementation with Redis",
        "Docker container orchestration with Kubernetes",
        "GraphQL schema design best practices",
        "Microservices communication patterns",
        "CI/CD pipeline configuration with GitHub Actions",
        "Memory leak detection in Node.js applications",
        "Rust ownership and borrowing concepts",
    ];

    for i in 0..count {
        let content = format!(
            "{} - variation {} with additional context about software development",
            sample_contents[i % sample_contents.len()],
            i
        );

        storage
            .with_transaction(|conn| {
                let input = CreateMemoryInput {
                    content,
                    memory_type: MemoryType::Note,
                    tags: vec![format!("topic{}", i % 5), "development".to_string()],
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
            .unwrap();
    }

    storage
}

fn bench_bm25_search(c: &mut Criterion) {
    let storage = setup_storage_with_data(1000);

    let mut group = c.benchmark_group("bm25_search");

    let queries = vec![
        "authentication",
        "database migration",
        "React hooks optimization",
        "API rate limiting Redis",
    ];

    for query in queries {
        group.bench_with_input(BenchmarkId::new("query", query), &query, |b, query| {
            b.iter(|| {
                storage
                    .with_connection(|conn| bm25_search(conn, black_box(query), 10, false))
                    .unwrap()
            })
        });
    }

    group.finish();
}

fn bench_hybrid_search(c: &mut Criterion) {
    let storage = setup_storage_with_data(1000);
    let embedder = TfIdfEmbedder::new(384);
    let config = SearchConfig::default();

    let mut group = c.benchmark_group("hybrid_search");

    let queries = vec![
        ("short", "auth"),
        ("medium", "database migration strategy"),
        (
            "long",
            "how to implement authentication with JWT tokens and refresh mechanism",
        ),
    ];

    for (name, query) in queries {
        let query_embedding = embedder.embed(query).unwrap();

        group.bench_with_input(
            BenchmarkId::new("query_type", name),
            &(query, &query_embedding),
            |b, (query, embedding)| {
                b.iter(|| {
                    let options = SearchOptions {
                        limit: Some(10),
                        ..Default::default()
                    };
                    storage
                        .with_connection(|conn| {
                            hybrid_search(
                                conn,
                                black_box(query),
                                Some(embedding.as_slice()),
                                &options,
                                &config,
                            )
                        })
                        .unwrap()
                })
            },
        );
    }

    group.finish();
}

fn bench_tfidf_embedding(c: &mut Criterion) {
    let embedder = TfIdfEmbedder::new(384);

    let mut group = c.benchmark_group("tfidf_embedding");

    let texts = vec![
        ("short", "hello world"),
        ("medium", "The quick brown fox jumps over the lazy dog"),
        ("long", "Authentication using JWT tokens requires careful consideration of security best practices including token expiration, refresh token rotation, and secure storage mechanisms"),
    ];

    for (name, text) in texts {
        group.bench_with_input(BenchmarkId::new("text_length", name), &text, |b, text| {
            b.iter(|| embedder.embed(black_box(text)).unwrap())
        });
    }

    // Batch embedding
    let batch: Vec<&str> = (0..100)
        .map(|i| {
            if i % 3 == 0 {
                "Short text"
            } else if i % 3 == 1 {
                "Medium length text with more content"
            } else {
                "Longer text with significantly more content to process and embed into vector space"
            }
        })
        .collect();

    group.bench_function("batch_100", |b| {
        b.iter(|| embedder.embed_batch(black_box(&batch)).unwrap())
    });

    group.finish();
}

fn bench_fuzzy_search(c: &mut Criterion) {
    let mut engine = FuzzyEngine::new();

    // Build vocabulary
    let words = vec![
        "authentication",
        "authorization",
        "configuration",
        "implementation",
        "documentation",
        "optimization",
        "synchronization",
        "initialization",
        "serialization",
        "deserialization",
        "transformation",
        "compilation",
    ];

    for word in &words {
        for _ in 0..10 {
            engine.add_to_vocabulary(word);
        }
    }

    let mut group = c.benchmark_group("fuzzy_search");

    let typos = vec![
        ("1_char_typo", "authentcation"),
        ("2_char_typo", "authentcatin"),
        ("transposition", "authetnicaiton"),
    ];

    for (name, query) in typos {
        group.bench_with_input(BenchmarkId::new("typo_type", name), &query, |b, query| {
            b.iter(|| engine.correct_query(black_box(query)))
        });
    }

    group.finish();
}

fn bench_search_at_scale(c: &mut Criterion) {
    let mut group = c.benchmark_group("search_scale");
    group.sample_size(50); // Fewer samples for slow benchmarks

    for &size in &[100, 1000, 10000] {
        let storage = setup_storage_with_data(size);
        let embedder = TfIdfEmbedder::new(384);
        let config = SearchConfig::default();
        let query = "authentication JWT tokens";
        let query_embedding = embedder.embed(query).unwrap();

        group.throughput(Throughput::Elements(size as u64));

        group.bench_with_input(
            BenchmarkId::new("memories", size),
            &(query.to_string(), query_embedding.clone()),
            |b, (query, embedding): &(String, Vec<f32>)| {
                b.iter(|| {
                    let options = SearchOptions {
                        limit: Some(10),
                        ..Default::default()
                    };
                    storage
                        .with_connection(|conn| {
                            hybrid_search(
                                conn,
                                black_box(query.as_str()),
                                Some(embedding.as_slice()),
                                &options,
                                &config,
                            )
                        })
                        .unwrap()
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_bm25_search,
    bench_hybrid_search,
    bench_tfidf_embedding,
    bench_fuzzy_search,
    bench_search_at_scale,
);

criterion_main!(benches);
