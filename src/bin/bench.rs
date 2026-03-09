//! Engram benchmark CLI (`engram-bench`)
//!
//! Runs standardized memory benchmarks: LOCOMO, LongMemEval, MemBench.

use clap::{Parser, Subcommand};

use engram::bench::{
    locomo::LocomoBenchmark, longmemeval::LongMemEvalBenchmark, membench::MemBenchmark,
    BenchmarkSuite,
};

#[derive(Parser)]
#[command(name = "engram-bench")]
#[command(about = "Engram benchmark suite — standardized AI memory benchmarks")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run benchmark(s)
    Run {
        /// Which suite to run: locomo, longmem, membench, or all
        #[arg(long, default_value = "all")]
        suite: String,

        /// Output format: json, md, or csv
        #[arg(long, default_value = "json")]
        output: String,

        /// Path to the benchmark database (uses a temporary file per benchmark)
        #[arg(long)]
        db_path: Option<String>,

        /// Number of memories for membench (default: 500)
        #[arg(long, default_value = "500")]
        num_memories: usize,

        /// Number of queries for membench (default: 50)
        #[arg(long, default_value = "50")]
        num_queries: usize,

        /// Number of conversations for locomo (default: 20)
        #[arg(long, default_value = "20")]
        num_conversations: usize,

        /// Queries per conversation for locomo (default: 3)
        #[arg(long, default_value = "3")]
        queries_per_conv: usize,
    },
    /// List available benchmarks
    List,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::List => {
            println!("Available benchmarks:");
            println!();
            println!("  locomo       Multi-session conversation memory (precision, recall, F1)");
            println!("  longmem      5-dimension evaluation (retention, temporal, update, multi-hop, contradiction)");
            println!("  membench     CRUD throughput and search quality (create/get/search per-sec, NDCG@10, MRR)");
            println!("  all          Run all benchmarks with default settings");
        }

        Commands::Run {
            suite,
            output,
            db_path,
            num_memories,
            num_queries,
            num_conversations,
            queries_per_conv,
        } => {
            let db = db_path.as_deref().unwrap_or(":memory:");

            eprintln!("Running benchmark suite: {}", suite);
            eprintln!("Database: {}", db);
            eprintln!();

            let mut bench_suite = BenchmarkSuite::new();

            match suite.as_str() {
                "locomo" => {
                    bench_suite.add(Box::new(LocomoBenchmark {
                        num_conversations,
                        queries_per_conversation: queries_per_conv,
                    }));
                }
                "longmem" => {
                    bench_suite.add(Box::new(LongMemEvalBenchmark::default()));
                }
                "membench" => {
                    bench_suite.add(Box::new(MemBenchmark {
                        num_memories,
                        num_queries,
                    }));
                }
                "all" => {
                    // Use the default suite with CLI-overridden sizes
                    bench_suite.add(Box::new(LocomoBenchmark {
                        num_conversations,
                        queries_per_conversation: queries_per_conv,
                    }));
                    bench_suite.add(Box::new(LongMemEvalBenchmark::default()));
                    bench_suite.add(Box::new(MemBenchmark {
                        num_memories,
                        num_queries,
                    }));
                }
                other => {
                    eprintln!("Unknown suite: '{}'. Use: locomo, longmem, membench, all", other);
                    std::process::exit(1);
                }
            }

            let results = bench_suite.run_all(db);

            // Print progress summary to stderr
            for result in &results {
                eprintln!(
                    "[{}] completed in {}ms",
                    result.name, result.duration_ms
                );
            }
            eprintln!();

            let report = match output.as_str() {
                "md" | "markdown" => BenchmarkSuite::report_markdown(&results),
                "csv" => BenchmarkSuite::report_csv(&results),
                _ => BenchmarkSuite::report_json(&results),
            };

            println!("{}", report);
        }
    }
}
