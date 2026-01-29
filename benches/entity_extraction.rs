//! Performance benchmarks for entity extraction

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use engram::intelligence::entities::{EntityExtractionConfig, EntityExtractor};

fn bench_entity_extractor_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("entity_extractor_new");
    group.throughput(Throughput::Elements(1));

    group.bench_function("default", |b| {
        b.iter(|| EntityExtractor::new(EntityExtractionConfig::default()))
    });

    group.finish();
}

fn bench_entity_extraction(c: &mut Criterion) {
    let extractor = EntityExtractor::default();
    let text = "Mr. John Smith and Ms. Jane Doe are working at Anthropic on the Claude project. \
                They met yesterday at 2024-01-25 to discuss semantic search and vector databases. \
                You can find the code at https://github.com/engram/engram.";

    let mut group = c.benchmark_group("entity_extraction");
    group.throughput(Throughput::Bytes(text.len() as u64));

    group.bench_function("extract_mixed", |b| b.iter(|| extractor.extract(text)));

    group.finish();
}

criterion_group!(benches, bench_entity_extractor_new, bench_entity_extraction);

criterion_main!(benches);
