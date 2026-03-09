//! MemBench — CRUD throughput and search quality benchmark
//!
//! Measures:
//! - create_per_sec: How many memories can be created per second
//! - get_per_sec: How many memories can be retrieved by ID per second
//! - search_per_sec: How many keyword searches can run per second
//! - ndcg_at_10: Normalized Discounted Cumulative Gain@10 for search quality
//! - mrr: Mean Reciprocal Rank for search quality

use std::collections::HashMap;
use std::time::Instant;

use super::{Benchmark, BenchmarkResult};
use crate::storage::queries::{create_memory, get_memory};
use crate::storage::Storage;
use crate::types::{CreateMemoryInput, MemoryType, StorageConfig, StorageMode};

/// MemBench configuration
pub struct MemBenchmark {
    /// Number of memories to create during throughput test
    pub num_memories: usize,
    /// Number of search queries to run during quality test
    pub num_queries: usize,
}

/// A synthetic topic with keywords for relevance judgments
struct SyntheticTopic {
    keyword: &'static str,
    relevant_phrases: &'static [&'static str],
}

const TOPICS: &[SyntheticTopic] = &[
    SyntheticTopic {
        keyword: "machine learning",
        relevant_phrases: &[
            "machine learning model architecture",
            "deep learning neural network training",
            "gradient descent optimizer convergence",
            "training loss accuracy metrics",
        ],
    },
    SyntheticTopic {
        keyword: "database",
        relevant_phrases: &[
            "SQL query optimization plan",
            "database index scan performance",
            "transaction isolation level committed",
            "PostgreSQL connection pool management",
        ],
    },
    SyntheticTopic {
        keyword: "security",
        relevant_phrases: &[
            "authentication token JWT verification",
            "SQL injection vulnerability prevention",
            "HTTPS TLS certificate renewal",
            "password hashing bcrypt salt",
        ],
    },
    SyntheticTopic {
        keyword: "performance",
        relevant_phrases: &[
            "latency p99 benchmark test results",
            "throughput requests per second measurement",
            "memory allocation profiling heap",
            "cache hit ratio optimization",
        ],
    },
];

/// Corpus of memories to create, mixing relevant and irrelevant content
const CORPUS_TEMPLATES: &[&str] = &[
    "machine learning model trained on {} dataset with Adam optimizer",
    "deep learning neural network achieved 95% accuracy on image classification",
    "gradient descent optimizer converged after 1000 epochs of training",
    "SQL query optimization reduced latency by 40% after index tuning",
    "database index scan improved search performance significantly",
    "transaction isolation level set to READ COMMITTED for consistency",
    "authentication token JWT expires after 1 hour session timeout",
    "HTTPS TLS certificate renewed for production domain hosting",
    "latency p99 benchmark shows 12ms for 100k RPS under load",
    "memory allocation profiling revealed 200MB footprint in production",
    "unrelated fact about cooking: pasta needs salted boiling water",
    "weather today is sunny with 25 degrees Celsius temperature",
    "team meeting scheduled for next Tuesday at 2pm in conference room",
    "coffee machine on floor 3 needs maintenance and refill",
    "quarterly report submitted to finance department for review",
    "new joiner onboarding checklist completed successfully",
    "vacation request approved for two weeks in August holidays",
    "parking permit renewed for building B underground garage",
    "printer on floor 2 is out of paper and toner cartridge",
    "lunch order: 5 sandwiches and 3 salads for the engineering team",
];

impl MemBenchmark {
    /// Compute NDCG@k given ordered retrieved IDs and a set of relevant IDs
    pub fn ndcg_at_k(retrieved: &[i64], relevant_ids: &[i64], k: usize) -> f64 {
        let top_k: Vec<_> = retrieved.iter().take(k).collect();

        // DCG
        let dcg: f64 = top_k
            .iter()
            .enumerate()
            .map(|(i, &&id)| {
                let rel = if relevant_ids.contains(&id) { 1.0 } else { 0.0 };
                rel / (i as f64 + 2.0).log2()
            })
            .sum();

        // Ideal DCG: assume all relevant docs at the top
        let num_relevant = relevant_ids.len().min(k);
        let idcg: f64 = (0..num_relevant)
            .map(|i| 1.0 / (i as f64 + 2.0).log2())
            .sum();

        if idcg == 0.0 {
            0.0
        } else {
            dcg / idcg
        }
    }

    /// Compute MRR given ordered retrieved IDs and a set of relevant IDs
    pub fn mrr(retrieved: &[i64], relevant_ids: &[i64]) -> f64 {
        for (i, &id) in retrieved.iter().enumerate() {
            if relevant_ids.contains(&id) {
                return 1.0 / (i as f64 + 1.0);
            }
        }
        0.0
    }
}

impl Benchmark for MemBenchmark {
    fn name(&self) -> &str {
        "membench"
    }

    fn description(&self) -> &str {
        "CRUD throughput and search quality benchmark. Measures create_per_sec, get_per_sec, \
         search_per_sec, NDCG@10, and MRR using synthetic memories."
    }

    fn run(&self, db_path: &str) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
        let storage = if db_path == ":memory:" {
            Storage::open_in_memory()?
        } else {
            let bench_path = format!("{}.membench.db", db_path);
            Storage::open(StorageConfig {
                db_path: bench_path,
                storage_mode: StorageMode::Local,
                cloud_uri: None,
                encrypt_cloud: false,
                confidence_half_life_days: 30.0,
                auto_sync: false,
                sync_debounce_ms: 5000,
            })?
        };

        // ===== Phase 1: CREATE throughput =====
        let create_start = Instant::now();
        let mut created_ids: Vec<i64> = Vec::with_capacity(self.num_memories);

        for i in 0..self.num_memories {
            let template = CORPUS_TEMPLATES[i % CORPUS_TEMPLATES.len()];
            let content = template.replace("{}", &format!("batch_{}", i));
            let mem = storage.with_connection(|conn| {
                create_memory(
                    conn,
                    &CreateMemoryInput {
                        content,
                        memory_type: MemoryType::Note,
                        workspace: Some("membench".to_string()),
                        ..Default::default()
                    },
                )
            })?;
            created_ids.push(mem.id);
        }
        let create_elapsed = create_start.elapsed();
        let create_per_sec = if create_elapsed.as_secs_f64() > 0.0 {
            self.num_memories as f64 / create_elapsed.as_secs_f64()
        } else {
            self.num_memories as f64 * 1_000_000.0
        };

        // ===== Phase 2: GET throughput =====
        let get_start = Instant::now();
        let mut get_hits = 0usize;
        for &id in &created_ids {
            if storage.with_connection(|conn| get_memory(conn, id)).is_ok() {
                get_hits += 1;
            }
        }
        let get_elapsed = get_start.elapsed();
        let get_per_sec = if get_elapsed.as_secs_f64() > 0.0 {
            get_hits as f64 / get_elapsed.as_secs_f64()
        } else {
            get_hits as f64 * 1_000_000.0
        };

        // ===== Phase 3: SEARCH throughput + quality =====
        // Create topic-specific memories and track which IDs are relevant
        let mut topic_relevant_ids: HashMap<&str, Vec<i64>> = HashMap::new();

        for topic in TOPICS {
            let mut relevant = Vec::new();
            for phrase in topic.relevant_phrases {
                let mem = storage.with_connection(|conn| {
                    create_memory(
                        conn,
                        &CreateMemoryInput {
                            content: phrase.to_string(),
                            memory_type: MemoryType::Note,
                            workspace: Some("membench-quality".to_string()),
                            ..Default::default()
                        },
                    )
                })?;
                relevant.push(mem.id);
            }
            topic_relevant_ids.insert(topic.keyword, relevant);
        }

        let search_start = Instant::now();
        let mut ndcg_sum = 0.0f64;
        let mut mrr_sum = 0.0f64;
        let mut search_count = 0usize;

        let queries: Vec<&str> = TOPICS
            .iter()
            .map(|t| t.keyword)
            .cycle()
            .take(self.num_queries)
            .collect();

        for query in &queries {
            let keyword_pattern = format!("%{}%", query);
            let retrieved_ids: Vec<i64> = storage.with_connection(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id FROM memories WHERE content LIKE ?1 \
                     ORDER BY created_at DESC LIMIT 10",
                )?;
                let ids: Vec<i64> = stmt
                    .query_map([&keyword_pattern], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect();
                Ok(ids)
            })?;

            if let Some(relevant_ids) = topic_relevant_ids.get(query) {
                ndcg_sum += Self::ndcg_at_k(&retrieved_ids, relevant_ids, 10);
                mrr_sum += Self::mrr(&retrieved_ids, relevant_ids);
                search_count += 1;
            }
        }

        let search_elapsed = search_start.elapsed();
        let search_per_sec = if search_elapsed.as_secs_f64() > 0.0 {
            self.num_queries as f64 / search_elapsed.as_secs_f64()
        } else {
            self.num_queries as f64 * 1_000_000.0
        };

        let ndcg_at_10 = if search_count > 0 {
            ndcg_sum / search_count as f64
        } else {
            0.0
        };

        let mrr = if search_count > 0 {
            mrr_sum / search_count as f64
        } else {
            0.0
        };

        let duration_ms = create_elapsed.as_millis() as u64
            + get_elapsed.as_millis() as u64
            + search_elapsed.as_millis() as u64;

        let mut metrics = HashMap::new();
        metrics.insert(
            "create_per_sec".to_string(),
            create_per_sec.min(1_000_000.0),
        );
        metrics.insert("get_per_sec".to_string(), get_per_sec.min(1_000_000.0));
        metrics.insert(
            "search_per_sec".to_string(),
            search_per_sec.min(1_000_000.0),
        );
        metrics.insert("ndcg_at_10".to_string(), ndcg_at_10);
        metrics.insert("mrr".to_string(), mrr);
        metrics.insert("num_memories".to_string(), self.num_memories as f64);
        metrics.insert("num_queries".to_string(), self.num_queries as f64);

        // Clean up temporary file
        if db_path != ":memory:" {
            let bench_path = format!("{}.membench.db", db_path);
            drop(storage);
            let _ = std::fs::remove_file(&bench_path);
            let _ = std::fs::remove_file(format!("{}-wal", bench_path));
            let _ = std::fs::remove_file(format!("{}-shm", bench_path));
        }

        Ok(BenchmarkResult {
            name: self.name().to_string(),
            metrics,
            duration_ms,
            timestamp: chrono::Utc::now().to_rfc3339(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_membench_runs() {
        let bench = MemBenchmark {
            num_memories: 20,
            num_queries: 5,
        };
        let result = bench.run(":memory:").expect("benchmark should succeed");
        assert_eq!(result.name, "membench");
    }

    #[test]
    fn test_membench_metrics_present() {
        let bench = MemBenchmark {
            num_memories: 10,
            num_queries: 4,
        };
        let result = bench.run(":memory:").expect("benchmark should succeed");

        let expected_keys = [
            "create_per_sec",
            "get_per_sec",
            "search_per_sec",
            "ndcg_at_10",
            "mrr",
        ];
        for key in &expected_keys {
            assert!(result.metrics.contains_key(*key), "missing metric: {}", key);
        }
    }

    #[test]
    fn test_throughput_positive() {
        let bench = MemBenchmark {
            num_memories: 50,
            num_queries: 10,
        };
        let result = bench.run(":memory:").expect("benchmark should succeed");
        assert!(
            result.metrics["create_per_sec"] > 0.0,
            "create_per_sec should be positive"
        );
        assert!(
            result.metrics["get_per_sec"] > 0.0,
            "get_per_sec should be positive"
        );
    }

    #[test]
    fn test_ndcg_range() {
        let bench = MemBenchmark {
            num_memories: 30,
            num_queries: 8,
        };
        let result = bench.run(":memory:").expect("benchmark should succeed");
        let ndcg = result.metrics["ndcg_at_10"];
        assert!(
            (0.0..=1.0).contains(&ndcg),
            "NDCG@10 = {} out of range",
            ndcg
        );
    }

    #[test]
    fn test_ndcg_at_k_computation() {
        // Relevant docs at positions 0, 2, 4 → DCG should be positive
        let relevant = vec![1i64, 2, 3];
        let retrieved = vec![1i64, 4, 2, 5, 3];
        let ndcg = MemBenchmark::ndcg_at_k(&retrieved, &relevant, 5);
        assert!(ndcg > 0.0 && ndcg <= 1.0, "ndcg={}", ndcg);

        // Empty retrieval → NDCG = 0
        let ndcg_empty = MemBenchmark::ndcg_at_k(&[], &relevant, 10);
        assert_eq!(ndcg_empty, 0.0);

        // Perfect ranking: relevant docs at the top → NDCG = 1.0
        let perfect = vec![1i64, 2, 3, 4, 5];
        let ndcg_perfect = MemBenchmark::ndcg_at_k(&perfect, &[1, 2, 3], 3);
        assert!(
            (ndcg_perfect - 1.0).abs() < 1e-9,
            "perfect ndcg={}",
            ndcg_perfect
        );
    }

    #[test]
    fn test_mrr_computation() {
        // First hit at position 2 (0-indexed) → MRR = 1/3
        let relevant = vec![3i64];
        let retrieved = vec![1i64, 2, 3, 4, 5];
        let mrr = MemBenchmark::mrr(&retrieved, &relevant);
        assert!((mrr - 1.0 / 3.0).abs() < 1e-9, "mrr={}", mrr);

        // No hit → MRR = 0
        let mrr_miss = MemBenchmark::mrr(&[10, 11, 12], &[99]);
        assert_eq!(mrr_miss, 0.0);

        // First position hit → MRR = 1.0
        let mrr_first = MemBenchmark::mrr(&[5, 6, 7], &[5]);
        assert!((mrr_first - 1.0).abs() < 1e-9, "mrr_first={}", mrr_first);
    }
}
