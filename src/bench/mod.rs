//! Standardized benchmark suite for Engram
//!
//! Provides implementations of standard AI memory benchmarks:
//! - LOCOMO: Multi-session conversation memory
//! - LongMemEval: 5-dimension memory evaluation
//! - MemBench: CRUD throughput and search quality

pub mod locomo;
pub mod longmemeval;
pub mod membench;

use std::collections::HashMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Result of running a single benchmark
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub name: String,
    pub metrics: HashMap<String, f64>,
    pub duration_ms: u64,
    pub timestamp: String,
}

/// A benchmark that can be run against an Engram database
pub trait Benchmark: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn run(&self, db_path: &str) -> Result<BenchmarkResult, Box<dyn std::error::Error>>;
}

/// Suite that manages and runs multiple benchmarks
pub struct BenchmarkSuite {
    benchmarks: Vec<Box<dyn Benchmark>>,
}

impl BenchmarkSuite {
    /// Create an empty benchmark suite
    pub fn new() -> Self {
        Self {
            benchmarks: Vec::new(),
        }
    }

    /// Add a benchmark to the suite
    pub fn add(&mut self, benchmark: Box<dyn Benchmark>) {
        self.benchmarks.push(benchmark);
    }

    /// Run all benchmarks and return results
    pub fn run_all(&self, db_path: &str) -> Vec<BenchmarkResult> {
        self.benchmarks
            .iter()
            .map(|b| {
                b.run(db_path).unwrap_or_else(|e| BenchmarkResult {
                    name: b.name().to_string(),
                    metrics: {
                        let mut m = HashMap::new();
                        m.insert("error".to_string(), 0.0);
                        m.insert("error_message".to_string(), 0.0);
                        let _ = e;
                        m
                    },
                    duration_ms: 0,
                    timestamp: Utc::now().to_rfc3339(),
                })
            })
            .collect()
    }

    /// Format results as JSON
    pub fn report_json(results: &[BenchmarkResult]) -> String {
        serde_json::to_string_pretty(results).unwrap_or_else(|_| "[]".to_string())
    }

    /// Format results as Markdown table
    pub fn report_markdown(results: &[BenchmarkResult]) -> String {
        if results.is_empty() {
            return "No benchmark results.\n".to_string();
        }

        let mut out = String::new();
        out.push_str("# Engram Benchmark Results\n\n");
        out.push_str(&format!("*Run at: {}*\n\n", Utc::now().to_rfc3339()));

        for result in results {
            out.push_str(&format!("## {}\n\n", result.name));
            out.push_str(&format!("Duration: {}ms\n\n", result.duration_ms));
            out.push_str("| Metric | Value |\n");
            out.push_str("|--------|-------|\n");

            let mut metrics: Vec<_> = result.metrics.iter().collect();
            metrics.sort_by_key(|(k, _)| k.as_str());
            for (key, value) in metrics {
                out.push_str(&format!("| {} | {:.4} |\n", key, value));
            }
            out.push('\n');
        }

        out
    }

    /// Format results as CSV
    pub fn report_csv(results: &[BenchmarkResult]) -> String {
        if results.is_empty() {
            return "benchmark,metric,value,duration_ms,timestamp\n".to_string();
        }

        let mut out = String::from("benchmark,metric,value,duration_ms,timestamp\n");

        for result in results {
            let mut metrics: Vec<_> = result.metrics.iter().collect();
            metrics.sort_by_key(|(k, _)| k.as_str());
            for (key, value) in metrics {
                out.push_str(&format!(
                    "{},{},{:.6},{},{}\n",
                    result.name, key, value, result.duration_ms, result.timestamp
                ));
            }
        }

        out
    }
}

impl Default for BenchmarkSuite {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the default suite with all benchmarks
pub fn default_suite() -> BenchmarkSuite {
    let mut suite = BenchmarkSuite::new();
    suite.add(Box::new(locomo::LocomoBenchmark {
        num_conversations: 10,
        queries_per_conversation: 3,
    }));
    suite.add(Box::new(longmemeval::LongMemEvalBenchmark::default()));
    suite.add(Box::new(membench::MemBenchmark {
        num_memories: 100,
        num_queries: 20,
    }));
    suite
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyBenchmark {
        name: String,
    }

    impl Benchmark for DummyBenchmark {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "A dummy benchmark for testing"
        }

        fn run(&self, _db_path: &str) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
            let mut metrics = HashMap::new();
            metrics.insert("score".to_string(), 0.95);
            metrics.insert("latency_ms".to_string(), 12.5);

            Ok(BenchmarkResult {
                name: self.name.clone(),
                metrics,
                duration_ms: 42,
                timestamp: Utc::now().to_rfc3339(),
            })
        }
    }

    #[test]
    fn test_suite_creation() {
        let suite = BenchmarkSuite::new();
        assert_eq!(suite.benchmarks.len(), 0);
    }

    #[test]
    fn test_suite_add_and_run() {
        let mut suite = BenchmarkSuite::new();
        suite.add(Box::new(DummyBenchmark {
            name: "test-bench".to_string(),
        }));
        assert_eq!(suite.benchmarks.len(), 1);

        let results = suite.run_all(":memory:");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "test-bench");
        assert!(results[0].metrics.contains_key("score"));
        assert_eq!(results[0].duration_ms, 42);
    }

    #[test]
    fn test_report_json() {
        let results = vec![BenchmarkResult {
            name: "test".to_string(),
            metrics: {
                let mut m = HashMap::new();
                m.insert("precision".to_string(), 0.85);
                m
            },
            duration_ms: 100,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        }];

        let json = BenchmarkSuite::report_json(&results);
        assert!(json.contains("\"test\""));
        assert!(json.contains("precision"));
        assert!(json.contains("0.85"));
    }

    #[test]
    fn test_report_markdown() {
        let results = vec![BenchmarkResult {
            name: "locomo".to_string(),
            metrics: {
                let mut m = HashMap::new();
                m.insert("f1".to_string(), 0.72);
                m
            },
            duration_ms: 200,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        }];

        let md = BenchmarkSuite::report_markdown(&results);
        assert!(md.contains("## locomo"));
        assert!(md.contains("200ms"));
        assert!(md.contains("f1"));
    }

    #[test]
    fn test_report_csv() {
        let results = vec![BenchmarkResult {
            name: "membench".to_string(),
            metrics: {
                let mut m = HashMap::new();
                m.insert("create_per_sec".to_string(), 500.0);
                m
            },
            duration_ms: 150,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        }];

        let csv = BenchmarkSuite::report_csv(&results);
        assert!(csv.starts_with("benchmark,metric,value"));
        assert!(csv.contains("membench"));
        assert!(csv.contains("create_per_sec"));
    }

    #[test]
    fn test_result_serialization() {
        let result = BenchmarkResult {
            name: "roundtrip".to_string(),
            metrics: {
                let mut m = HashMap::new();
                m.insert("recall".to_string(), 0.9);
                m
            },
            duration_ms: 77,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };

        let serialized = serde_json::to_string(&result).expect("should serialize");
        let deserialized: BenchmarkResult =
            serde_json::from_str(&serialized).expect("should deserialize");
        assert_eq!(deserialized.name, result.name);
        assert_eq!(deserialized.duration_ms, result.duration_ms);
        assert!((deserialized.metrics["recall"] - 0.9).abs() < 1e-9);
    }
}
