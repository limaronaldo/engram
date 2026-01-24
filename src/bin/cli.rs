//! Engram CLI
//!
//! Command-line interface for memory management.

use std::io::{self, Write};

use clap::{Parser, Subcommand};

use engram::embedding::create_embedder;
use engram::error::Result;
use engram::graph::KnowledgeGraph;
use engram::search::{hybrid_search, SearchConfig};
use engram::storage::queries::*;
use engram::storage::Storage;
use engram::types::*;

#[derive(Parser)]
#[command(name = "engram")]
#[command(about = "AI Memory Infrastructure CLI")]
#[command(version)]
struct Cli {
    /// Database path
    #[arg(
        long,
        env = "ENGRAM_DB_PATH",
        default_value = "~/.local/share/engram/memories.db"
    )]
    db_path: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new memory
    Create {
        /// Content to remember
        content: String,
        /// Memory type
        #[arg(short, long, default_value = "note")]
        r#type: String,
        /// Tags (comma-separated)
        #[arg(short = 'T', long)]
        tags: Option<String>,
        /// Importance (0-1)
        #[arg(short, long)]
        importance: Option<f32>,
    },
    /// Get a memory by ID
    Get {
        /// Memory ID
        id: i64,
    },
    /// List memories
    List {
        /// Maximum number to return
        #[arg(short, long, default_value = "20")]
        limit: i64,
        /// Filter by tags (comma-separated)
        #[arg(short = 'T', long)]
        tags: Option<String>,
        /// Filter by type
        #[arg(short, long)]
        r#type: Option<String>,
    },
    /// Search memories
    Search {
        /// Search query
        query: String,
        /// Maximum results
        #[arg(short, long, default_value = "10")]
        limit: i64,
        /// Show match explanations
        #[arg(short, long)]
        explain: bool,
    },
    /// Delete a memory
    Delete {
        /// Memory ID
        id: i64,
    },
    /// Show statistics
    Stats,
    /// Export knowledge graph
    Graph {
        /// Output format (html, json)
        #[arg(short, long, default_value = "html")]
        format: String,
        /// Output file (- for stdout)
        #[arg(short, long, default_value = "-")]
        output: String,
        /// Maximum nodes
        #[arg(short, long, default_value = "500")]
        max_nodes: i64,
    },
    /// Link two memories
    Link {
        /// Source memory ID
        from: i64,
        /// Target memory ID
        to: i64,
        /// Relationship type
        #[arg(short, long, default_value = "related_to")]
        edge_type: String,
    },
    /// Show version history
    Versions {
        /// Memory ID
        id: i64,
    },
    /// Interactive mode
    Interactive,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Expand ~ in path
    let db_path = shellexpand::tilde(&cli.db_path).to_string();

    let config = StorageConfig {
        db_path,
        storage_mode: StorageMode::Local,
        cloud_uri: None,
        encrypt_cloud: false,
        confidence_half_life_days: 30.0,
        auto_sync: false,
        sync_debounce_ms: 5000,
    };

    let storage = Storage::open(config)?;

    match cli.command {
        Commands::Create {
            content,
            r#type,
            tags,
            importance,
        } => {
            let memory_type: MemoryType = r#type.parse().unwrap_or(MemoryType::Note);
            let tags: Vec<String> = tags
                .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_default();

            let input = CreateMemoryInput {
                content,
                memory_type,
                tags,
                metadata: Default::default(),
                importance,
                scope: Default::default(),
                defer_embedding: true,
            };

            let memory = storage.with_transaction(|conn| create_memory(conn, &input))?;
            println!("Created memory #{}", memory.id);
            println!("{}", serde_json::to_string_pretty(&memory)?);
        }

        Commands::Get { id } => {
            let memory = storage.with_connection(|conn| get_memory(conn, id))?;
            println!("{}", serde_json::to_string_pretty(&memory)?);
        }

        Commands::List {
            limit,
            tags,
            r#type,
        } => {
            let tags: Option<Vec<String>> =
                tags.map(|t| t.split(',').map(|s| s.trim().to_string()).collect());
            let memory_type = r#type.and_then(|t| t.parse().ok());

            let options = ListOptions {
                limit: Some(limit),
                tags,
                memory_type,
                ..Default::default()
            };

            let memories = storage.with_connection(|conn| list_memories(conn, &options))?;
            for memory in memories {
                println!(
                    "#{} [{}] {} - {}",
                    memory.id,
                    memory.memory_type.as_str(),
                    memory.tags.join(", "),
                    truncate(&memory.content, 60)
                );
            }
        }

        Commands::Search {
            query,
            limit,
            explain,
        } => {
            let embedding_config = EmbeddingConfig::default();
            let embedder = create_embedder(&embedding_config)?;
            let query_embedding = embedder.embed(&query).ok();

            let options = SearchOptions {
                limit: Some(limit),
                explain,
                ..Default::default()
            };

            let config = SearchConfig::default();
            let results = storage.with_connection(|conn| {
                hybrid_search(conn, &query, query_embedding.as_deref(), &options, &config)
            })?;

            for result in results {
                println!(
                    "#{} (score: {:.3}) - {}",
                    result.memory.id,
                    result.score,
                    truncate(&result.memory.content, 60)
                );
                if explain {
                    println!(
                        "  Strategy: {:?}, Matched: {:?}",
                        result.match_info.strategy, result.match_info.matched_terms
                    );
                }
            }
        }

        Commands::Delete { id } => {
            storage.with_transaction(|conn| delete_memory(conn, id))?;
            println!("Deleted memory #{}", id);
        }

        Commands::Stats => {
            let stats = storage.with_connection(get_stats)?;
            println!("{}", serde_json::to_string_pretty(&stats)?);
        }

        Commands::Graph {
            format,
            output,
            max_nodes,
        } => {
            let options = ListOptions {
                limit: Some(max_nodes),
                ..Default::default()
            };

            let (memories, crossrefs) = storage.with_connection(|conn| {
                let memories = list_memories(conn, &options)?;
                let mut all_crossrefs = Vec::new();
                for memory in &memories {
                    if let Ok(refs) = get_related(conn, memory.id) {
                        all_crossrefs.extend(refs);
                    }
                }
                Ok((memories, all_crossrefs))
            })?;

            let graph = KnowledgeGraph::from_data(&memories, &crossrefs);

            let content = match format.as_str() {
                "json" => serde_json::to_string_pretty(&graph.to_visjs_json())?,
                _ => graph.to_html(),
            };

            if output == "-" {
                println!("{}", content);
            } else {
                std::fs::write(&output, content)?;
                println!("Graph exported to {}", output);
            }
        }

        Commands::Link {
            from,
            to,
            edge_type,
        } => {
            let edge_type: EdgeType = edge_type.parse().unwrap_or(EdgeType::RelatedTo);
            let input = CreateCrossRefInput {
                from_id: from,
                to_id: to,
                edge_type,
                strength: None,
                source_context: None,
                pinned: false,
            };

            storage.with_transaction(|conn| create_crossref(conn, &input))?;
            println!("Linked #{} -> #{} ({})", from, to, edge_type.as_str());
        }

        Commands::Versions { id } => {
            let versions = storage.with_connection(|conn| get_memory_versions(conn, id))?;
            for version in versions {
                println!(
                    "v{} ({}) - {}",
                    version.version,
                    version.created_at.format("%Y-%m-%d %H:%M"),
                    truncate(&version.content, 50)
                );
            }
        }

        Commands::Interactive => {
            println!("Engram Interactive Mode");
            println!("Type 'help' for commands, 'quit' to exit\n");

            let stdin = io::stdin();
            let mut stdout = io::stdout();

            loop {
                print!("engram> ");
                stdout.flush()?;

                let mut line = String::new();
                stdin.read_line(&mut line)?;
                let line = line.trim();

                if line.is_empty() {
                    continue;
                }

                match line {
                    "quit" | "exit" => break,
                    "help" => {
                        println!("Commands:");
                        println!("  create <content>  - Create a memory");
                        println!("  get <id>          - Get memory by ID");
                        println!("  list              - List recent memories");
                        println!("  search <query>    - Search memories");
                        println!("  stats             - Show statistics");
                        println!("  quit              - Exit");
                    }
                    "stats" => {
                        let stats = storage.with_connection(get_stats)?;
                        println!("Memories: {}", stats.total_memories);
                        println!("Tags: {}", stats.total_tags);
                        println!("Cross-refs: {}", stats.total_crossrefs);
                    }
                    "list" => {
                        let options = ListOptions {
                            limit: Some(10),
                            ..Default::default()
                        };
                        let memories =
                            storage.with_connection(|conn| list_memories(conn, &options))?;
                        for memory in memories {
                            println!("#{}: {}", memory.id, truncate(&memory.content, 60));
                        }
                    }
                    _ if line.starts_with("get ") => {
                        if let Ok(id) = line[4..].trim().parse::<i64>() {
                            match storage.with_connection(|conn| get_memory(conn, id)) {
                                Ok(memory) => {
                                    println!("{}", serde_json::to_string_pretty(&memory)?);
                                }
                                Err(e) => println!("Error: {}", e),
                            }
                        } else {
                            println!("Invalid ID");
                        }
                    }
                    _ if line.starts_with("create ") => {
                        let content = line[7..].trim();
                        let input = CreateMemoryInput {
                            content: content.to_string(),
                            memory_type: MemoryType::Note,
                            tags: vec![],
                            metadata: Default::default(),
                            importance: None,
                            scope: Default::default(),
                            defer_embedding: true,
                        };
                        match storage.with_transaction(|conn| create_memory(conn, &input)) {
                            Ok(memory) => println!("Created #{}", memory.id),
                            Err(e) => println!("Error: {}", e),
                        }
                    }
                    _ if line.starts_with("search ") => {
                        let query = line[7..].trim();
                        let embedding_config = EmbeddingConfig::default();
                        let embedder = create_embedder(&embedding_config)?;
                        let query_embedding = embedder.embed(query).ok();

                        let options = SearchOptions {
                            limit: Some(5),
                            ..Default::default()
                        };
                        let config = SearchConfig::default();

                        match storage.with_connection(|conn| {
                            hybrid_search(
                                conn,
                                query,
                                query_embedding.as_deref(),
                                &options,
                                &config,
                            )
                        }) {
                            Ok(results) => {
                                for result in results {
                                    println!(
                                        "#{} ({:.2}): {}",
                                        result.memory.id,
                                        result.score,
                                        truncate(&result.memory.content, 50)
                                    );
                                }
                            }
                            Err(e) => println!("Error: {}", e),
                        }
                    }
                    _ => println!("Unknown command. Type 'help' for available commands."),
                }
            }

            println!("Goodbye!");
        }
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() <= max {
        first_line.to_string()
    } else {
        format!("{}...", &first_line[..max - 3])
    }
}
