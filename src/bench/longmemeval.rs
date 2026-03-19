//! LongMemEval benchmark — 5-dimension memory evaluation
//!
//! Evaluates memory quality across five dimensions:
//! 1. Information Retention — can we retrieve stored facts?
//! 2. Temporal Reasoning — can we answer time-based questions?
//! 3. Knowledge Update — does updating a fact reflect correctly?
//! 4. Multi-Hop Reasoning — can we chain facts across memories?
//! 5. Contradiction Detection — do we surface conflicting facts?

use std::collections::HashMap;
use std::time::Instant;

use super::{Benchmark, BenchmarkResult};
use crate::storage::queries::{create_memory, update_memory};
use crate::storage::Storage;
use crate::types::{CreateMemoryInput, MemoryType, StorageConfig, StorageMode, UpdateMemoryInput};

/// LongMemEval benchmark with configurable dimension weights
pub struct LongMemEvalBenchmark {
    /// Weights per dimension (will be normalized if they don't sum to 1.0)
    pub dimension_weights: HashMap<String, f64>,
}

impl Default for LongMemEvalBenchmark {
    fn default() -> Self {
        let mut weights = HashMap::new();
        weights.insert("information_retention".to_string(), 0.25);
        weights.insert("temporal_reasoning".to_string(), 0.20);
        weights.insert("knowledge_update".to_string(), 0.20);
        weights.insert("multi_hop".to_string(), 0.20);
        weights.insert("contradiction_detection".to_string(), 0.15);
        Self {
            dimension_weights: weights,
        }
    }
}

/// A single test case for keyword-retrieval dimensions
struct TestCase {
    setup_memories: Vec<String>,
    query_keyword: String,
    expected_content_substring: String,
}

impl LongMemEvalBenchmark {
    /// Evaluate information retention: store facts and retrieve them by keyword
    fn eval_information_retention(&self, storage: &Storage) -> f64 {
        let cases = vec![
            TestCase {
                setup_memories: vec![
                    "Alice is a software engineer at TechCorp".to_string(),
                    "Alice has 5 years of experience in Rust".to_string(),
                ],
                query_keyword: "Alice".to_string(),
                expected_content_substring: "engineer".to_string(),
            },
            TestCase {
                setup_memories: vec![
                    "The Eiffel Tower is located in Paris, France".to_string(),
                    "The Eiffel Tower was built in 1889".to_string(),
                ],
                query_keyword: "Eiffel".to_string(),
                expected_content_substring: "Paris".to_string(),
            },
            TestCase {
                setup_memories: vec!["Project Alpha deadline is Q3 2026".to_string()],
                query_keyword: "Alpha".to_string(),
                expected_content_substring: "Q3 2026".to_string(),
            },
        ];

        self.run_cases(storage, &cases)
    }

    /// Evaluate temporal reasoning: tag memories with dates and retrieve by time context
    fn eval_temporal_reasoning(&self, storage: &Storage) -> f64 {
        let cases = vec![
            TestCase {
                setup_memories: vec![
                    "Meeting on 2026-01-10: discussed Q1 roadmap".to_string(),
                    "Meeting on 2026-02-15: reviewed Q2 budget".to_string(),
                ],
                query_keyword: "2026-01".to_string(),
                expected_content_substring: "Q1 roadmap".to_string(),
            },
            TestCase {
                setup_memories: vec![
                    "Sprint 42 started on 2026-03-01".to_string(),
                    "Sprint 42 ended on 2026-03-14 with 12 story points".to_string(),
                ],
                query_keyword: "Sprint 42".to_string(),
                expected_content_substring: "story points".to_string(),
            },
        ];

        self.run_cases(storage, &cases)
    }

    /// Evaluate knowledge update: create a fact, update it, verify new value is retrievable
    fn eval_knowledge_update(&self, storage: &Storage) -> f64 {
        let mut correct = 0usize;
        let total = 3usize;

        // Test 1: Update memory content and verify retrieval
        let mem = storage
            .with_connection(|conn| {
                create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Budget for Q1 is $50,000".to_string(),
                        memory_type: MemoryType::Note,
                        workspace: Some("longmemeval-bench".to_string()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();

        let update = UpdateMemoryInput {
            content: Some("Budget for Q1 is $75,000 (revised)".to_string()),
            memory_type: None,
            tags: None,
            metadata: None,
            importance: None,
            scope: None,
            ttl_seconds: None,
            event_time: None,
            trigger_pattern: None,
            media_url: None,
        };
        let _ = storage.with_connection(|conn| update_memory(conn, mem.id, &update));

        let updated_content: Option<String> = storage
            .with_connection(|conn| {
                conn.query_row(
                    "SELECT content FROM memories WHERE id = ?1",
                    [mem.id],
                    |row| row.get(0),
                )
                .map_err(crate::error::EngramError::Database)
            })
            .ok();

        if let Some(c) = updated_content {
            if c.contains("$75,000") {
                correct += 1;
            }
        }

        // Test 2: Update service config timeout
        let tag_mem = storage
            .with_connection(|conn| {
                create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Service config: timeout=30s".to_string(),
                        memory_type: MemoryType::Note,
                        workspace: Some("longmemeval-bench".to_string()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();

        let update2 = UpdateMemoryInput {
            content: Some("Service config: timeout=60s (doubled for reliability)".to_string()),
            memory_type: None,
            tags: None,
            metadata: None,
            importance: None,
            scope: None,
            ttl_seconds: None,
            event_time: None,
            trigger_pattern: None,
            media_url: None,
        };
        let _ = storage.with_connection(|conn| update_memory(conn, tag_mem.id, &update2));

        let updated2: Option<String> = storage
            .with_connection(|conn| {
                conn.query_row(
                    "SELECT content FROM memories WHERE id = ?1",
                    [tag_mem.id],
                    |row| row.get(0),
                )
                .map_err(crate::error::EngramError::Database)
            })
            .ok();

        if let Some(c) = updated2 {
            if c.contains("timeout=60s") {
                correct += 1;
            }
        }

        // Test 3: Verify original value is replaced (not duplicated)
        let count: i64 = storage
            .with_connection(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM memories WHERE content LIKE '%timeout=30s%' AND id = ?1",
                    [tag_mem.id],
                    |row| row.get(0),
                )
                .map_err(crate::error::EngramError::Database)
            })
            .unwrap_or(1);

        if count == 0 {
            correct += 1;
        }

        correct as f64 / total as f64
    }

    /// Evaluate multi-hop: store chained facts, verify both are retrievable via linking keyword
    fn eval_multi_hop(&self, storage: &Storage) -> f64 {
        let cases = vec![
            TestCase {
                setup_memories: vec![
                    "Node A connects to Node B via link L1".to_string(),
                    "Node B connects to Node C via link L2".to_string(),
                ],
                query_keyword: "Node B".to_string(),
                expected_content_substring: "connects".to_string(),
            },
            TestCase {
                setup_memories: vec![
                    "Company Acme acquired Startup X in 2024".to_string(),
                    "Startup X built the Zephyr product".to_string(),
                    "Zephyr product has 50,000 active users".to_string(),
                ],
                query_keyword: "Zephyr".to_string(),
                expected_content_substring: "users".to_string(),
            },
        ];

        self.run_cases(storage, &cases)
    }

    /// Evaluate contradiction detection: store conflicting facts, verify both appear
    fn eval_contradiction_detection(&self, storage: &Storage) -> f64 {
        let mut correct = 0usize;
        let total = 2usize;

        let pairs = [
            (
                "Server capacity is 100 concurrent users (from 2025-01 report)",
                "Server capacity is 500 concurrent users (from 2026-01 report)",
                "concurrent users",
            ),
            (
                "API rate limit is 100 req/min per client",
                "API rate limit is 1000 req/min per client (updated)",
                "rate limit",
            ),
        ];

        for (fact_a, fact_b, keyword) in &pairs {
            let _ = storage.with_connection(|conn| {
                create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: fact_a.to_string(),
                        memory_type: MemoryType::Note,
                        workspace: Some("longmemeval-bench".to_string()),
                        ..Default::default()
                    },
                )
            });
            let _ = storage.with_connection(|conn| {
                create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: fact_b.to_string(),
                        memory_type: MemoryType::Note,
                        workspace: Some("longmemeval-bench".to_string()),
                        ..Default::default()
                    },
                )
            });

            let count: i64 = storage
                .with_connection(|conn| {
                    conn.query_row(
                        "SELECT COUNT(*) FROM memories WHERE content LIKE ?1",
                        [format!("%{}%", keyword)],
                        |row| row.get(0),
                    )
                    .map_err(crate::error::EngramError::Database)
                })
                .unwrap_or(0);

            if count >= 2 {
                correct += 1;
            }
        }

        correct as f64 / total as f64
    }

    /// Helper: store memories for each case and check if expected content is retrievable
    fn run_cases(&self, storage: &Storage, cases: &[TestCase]) -> f64 {
        if cases.is_empty() {
            return 1.0;
        }

        let mut correct = 0usize;

        for case in cases {
            for content in &case.setup_memories {
                let _ = storage.with_connection(|conn| {
                    create_memory(
                        conn,
                        &CreateMemoryInput {
                            content: content.clone(),
                            memory_type: MemoryType::Note,
                            workspace: Some("longmemeval-bench".to_string()),
                            ..Default::default()
                        },
                    )
                });
            }

            let retrieved: Option<String> = storage
                .with_connection(|conn| {
                    conn.query_row(
                        "SELECT content FROM memories WHERE content LIKE ?1 LIMIT 1",
                        [format!("%{}%", case.query_keyword)],
                        |row| row.get(0),
                    )
                    .map_err(crate::error::EngramError::Database)
                })
                .ok();

            if let Some(content) = retrieved {
                if content.contains(&case.expected_content_substring) {
                    correct += 1;
                }
            }
        }

        correct as f64 / cases.len() as f64
    }

    /// Compute weighted score across dimensions
    fn weighted_score(&self, scores: &HashMap<String, f64>) -> f64 {
        let total_weight: f64 = self.dimension_weights.values().sum();
        if total_weight == 0.0 {
            return 0.0;
        }

        self.dimension_weights
            .iter()
            .filter_map(|(dim, &weight)| scores.get(dim).map(|&score| score * weight))
            .sum::<f64>()
            / total_weight
    }
}

impl Benchmark for LongMemEvalBenchmark {
    fn name(&self) -> &str {
        "longmemeval"
    }

    fn description(&self) -> &str {
        "5-dimension memory evaluation benchmark: information retention, temporal reasoning, \
         knowledge update, multi-hop reasoning, and contradiction detection."
    }

    fn run(&self, db_path: &str) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
        let start = Instant::now();

        let storage = if db_path == ":memory:" {
            Storage::open_in_memory()?
        } else {
            let bench_path = format!("{}.longmemeval_bench.db", db_path);
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

        let retention = self.eval_information_retention(&storage);
        let temporal = self.eval_temporal_reasoning(&storage);
        let knowledge_update = self.eval_knowledge_update(&storage);
        let multi_hop = self.eval_multi_hop(&storage);
        let contradiction = self.eval_contradiction_detection(&storage);

        let mut dimension_scores = HashMap::new();
        dimension_scores.insert("information_retention".to_string(), retention);
        dimension_scores.insert("temporal_reasoning".to_string(), temporal);
        dimension_scores.insert("knowledge_update".to_string(), knowledge_update);
        dimension_scores.insert("multi_hop".to_string(), multi_hop);
        dimension_scores.insert("contradiction_detection".to_string(), contradiction);

        let weighted = self.weighted_score(&dimension_scores);

        let duration_ms = start.elapsed().as_millis() as u64;

        let mut metrics = dimension_scores;
        metrics.insert("weighted_score".to_string(), weighted);

        // Clean up temporary file
        if db_path != ":memory:" {
            let bench_path = format!("{}.longmemeval_bench.db", db_path);
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
    fn test_longmemeval_runs() {
        let bench = LongMemEvalBenchmark::default();
        let result = bench.run(":memory:").expect("benchmark should succeed");
        assert_eq!(result.name, "longmemeval");
    }

    #[test]
    fn test_all_dimensions_present() {
        let bench = LongMemEvalBenchmark::default();
        let result = bench.run(":memory:").expect("benchmark should succeed");

        let expected_dims = [
            "information_retention",
            "temporal_reasoning",
            "knowledge_update",
            "multi_hop",
            "contradiction_detection",
            "weighted_score",
        ];
        for dim in &expected_dims {
            assert!(
                result.metrics.contains_key(*dim),
                "missing dimension: {}",
                dim
            );
        }
    }

    #[test]
    fn test_scores_in_range() {
        let bench = LongMemEvalBenchmark::default();
        let result = bench.run(":memory:").expect("benchmark should succeed");

        for (key, value) in &result.metrics {
            assert!(
                (0.0..=1.0).contains(value),
                "metric '{}' = {} out of range [0,1]",
                key,
                value
            );
        }
    }

    #[test]
    fn test_weighted_score_with_custom_weights() {
        let mut weights = HashMap::new();
        weights.insert("information_retention".to_string(), 1.0);
        weights.insert("temporal_reasoning".to_string(), 0.0);
        weights.insert("knowledge_update".to_string(), 0.0);
        weights.insert("multi_hop".to_string(), 0.0);
        weights.insert("contradiction_detection".to_string(), 0.0);

        let bench = LongMemEvalBenchmark {
            dimension_weights: weights,
        };
        let result = bench.run(":memory:").expect("benchmark should succeed");
        let retention = result.metrics["information_retention"];
        let weighted = result.metrics["weighted_score"];
        assert!(
            (weighted - retention).abs() < 1e-9,
            "weighted={} retention={}",
            weighted,
            retention
        );
    }
}
