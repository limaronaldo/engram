//! LOCOMO benchmark — Multi-session conversation memory
//!
//! Measures precision, recall, and F1 for retrieving facts that were
//! discussed across multiple synthetic conversation sessions.

use std::collections::HashMap;
use std::time::Instant;

use super::{Benchmark, BenchmarkResult};
use crate::storage::queries::create_memory;
use crate::storage::Storage;
use crate::types::{CreateMemoryInput, MemoryType, StorageConfig, StorageMode};

/// LOCOMO benchmark configuration
pub struct LocomoBenchmark {
    /// Number of synthetic conversations to generate
    pub num_conversations: usize,
    /// Number of recall queries per conversation
    pub queries_per_conversation: usize,
}

/// A synthetic conversation with ground-truth answers
struct SyntheticConversation {
    session_id: usize,
    facts: Vec<String>,
    queries: Vec<ConversationQuery>,
}

/// A recall query with expected relevant fact indices (into `SyntheticConversation::facts`)
struct ConversationQuery {
    expected_fact_indices: Vec<usize>,
}

impl LocomoBenchmark {
    fn generate_conversations(&self) -> Vec<SyntheticConversation> {
        let templates = [
            (
                "Alice works at Acme Corp as a software engineer",
                "Bob is studying machine learning at MIT",
                "Carol prefers Python over Rust for scripting",
            ),
            (
                "David's favorite color is blue and he lives in London",
                "Eve is learning Japanese and visits Tokyo each year",
                "Frank is allergic to peanuts and avoids Thai food",
            ),
            (
                "Grace runs marathons every spring in Boston",
                "Henry has two cats named Luna and Mochi",
                "Iris is a vegetarian who loves Italian cuisine",
            ),
            (
                "Jack recently moved from New York to San Francisco",
                "Karen plays the piano and violin professionally",
                "Leo is a night owl who does his best work after midnight",
            ),
            (
                "Mia has a PhD in quantum computing from Caltech",
                "Noah volunteers at the local animal shelter every weekend",
                "Olivia runs a small bakery specializing in sourdough bread",
            ),
        ];

        (0..self.num_conversations)
            .map(|i| {
                let tpl = &templates[i % templates.len()];

                let facts = vec![
                    format!("Session {}: {}", i, tpl.0),
                    format!("Session {}: {}", i, tpl.1),
                    format!("Session {}: {}", i, tpl.2),
                ];

                let num_queries = self.queries_per_conversation.min(facts.len());
                let queries = (0..num_queries)
                    .map(|fi| ConversationQuery {
                        expected_fact_indices: vec![fi],
                    })
                    .collect();

                SyntheticConversation {
                    session_id: i,
                    facts,
                    queries,
                }
            })
            .collect()
    }
}

impl Benchmark for LocomoBenchmark {
    fn name(&self) -> &str {
        "locomo"
    }

    fn description(&self) -> &str {
        "Multi-session conversation memory benchmark. Measures precision, recall, and F1 \
         for retrieving facts stored across multiple synthetic conversation sessions."
    }

    fn run(&self, db_path: &str) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
        let start = Instant::now();

        // Open an isolated Storage
        let storage = if db_path == ":memory:" {
            Storage::open_in_memory()?
        } else {
            let bench_path = format!("{}.locomo_bench.db", db_path);
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

        // Phase 1: Index synthetic conversations
        let conversations = self.generate_conversations();
        let mut memory_ids: Vec<Vec<i64>> = Vec::new();

        for conv in &conversations {
            let mut ids = Vec::new();
            for fact in &conv.facts {
                let content = fact.clone();
                let session_tag = format!("session:{}", conv.session_id);
                let mem = storage.with_connection(|conn| {
                    create_memory(
                        conn,
                        &CreateMemoryInput {
                            content,
                            memory_type: MemoryType::Episodic,
                            tags: vec![session_tag],
                            workspace: Some("locomo-bench".to_string()),
                            ..Default::default()
                        },
                    )
                })?;
                ids.push(mem.id);
            }
            memory_ids.push(ids);
        }

        // Phase 2: Run recall queries using session-scoped LIKE search
        let mut true_positives = 0usize;
        let mut false_positives = 0usize;
        let mut false_negatives = 0usize;

        for (conv_idx, conv) in conversations.iter().enumerate() {
            let conv_ids = &memory_ids[conv_idx];

            for query in &conv.queries {
                // Retrieve all memories for this session
                let keyword = format!("%Session {}%", conv.session_id);
                let retrieved_ids: Vec<i64> = storage.with_connection(|conn| {
                    let mut stmt = conn.prepare(
                        "SELECT id FROM memories WHERE content LIKE ?1 LIMIT 10",
                    )?;
                    let ids: Vec<i64> = stmt
                        .query_map([&keyword], |row| row.get(0))?
                        .filter_map(|r| r.ok())
                        .collect();
                    Ok(ids)
                })?;

                // Compute expected IDs
                let expected_ids: Vec<i64> = query
                    .expected_fact_indices
                    .iter()
                    .filter_map(|&fi| conv_ids.get(fi).copied())
                    .collect();

                for &rid in &retrieved_ids {
                    if expected_ids.contains(&rid) {
                        true_positives += 1;
                    } else {
                        false_positives += 1;
                    }
                }

                for &eid in &expected_ids {
                    if !retrieved_ids.contains(&eid) {
                        false_negatives += 1;
                    }
                }
            }
        }

        // Compute precision, recall, F1
        let precision = if true_positives + false_positives > 0 {
            true_positives as f64 / (true_positives + false_positives) as f64
        } else {
            0.0
        };

        let recall = if true_positives + false_negatives > 0 {
            true_positives as f64 / (true_positives + false_negatives) as f64
        } else {
            0.0
        };

        let f1 = if precision + recall > 0.0 {
            2.0 * precision * recall / (precision + recall)
        } else {
            0.0
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        let mut metrics = HashMap::new();
        metrics.insert("precision".to_string(), precision);
        metrics.insert("recall".to_string(), recall);
        metrics.insert("f1".to_string(), f1);
        metrics.insert("num_conversations".to_string(), self.num_conversations as f64);
        metrics.insert(
            "queries_per_conversation".to_string(),
            self.queries_per_conversation as f64,
        );
        metrics.insert("true_positives".to_string(), true_positives as f64);
        metrics.insert("false_positives".to_string(), false_positives as f64);
        metrics.insert("false_negatives".to_string(), false_negatives as f64);

        // Clean up temporary database file if not in-memory
        if db_path != ":memory:" {
            let bench_path = format!("{}.locomo_bench.db", db_path);
            // Drop storage first to release file handles
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
    fn test_locomo_runs_in_memory() {
        let bench = LocomoBenchmark {
            num_conversations: 3,
            queries_per_conversation: 2,
        };
        let result = bench.run(":memory:").expect("benchmark should succeed");
        assert_eq!(result.name, "locomo");
        assert!(result.metrics.contains_key("precision"));
        assert!(result.metrics.contains_key("recall"));
        assert!(result.metrics.contains_key("f1"));
    }

    #[test]
    fn test_locomo_metrics_range() {
        let bench = LocomoBenchmark {
            num_conversations: 2,
            queries_per_conversation: 1,
        };
        let result = bench.run(":memory:").expect("benchmark should succeed");
        let precision = result.metrics["precision"];
        let recall = result.metrics["recall"];
        let f1 = result.metrics["f1"];

        assert!((0.0..=1.0).contains(&precision), "precision out of range: {}", precision);
        assert!((0.0..=1.0).contains(&recall), "recall out of range: {}", recall);
        assert!((0.0..=1.0).contains(&f1), "f1 out of range: {}", f1);
    }

    #[test]
    fn test_locomo_generates_correct_conversation_count() {
        let bench = LocomoBenchmark {
            num_conversations: 5,
            queries_per_conversation: 2,
        };
        let conversations = bench.generate_conversations();
        assert_eq!(conversations.len(), 5);
        for conv in &conversations {
            assert_eq!(conv.queries.len(), 2);
            assert_eq!(conv.facts.len(), 3);
        }
    }
}
