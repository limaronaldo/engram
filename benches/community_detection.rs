use criterion::{black_box, criterion_group, criterion_main, Criterion};
use engram::graph::{GraphEdge, GraphNode, KnowledgeGraph};
use engram::types::MemoryId;
use rand::prelude::*;

fn generate_graph(node_count: usize, edge_density: f32) -> KnowledgeGraph {
    let mut rng = StdRng::seed_from_u64(42);
    let mut nodes = Vec::with_capacity(node_count);

    let memory_types = vec![
        "note",
        "todo",
        "issue",
        "decision",
        "preference",
        "learning",
    ];
    let tag_pool = vec![
        "rust",
        "python",
        "ai",
        "database",
        "web",
        "api",
        "cli",
        "graph",
        "performance",
    ];

    for i in 0..node_count {
        let memory_type = memory_types.choose(&mut rng).unwrap().to_string();
        let num_tags = rng.gen_range(20..50);
        let mut tags = Vec::new();
        for _ in 0..num_tags {
            tags.push(tag_pool.choose(&mut rng).unwrap().to_string());
        }

        nodes.push(GraphNode {
            id: i as MemoryId,
            label: format!("Node {}", i),
            memory_type,
            importance: rng.gen(),
            tags,
        });
    }

    let mut edges = Vec::new();
    let edge_types = vec!["related_to", "depends_on", "part_of", "contradicts"];

    // Ensure some structure by creating clusters
    let cluster_size = 50;
    for i in 0..node_count {
        // Connect to neighbors in same "cluster"
        let cluster_start = (i / cluster_size) * cluster_size;
        let num_edges = rng.gen_range(2..6);

        for _ in 0..num_edges {
            let target =
                rng.gen_range(cluster_start..(cluster_start + cluster_size).min(node_count));
            if target != i {
                edges.push(GraphEdge {
                    from: i as MemoryId,
                    to: target as MemoryId,
                    edge_type: edge_types.choose(&mut rng).unwrap().to_string(),
                    score: rng.gen(),
                    confidence: rng.gen(),
                });
            }
        }

        // Occasional random link
        if rng.gen::<f32>() < edge_density {
            let target = rng.gen_range(0..node_count);
            if target != i {
                edges.push(GraphEdge {
                    from: i as MemoryId,
                    to: target as MemoryId,
                    edge_type: edge_types.choose(&mut rng).unwrap().to_string(),
                    score: rng.gen(),
                    confidence: rng.gen(),
                });
            }
        }
    }

    KnowledgeGraph { nodes, edges }
}

fn bench_detect_communities(c: &mut Criterion) {
    let graph = generate_graph(500, 0.05);

    let mut group = c.benchmark_group("community_detection");
    group.sample_size(10);
    group.bench_function("detect_communities_500_nodes", |b| {
        b.iter(|| graph.detect_communities(black_box(10)))
    });
    group.finish();
}

criterion_group!(benches, bench_detect_communities);
criterion_main!(benches);
