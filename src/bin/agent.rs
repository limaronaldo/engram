//! Engram Autonomous Memory Agent
//!
//! Command-line interface for running the autonomous memory agent,
//! previewing garden maintenance, and getting acquisition suggestions.

use clap::{Parser, Subcommand};

use engram::intelligence::agent_loop::{AgentConfig, MemoryAgent};
use engram::intelligence::gardening::{GardenConfig, MemoryGardener};
use engram::intelligence::proactive::GapDetector;
use engram::storage::Storage;
use engram::types::{StorageConfig, StorageMode};

#[derive(Parser)]
#[command(name = "engram-agent", about = "Engram autonomous memory agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run one agent cycle (observe → decide → act).
    Run {
        /// Workspace to operate on.
        #[arg(long, default_value = "default")]
        workspace: String,
        /// Check interval in seconds (informational; this runs one cycle only).
        #[arg(long, default_value = "300")]
        interval: u64,
        /// Path to the SQLite database file.
        #[arg(long, env = "ENGRAM_DB_PATH")]
        db_path: Option<String>,
    },
    /// Show the current agent status for a workspace.
    Status {
        /// Workspace to inspect.
        #[arg(long, default_value = "default")]
        workspace: String,
        /// Path to the SQLite database file.
        #[arg(long, env = "ENGRAM_DB_PATH")]
        db_path: Option<String>,
    },
    /// Run (or preview) memory garden maintenance.
    Garden {
        /// Workspace to garden.
        #[arg(long, default_value = "default")]
        workspace: String,
        /// Dry-run: show what would be done without making changes.
        #[arg(long)]
        preview: bool,
        /// Path to the SQLite database file.
        #[arg(long, env = "ENGRAM_DB_PATH")]
        db_path: Option<String>,
    },
    /// Suggest new memories to create based on knowledge gap analysis.
    Suggest {
        /// Workspace to analyse.
        #[arg(long, default_value = "default")]
        workspace: String,
        /// Maximum number of suggestions to return.
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Path to the SQLite database file.
        #[arg(long, env = "ENGRAM_DB_PATH")]
        db_path: Option<String>,
    },
}

fn resolve_db_path(db_path: Option<String>) -> String {
    db_path.unwrap_or_else(|| {
        std::env::var("ENGRAM_DB_PATH")
            .unwrap_or_else(|_| "~/.local/share/engram/memories.db".to_string())
    })
}

fn open_storage(db_path: &str) -> Result<Storage, Box<dyn std::error::Error>> {
    let expanded = shellexpand::tilde(db_path).to_string();
    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&expanded).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let config = StorageConfig {
        db_path: expanded,
        storage_mode: StorageMode::Local,
        cloud_uri: None,
        encrypt_cloud: false,
        confidence_half_life_days: 30.0,
        auto_sync: false,
        sync_debounce_ms: 5000,
    };
    Ok(Storage::open(config)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            workspace,
            interval,
            db_path,
        } => {
            let path = resolve_db_path(db_path);
            let storage = open_storage(&path)?;

            println!("engram-agent: running one cycle for workspace '{}'", workspace);
            println!("  database: {}", path);
            println!("  interval: {}s (single cycle mode)", interval);

            let config = AgentConfig {
                workspace: workspace.clone(),
                check_interval_secs: interval,
                ..AgentConfig::default()
            };
            let mut agent = MemoryAgent::new(config);
            agent.start();

            storage.with_connection(|conn| {
                let result = agent.tick(conn)?;

                println!("\nCycle {} completed in {}ms", result.cycle_number, result.duration_ms);
                println!("Actions decided: {}", result.actions.len());

                for action in &result.actions {
                    println!("  {:?}", action);
                }

                let metrics = agent.metrics();
                println!("\nMetrics:");
                println!("  cycles: {}", metrics.cycles);
                println!("  total_actions: {}", metrics.total_actions);
                println!("  memories_pruned: {}", metrics.memories_pruned);
                println!("  memories_merged: {}", metrics.memories_merged);
                println!("  memories_archived: {}", metrics.memories_archived);
                println!("  suggestions_made: {}", metrics.suggestions_made);

                Ok(())
            })?;
        }

        Commands::Status { workspace, db_path } => {
            let path = resolve_db_path(db_path);
            let storage = open_storage(&path)?;

            println!("engram-agent status — workspace: '{}'", workspace);
            println!("  database: {}", path);

            storage.with_connection(|conn| {
                let gap_detector = GapDetector::new();
                let coverage = gap_detector.analyze_coverage(conn, &workspace)?;

                println!("\nCoverage report:");
                println!("  total_memories: {}", coverage.total_memories);
                println!("  topic_count: {}", coverage.topic_distribution.len());
                println!("  temporal_gaps: {}", coverage.temporal_gaps.len());
                println!("  weak_areas: {}", coverage.weak_areas.len());

                Ok(())
            })?;
        }

        Commands::Garden {
            workspace,
            preview,
            db_path,
        } => {
            let path = resolve_db_path(db_path);
            let storage = open_storage(&path)?;

            if preview {
                println!("engram-agent garden preview — workspace: '{}' (dry-run)", workspace);
            } else {
                println!("engram-agent garden — workspace: '{}'", workspace);
            }
            println!("  database: {}", path);

            storage.with_connection(|conn| {
                let config = GardenConfig {
                    dry_run: preview,
                    ..GardenConfig::default()
                };
                let gardener = MemoryGardener::new(config);
                let report = gardener.garden(conn, &workspace)?;

                let label = if preview { "would " } else { "" };
                println!("\nGarden report:");
                println!("  memories {}pruned: {}", label, report.memories_pruned);
                println!("  memories {}merged: {}", label, report.memories_merged);
                println!("  memories {}archived: {}", label, report.memories_archived);
                println!("  memories {}compressed: {}", label, report.memories_compressed);
                println!("  tokens {}freed: {}", label, report.tokens_freed);
                println!("  actions: {}", report.actions.len());

                Ok(())
            })?;
        }

        Commands::Suggest {
            workspace,
            limit,
            db_path,
        } => {
            let path = resolve_db_path(db_path);
            let storage = open_storage(&path)?;

            println!("engram-agent suggest — workspace: '{}'", workspace);
            println!("  database: {}", path);
            println!("  limit: {}", limit);

            storage.with_connection(|conn| {
                let detector = GapDetector::new();
                let suggestions = detector.suggest_acquisitions(conn, &workspace, limit)?;

                println!("\nAcquisition suggestions ({}):", suggestions.len());
                for (i, s) in suggestions.iter().enumerate() {
                    println!("\n  [{}] priority={}", i + 1, s.priority);
                    println!("      type: {}", s.suggested_type);
                    println!("      hint: {}", s.content_hint);
                    println!("      reason: {}", s.reason);
                }

                if suggestions.is_empty() {
                    println!("  No gaps detected — workspace looks well-covered.");
                }

                Ok(())
            })?;
        }
    }

    Ok(())
}
