//! Engram MCP Server
//!
//! Run with: engram-server

use std::sync::Arc;

use clap::Parser;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use engram::embedding::create_embedder;
use engram::error::Result;
use engram::graph::KnowledgeGraph;
use engram::intelligence::{DocumentFormat, DocumentIngestor, IngestConfig, ProjectContextEngine};
use engram::mcp::{
    get_tool_definitions, methods, InitializeResult, McpHandler, McpRequest, McpResponse,
    McpServer, ToolCallResult,
};
use engram::search::{hybrid_search, FuzzyEngine, SearchConfig};
use engram::storage::queries::*;
use engram::storage::Storage;
use engram::sync::get_sync_status;
use engram::types::*;

#[derive(Parser, Debug)]
#[command(name = "engram-server")]
#[command(about = "Engram MCP server for AI memory")]
struct Args {
    /// Database path
    #[arg(
        long,
        env = "ENGRAM_DB_PATH",
        default_value = "~/.local/share/engram/memories.db"
    )]
    db_path: String,

    /// Storage mode (local or cloud-safe)
    #[arg(long, env = "ENGRAM_STORAGE_MODE", default_value = "local")]
    storage_mode: String,

    /// Cloud storage URI (s3://bucket/path)
    #[arg(long, env = "ENGRAM_STORAGE_URI")]
    cloud_uri: Option<String>,

    /// Enable cloud encryption
    #[arg(long, env = "ENGRAM_CLOUD_ENCRYPT")]
    encrypt: bool,

    /// Embedding model (openai, tfidf)
    #[arg(long, env = "ENGRAM_EMBEDDING_MODEL", default_value = "tfidf")]
    embedding_model: String,

    /// OpenAI API key
    #[arg(long, env = "OPENAI_API_KEY")]
    openai_key: Option<String>,

    /// Sync debounce in ms
    #[arg(long, env = "ENGRAM_SYNC_DEBOUNCE_MS", default_value = "5000")]
    sync_debounce_ms: u64,

    /// Confidence decay half-life in days
    #[arg(long, env = "ENGRAM_CONFIDENCE_HALF_LIFE", default_value = "30")]
    half_life_days: f32,

    /// Memory cleanup interval in seconds (0 = disabled)
    /// When enabled, expired memories are automatically cleaned up at this interval
    #[arg(long, env = "ENGRAM_CLEANUP_INTERVAL", default_value = "3600")]
    cleanup_interval_seconds: u64,
}

/// MCP request handler
struct EngramHandler {
    storage: Storage,
    embedder: Arc<dyn engram::embedding::Embedder>,
    fuzzy_engine: Arc<Mutex<FuzzyEngine>>,
    search_config: SearchConfig,
}

impl EngramHandler {
    fn new(storage: Storage, embedder: Arc<dyn engram::embedding::Embedder>) -> Self {
        Self {
            storage,
            embedder,
            fuzzy_engine: Arc::new(Mutex::new(FuzzyEngine::new())),
            search_config: SearchConfig::default(),
        }
    }

    fn handle_tool_call(&self, name: &str, params: Value) -> Value {
        match name {
            "memory_create" => self.tool_memory_create(params),
            "memory_get" => self.tool_memory_get(params),
            "memory_update" => self.tool_memory_update(params),
            "memory_delete" => self.tool_memory_delete(params),
            "memory_list" => self.tool_memory_list(params),
            "memory_search" => self.tool_memory_search(params),
            "memory_link" => self.tool_memory_link(params),
            "memory_unlink" => self.tool_memory_unlink(params),
            "memory_related" => self.tool_memory_related(params),
            "memory_stats" => self.tool_memory_stats(params),
            "memory_versions" => self.tool_memory_versions(params),
            "memory_search_suggest" => self.tool_search_suggest(params),
            "memory_export_graph" => self.tool_export_graph(params),
            "memory_create_todo" => self.tool_create_todo(params),
            "memory_create_issue" => self.tool_create_issue(params),
            "memory_sync_status" => self.tool_sync_status(params),
            "memory_scan_project" => self.tool_scan_project(params),
            "memory_get_project_context" => self.tool_get_project_context(params),
            "memory_ingest_document" => self.tool_ingest_document(params),
            // Entity tools (RML-925)
            "memory_extract_entities" => self.tool_extract_entities(params),
            "memory_get_entities" => self.tool_get_entities(params),
            "memory_search_entities" => self.tool_search_entities(params),
            "memory_entity_stats" => self.tool_entity_stats(params),
            // Graph traversal tools (RML-926)
            "memory_traverse" => self.tool_memory_traverse(params),
            "memory_find_path" => self.tool_find_path(params),
            // TTL / Expiration tools (RML-930)
            "memory_set_expiration" => self.tool_set_expiration(params),
            "memory_cleanup_expired" => self.tool_cleanup_expired(params),
            _ => json!({"error": format!("Unknown tool: {}", name)}),
        }
    }

    fn tool_memory_create(&self, params: Value) -> Value {
        let input: CreateMemoryInput = match serde_json::from_value(params) {
            Ok(i) => i,
            Err(e) => return json!({"error": e.to_string()}),
        };

        self.storage
            .with_transaction(|conn| {
                let memory = create_memory(conn, &input)?;

                // Add to fuzzy engine vocabulary
                let mut fuzzy = self.fuzzy_engine.lock();
                fuzzy.add_to_vocabulary(&memory.content);

                Ok(json!(memory))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_get(&self, params: Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);

        self.storage
            .with_connection(|conn| {
                let memory = get_memory(conn, id)?;
                Ok(json!(memory))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_update(&self, params: Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let input: UpdateMemoryInput = match serde_json::from_value(params) {
            Ok(i) => i,
            Err(e) => return json!({"error": e.to_string()}),
        };

        self.storage
            .with_transaction(|conn| {
                let memory = update_memory(conn, id, &input)?;
                Ok(json!(memory))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_delete(&self, params: Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);

        self.storage
            .with_transaction(|conn| {
                delete_memory(conn, id)?;
                Ok(json!({"deleted": id}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_list(&self, params: Value) -> Value {
        let options: ListOptions = serde_json::from_value(params).unwrap_or_default();

        self.storage
            .with_connection(|conn| {
                let memories = list_memories(conn, &options)?;
                Ok(json!(memories))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_search(&self, params: Value) -> Value {
        use engram::search::{RerankConfig, RerankStrategy, Reranker};

        let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let options: SearchOptions = serde_json::from_value(params.clone()).unwrap_or_default();

        // Reranking options
        let rerank_enabled = params
            .get("rerank")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let rerank_strategy = match params.get("rerank_strategy").and_then(|v| v.as_str()) {
            Some("none") => RerankStrategy::None,
            Some("multi_signal") => RerankStrategy::MultiSignal,
            _ => RerankStrategy::Heuristic,
        };

        // Generate query embedding
        let query_embedding = self.embedder.embed(query).ok();
        let embedding_ref = query_embedding.as_deref();

        // Set project context path from current working directory for boost
        let mut search_config = self.search_config.clone();
        if let Ok(cwd) = std::env::current_dir() {
            if let Ok(canonical) = cwd.canonicalize() {
                search_config.project_context_path = Some(canonical.to_string_lossy().to_string());
            }
        }

        self.storage
            .with_connection(|conn| {
                let results = hybrid_search(conn, query, embedding_ref, &options, &search_config)?;

                // Apply reranking if enabled
                if rerank_enabled && rerank_strategy != RerankStrategy::None {
                    let config = RerankConfig {
                        enabled: true,
                        strategy: rerank_strategy,
                        ..Default::default()
                    };
                    let reranker = Reranker::with_config(config);
                    let reranked = reranker.rerank(results, query, None);

                    // Return reranked results with info if explain is enabled
                    if options.explain {
                        Ok(json!({
                            "results": reranked.iter().map(|r| {
                                json!({
                                    "memory": r.result.memory,
                                    "score": r.rerank_info.final_score,
                                    "match_info": r.result.match_info,
                                    "rerank_info": r.rerank_info
                                })
                            }).collect::<Vec<_>>(),
                            "reranked": true,
                            "strategy": format!("{:?}", rerank_strategy)
                        }))
                    } else {
                        // Return simplified results
                        Ok(json!(reranked
                            .iter()
                            .map(|r| {
                                json!({
                                    "memory": r.result.memory,
                                    "score": r.rerank_info.final_score,
                                    "match_info": r.result.match_info
                                })
                            })
                            .collect::<Vec<_>>()))
                    }
                } else {
                    Ok(json!(results))
                }
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_link(&self, params: Value) -> Value {
        let input: CreateCrossRefInput = match serde_json::from_value(params) {
            Ok(i) => i,
            Err(e) => return json!({"error": e.to_string()}),
        };

        self.storage
            .with_transaction(|conn| {
                let crossref = create_crossref(conn, &input)?;
                Ok(json!(crossref))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_unlink(&self, params: Value) -> Value {
        let from_id = params.get("from_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let to_id = params.get("to_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let edge_type_str = params
            .get("edge_type")
            .and_then(|v| v.as_str())
            .unwrap_or("related_to");
        let edge_type: EdgeType = edge_type_str.parse().unwrap_or_default();

        self.storage
            .with_transaction(|conn| {
                delete_crossref(conn, from_id, to_id, edge_type)?;
                Ok(json!({"unlinked": true}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_related(&self, params: Value) -> Value {
        use engram::storage::graph_queries::{get_related_multi_hop, TraversalOptions};

        let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let depth = params.get("depth").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let include_entities = params
            .get("include_entities")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let include_decayed = params
            .get("include_decayed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let edge_type = params
            .get("edge_type")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<EdgeType>().ok());

        // For backward compatibility, depth=1 uses the simple get_related
        if depth <= 1 && !include_entities && !include_decayed {
            return self
                .storage
                .with_connection(|conn| {
                    let mut related = get_related(conn, id)?;
                    if let Some(edge_type) = edge_type {
                        related.retain(|r| r.edge_type == edge_type);
                    }
                    Ok(json!(related))
                })
                .unwrap_or_else(|e| json!({"error": e.to_string()}));
        }

        // Use multi-hop traversal for depth > 1 or when entities are included
        let options = TraversalOptions {
            depth,
            edge_types: edge_type.map(|t| vec![t]).unwrap_or_default(),
            include_entities,
            ..Default::default()
        };

        self.storage
            .with_connection(|conn| {
                if include_decayed && depth <= 1 && !include_entities {
                    use engram::storage::{get_related_with_decay, DEFAULT_HALF_LIFE_DAYS};

                    let mut results =
                        get_related_with_decay(conn, id, DEFAULT_HALF_LIFE_DAYS, 0.0)?;
                    if let Some(edge_type) = edge_type {
                        let edge_type = edge_type.as_str();
                        results.retain(|r| r.edge_type == edge_type);
                    }
                    Ok(json!(results))
                } else {
                    let result = get_related_multi_hop(conn, id, &options)?;
                    Ok(json!(result))
                }
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_stats(&self, _params: Value) -> Value {
        self.storage
            .with_connection(|conn| {
                let stats = get_stats(conn)?;
                Ok(json!(stats))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_versions(&self, params: Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);

        self.storage
            .with_connection(|conn| {
                let versions = get_memory_versions(conn, id)?;
                Ok(json!(versions))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_search_suggest(&self, params: Value) -> Value {
        let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");

        let fuzzy = self.fuzzy_engine.lock();
        let result = fuzzy.correct_query(query);
        json!(result)
    }

    fn tool_export_graph(&self, params: Value) -> Value {
        let format = params
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("html");
        let max_nodes = params
            .get("max_nodes")
            .and_then(|v| v.as_i64())
            .unwrap_or(500);

        self.storage
            .with_connection(|conn| {
                let options = ListOptions {
                    limit: Some(max_nodes),
                    ..Default::default()
                };
                let memories = list_memories(conn, &options)?;

                // Get all cross-references for these memories
                let mut all_crossrefs = Vec::new();
                for memory in &memories {
                    if let Ok(refs) = get_related(conn, memory.id) {
                        all_crossrefs.extend(refs);
                    }
                }

                let graph = KnowledgeGraph::from_data(&memories, &all_crossrefs);

                match format {
                    "json" => Ok(graph.to_visjs_json()),
                    _ => Ok(json!({"html": graph.to_html()})),
                }
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_create_todo(&self, params: Value) -> Value {
        let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let priority = params
            .get("priority")
            .and_then(|v| v.as_str())
            .unwrap_or("medium");
        let tags: Vec<String> = params
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("priority".to_string(), json!(priority));
        if let Some(due) = params.get("due_date") {
            metadata.insert("due_date".to_string(), due.clone());
        }

        let importance = match priority {
            "critical" => 1.0,
            "high" => 0.8,
            "medium" => 0.5,
            "low" => 0.3,
            _ => 0.5,
        };

        let input = CreateMemoryInput {
            content: content.to_string(),
            memory_type: MemoryType::Todo,
            tags,
            metadata,
            importance: Some(importance),
            scope: Default::default(),
            defer_embedding: false,
            ttl_seconds: None,
        };

        self.tool_memory_create(json!(input))
    }

    fn tool_create_issue(&self, params: Value) -> Value {
        let title = params.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let description = params
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let severity = params
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("medium");
        let tags: Vec<String> = params
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let content = if description.is_empty() {
            title.to_string()
        } else {
            format!("{}\n\n{}", title, description)
        };

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("severity".to_string(), json!(severity));
        metadata.insert("title".to_string(), json!(title));

        let importance = match severity {
            "critical" => 1.0,
            "high" => 0.8,
            "medium" => 0.5,
            "low" => 0.3,
            _ => 0.5,
        };

        let input = CreateMemoryInput {
            content,
            memory_type: MemoryType::Issue,
            tags,
            metadata,
            importance: Some(importance),
            scope: Default::default(),
            defer_embedding: false,
            ttl_seconds: None,
        };

        self.tool_memory_create(json!(input))
    }

    fn tool_sync_status(&self, _params: Value) -> Value {
        self.storage
            .with_connection(|conn| {
                let status = get_sync_status(conn)?;
                Ok(json!(status))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_scan_project(&self, params: Value) -> Value {
        use chrono::Utc;
        use engram::intelligence::{ProjectContextConfig, ScanResult};
        use std::collections::HashSet;
        use std::path::PathBuf;

        // Get scan path (default to current working directory) and canonicalize
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".to_string())
            });

        let path = PathBuf::from(&path_str);
        // Canonicalize path for consistent matching
        let canonical_path = path.canonicalize().unwrap_or(path.clone());
        let canonical_path_str = canonical_path.to_string_lossy().to_string();

        // Get options
        let scan_parents = params
            .get("scan_parents")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let extract_sections = params
            .get("extract_sections")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Create engine with custom config if needed
        let config = ProjectContextConfig {
            scan_parents,
            extract_sections,
            ..Default::default()
        };
        let engine = ProjectContextEngine::with_config(config);

        // Scan for instruction files
        let (discovered, files_skipped) = match engine.scan_directory_with_stats(&canonical_path) {
            Ok(result) => result,
            Err(e) => return json!({"error": format!("Scan failed: {}", e)}),
        };

        let mut result = ScanResult {
            project_path: canonical_path_str.clone(),
            files_found: discovered.len(),
            memories_created: 0,
            memories_updated: 0,
            files_skipped,
            errors: Vec::new(),
            scanned_at: Utc::now(),
        };

        // Process each discovered file
        for file in &discovered {
            // Canonicalize file path for consistent matching
            let file_path_canonical = file
                .path
                .canonicalize()
                .unwrap_or_else(|_| file.path.clone())
                .to_string_lossy()
                .to_string();

            // Query for existing parent memory by source_file metadata
            let mut filter = std::collections::HashMap::new();
            filter.insert(
                "source_file".to_string(),
                json!(file_path_canonical.clone()),
            );

            let existing_parent = self.storage.with_connection(|conn| {
                let options = ListOptions {
                    metadata_filter: Some(filter),
                    limit: Some(1),
                    ..Default::default()
                };
                let memories = list_memories(conn, &options)?;
                Ok(memories.into_iter().next())
            });

            // Determine if we need to process sections
            let mut sections_to_process = Vec::new();

            if extract_sections {
                match engine.parse_file(file) {
                    Ok(parsed) => {
                        for section in parsed.sections {
                            if !section.content.trim().is_empty() {
                                sections_to_process.push(section);
                            }
                        }
                    }
                    Err(e) => {
                        result
                            .errors
                            .push(format!("Failed to parse {}: {}", file.filename, e));
                    }
                }
            }

            let parent_id = match existing_parent {
                Ok(Some(existing_memory)) => {
                    // Check if file has changed (by hash)
                    let existing_hash = existing_memory
                        .metadata
                        .get("file_hash")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    if existing_hash == file.content_hash {
                        // File unchanged - but still need to ensure sections exist
                        // (handles case where previous scan had extract_sections=false)
                        existing_memory.id
                    } else {
                        // File changed - update parent memory
                        let memory = engine.file_to_memory(file);
                        let mut metadata = memory.metadata.clone();
                        // Use canonical path
                        metadata.insert(
                            "source_file".to_string(),
                            json!(file_path_canonical.clone()),
                        );
                        metadata.insert(
                            "project_path".to_string(),
                            json!(canonical_path_str.clone()),
                        );

                        let update_input = UpdateMemoryInput {
                            content: Some(memory.content),
                            memory_type: Some(memory.memory_type),
                            tags: Some(memory.tags),
                            metadata: Some(metadata),
                            importance: Some(memory.importance),
                            scope: None,
                            ttl_seconds: None,
                        };

                        match self.storage.with_transaction(|conn| {
                            update_memory(conn, existing_memory.id, &update_input)
                        }) {
                            Ok(updated) => {
                                result.memories_updated += 1;
                                updated.id
                            }
                            Err(e) => {
                                result
                                    .errors
                                    .push(format!("Failed to update {}: {}", file.filename, e));
                                continue;
                            }
                        }
                    }
                }
                Ok(None) => {
                    // Create new parent memory
                    let memory = engine.file_to_memory(file);
                    let mut metadata = memory.metadata.clone();
                    // Use canonical paths
                    metadata.insert(
                        "source_file".to_string(),
                        json!(file_path_canonical.clone()),
                    );
                    metadata.insert(
                        "project_path".to_string(),
                        json!(canonical_path_str.clone()),
                    );

                    let input = CreateMemoryInput {
                        content: memory.content,
                        memory_type: memory.memory_type,
                        tags: memory.tags,
                        metadata,
                        importance: Some(memory.importance),
                        scope: Default::default(),
                        defer_embedding: false,
                        ttl_seconds: None,
                    };

                    match self
                        .storage
                        .with_transaction(|conn| create_memory(conn, &input))
                    {
                        Ok(created) => {
                            result.memories_created += 1;
                            created.id
                        }
                        Err(e) => {
                            result.errors.push(format!(
                                "Failed to create memory for {}: {}",
                                file.filename, e
                            ));
                            continue;
                        }
                    }
                }
                Err(e) => {
                    result
                        .errors
                        .push(format!("Failed to check existing: {}", e));
                    continue;
                }
            };

            // Process section memories if enabled
            if extract_sections && !sections_to_process.is_empty() {
                // Get existing sections for this parent
                let mut section_filter = std::collections::HashMap::new();
                section_filter.insert(
                    "source_file".to_string(),
                    json!(file_path_canonical.clone()),
                );

                let existing_sections = self
                    .storage
                    .with_connection(|conn| {
                        let options = ListOptions {
                            tags: Some(vec!["section".to_string()]),
                            metadata_filter: Some(section_filter),
                            limit: Some(1000),
                            ..Default::default()
                        };
                        list_memories(conn, &options)
                    })
                    .unwrap_or_default();

                // Build lookup map for existing sections by path
                let existing_sections_by_path: std::collections::HashMap<String, &Memory> =
                    existing_sections
                        .iter()
                        .filter_map(|mem| {
                            mem.metadata
                                .get("section_path")
                                .and_then(|v| v.as_str())
                                .map(|path| (path.to_string(), mem))
                        })
                        .collect();

                // Track which section paths we're processing
                let mut processed_section_paths: HashSet<String> = HashSet::new();

                for section in &sections_to_process {
                    processed_section_paths.insert(section.section_path.clone());

                    // Check if section already exists
                    let existing_section = existing_sections_by_path.get(&section.section_path);

                    if let Some(existing) = existing_section {
                        // Check if content changed
                        let existing_hash = existing
                            .metadata
                            .get("content_hash")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        if existing_hash == section.content_hash {
                            // No change, skip
                            continue;
                        }

                        // Update existing section
                        let section_memory = engine.section_to_memory(section, file, parent_id);
                        let mut metadata = section_memory.metadata.clone();
                        metadata.insert(
                            "source_file".to_string(),
                            json!(file_path_canonical.clone()),
                        );
                        metadata.insert(
                            "project_path".to_string(),
                            json!(canonical_path_str.clone()),
                        );

                        let update_input = UpdateMemoryInput {
                            content: Some(section_memory.content),
                            memory_type: Some(section_memory.memory_type),
                            tags: Some(section_memory.tags),
                            metadata: Some(metadata),
                            importance: Some(section_memory.importance),
                            scope: None,
                            ttl_seconds: None,
                        };

                        match self.storage.with_transaction(|conn| {
                            update_memory(conn, existing.id, &update_input)
                        }) {
                            Ok(_) => result.memories_updated += 1,
                            Err(e) => {
                                result.errors.push(format!(
                                    "Failed to update section '{}': {}",
                                    section.title, e
                                ));
                            }
                        }
                    } else {
                        // Create new section
                        let section_memory = engine.section_to_memory(section, file, parent_id);
                        let mut metadata = section_memory.metadata.clone();
                        metadata.insert(
                            "source_file".to_string(),
                            json!(file_path_canonical.clone()),
                        );
                        metadata.insert(
                            "project_path".to_string(),
                            json!(canonical_path_str.clone()),
                        );

                        let input = CreateMemoryInput {
                            content: section_memory.content,
                            memory_type: section_memory.memory_type,
                            tags: section_memory.tags,
                            metadata,
                            importance: Some(section_memory.importance),
                            scope: Default::default(),
                            defer_embedding: false,
                            ttl_seconds: None,
                        };

                        match self
                            .storage
                            .with_transaction(|conn| create_memory(conn, &input))
                        {
                            Ok(_) => result.memories_created += 1,
                            Err(e) => {
                                result.errors.push(format!(
                                    "Failed to create section '{}': {}",
                                    section.title, e
                                ));
                            }
                        }
                    }
                }

                // Delete stale sections (sections that exist in DB but not in current file)
                for (path, existing) in &existing_sections_by_path {
                    if !processed_section_paths.contains(path) {
                        // Section no longer exists in file, delete it
                        match self
                            .storage
                            .with_transaction(|conn| delete_memory(conn, existing.id))
                        {
                            Ok(_) => {
                                tracing::info!("Deleted stale section: {}", path);
                            }
                            Err(e) => {
                                result.errors.push(format!(
                                    "Failed to delete stale section '{}': {}",
                                    path, e
                                ));
                            }
                        }
                    }
                }
            }
        }

        json!(result)
    }

    fn tool_get_project_context(&self, params: Value) -> Value {
        use std::path::PathBuf;

        // Get path (default to current working directory) and canonicalize
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".to_string())
            });

        // Canonicalize path for consistent matching
        let path = PathBuf::from(&path_str);
        let canonical_path_str = path
            .canonicalize()
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let include_sections = params
            .get("include_sections")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let file_types: Option<Vec<String>> = params
            .get("file_types")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        // Query memories with project-context tag and matching project_path using metadata filter
        let mut filter = std::collections::HashMap::new();
        filter.insert(
            "project_path".to_string(),
            json!(canonical_path_str.clone()),
        );

        self.storage
            .with_connection(|conn| {
                let options = ListOptions {
                    limit: Some(1000), // Reasonable limit for project context
                    tags: Some(vec!["project-context".to_string()]),
                    metadata_filter: Some(filter),
                    ..Default::default()
                };
                let all_memories = list_memories(conn, &options)?;

                // Filter by sections and file type
                let filtered: Vec<_> = all_memories
                    .into_iter()
                    .filter(|m| {
                        // Filter out sections if not requested
                        if !include_sections && m.tags.contains(&"section".to_string()) {
                            return false;
                        }

                        // Filter by file type if specified
                        if let Some(ref types) = file_types {
                            let file_type = m
                                .metadata
                                .get("file_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if !types.iter().any(|t| t == file_type) {
                                return false;
                            }
                        }

                        true
                    })
                    .collect();

                Ok(json!({
                    "project_path": canonical_path_str,
                    "count": filtered.len(),
                    "memories": filtered
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_ingest_document(&self, params: Value) -> Value {
        use serde::Deserialize;

        #[derive(Debug, Deserialize)]
        struct IngestParams {
            path: String,
            format: Option<String>,
            chunk_size: Option<usize>,
            overlap: Option<usize>,
            max_file_size: Option<u64>,
            tags: Option<Vec<String>>,
        }

        let input: IngestParams = match serde_json::from_value(params) {
            Ok(i) => i,
            Err(e) => return json!({"error": e.to_string()}),
        };

        let format = match input.format.as_deref() {
            None | Some("auto") => None,
            Some("md") | Some("markdown") => Some(DocumentFormat::Markdown),
            Some("pdf") => Some(DocumentFormat::Pdf),
            Some(other) => {
                return json!({"error": format!("Invalid format: {}", other)});
            }
        };

        let default_config = IngestConfig::default();
        let config = IngestConfig {
            format,
            chunk_size: input.chunk_size.unwrap_or(default_config.chunk_size),
            overlap: input.overlap.unwrap_or(default_config.overlap),
            max_file_size: input.max_file_size.unwrap_or(default_config.max_file_size),
            extra_tags: input.tags.unwrap_or_default(),
        };

        let ingestor = DocumentIngestor::new(&self.storage);
        match ingestor.ingest_file(&input.path, config) {
            Ok(result) => json!(result),
            Err(e) => json!({"error": e.to_string()}),
        }
    }

    // =========================================================================
    // Entity Tools (RML-925)
    // =========================================================================

    /// Extract entities from a memory's content and store them
    fn tool_extract_entities(&self, params: Value) -> Value {
        use engram::intelligence::{EntityExtractionConfig, EntityExtractor};
        use engram::storage::{link_entity_to_memory, upsert_entity};

        let memory_id = match params
            .get("memory_id")
            .or_else(|| params.get("id"))
            .and_then(|v| v.as_i64())
        {
            Some(id) => id,
            None => return json!({"error": "memory_id (or id) is required"}),
        };

        // Optional: custom confidence threshold
        let min_confidence = params
            .get("min_confidence")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32)
            .unwrap_or(0.5);

        self.storage
            .with_transaction(|conn| {
                // Get the memory
                let memory = get_memory(conn, memory_id)?;

                // Configure and run extraction
                let config = EntityExtractionConfig {
                    min_confidence,
                    ..Default::default()
                };
                let extractor = EntityExtractor::new(config);
                let result = extractor.extract(&memory.content);

                // Store entities and link them
                let mut stored_entities = Vec::new();
                for extracted in &result.entities {
                    let entity_id = upsert_entity(conn, extracted)?;
                    let _inserted = link_entity_to_memory(
                        conn,
                        memory_id,
                        entity_id,
                        extracted.suggested_relation,
                        extracted.confidence,
                        Some(extracted.offset),
                    )?;

                    stored_entities.push(json!({
                        "entity_id": entity_id,
                        "text": extracted.text,
                        "type": extracted.entity_type.as_str(),
                        "confidence": extracted.confidence,
                        "relation": extracted.suggested_relation.as_str(),
                    }));
                }

                Ok(json!({
                    "memory_id": memory_id,
                    "entities_found": result.entities.len(),
                    "extraction_time_ms": result.extraction_time_ms,
                    "entities": stored_entities
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    /// Get entities linked to a memory
    fn tool_get_entities(&self, params: Value) -> Value {
        use engram::storage::get_entities_for_memory;

        let memory_id = match params
            .get("memory_id")
            .or_else(|| params.get("id"))
            .and_then(|v| v.as_i64())
        {
            Some(id) => id,
            None => return json!({"error": "memory_id (or id) is required"}),
        };

        self.storage
            .with_connection(|conn| {
                let entities = get_entities_for_memory(conn, memory_id)?;

                let result: Vec<_> = entities
                    .into_iter()
                    .map(|(entity, relation, confidence)| {
                        json!({
                            "id": entity.id,
                            "name": entity.name,
                            "type": entity.entity_type.as_str(),
                            "mention_count": entity.mention_count,
                            "relation": relation.as_str(),
                            "confidence": confidence,
                        })
                    })
                    .collect();

                Ok(json!({
                    "memory_id": memory_id,
                    "count": result.len(),
                    "entities": result
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    /// Search for entities by name
    fn tool_search_entities(&self, params: Value) -> Value {
        use engram::intelligence::EntityType;
        use engram::storage::search_entities;

        let query = match params.get("query").and_then(|v| v.as_str()) {
            Some(q) => q,
            None => return json!({"error": "query is required"}),
        };

        let entity_type: Option<EntityType> = params
            .get("entity_type")
            .or_else(|| params.get("type"))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok());

        let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);

        self.storage
            .with_connection(|conn| {
                let entities = search_entities(conn, query, entity_type, limit)?;

                let result: Vec<_> = entities
                    .into_iter()
                    .map(|entity| {
                        json!({
                            "id": entity.id,
                            "name": entity.name,
                            "normalized_name": entity.normalized_name,
                            "type": entity.entity_type.as_str(),
                            "mention_count": entity.mention_count,
                            "created_at": entity.created_at.to_rfc3339(),
                        })
                    })
                    .collect();

                Ok(json!({
                    "query": query,
                    "count": result.len(),
                    "entities": result
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    /// Get entity statistics
    fn tool_entity_stats(&self, _params: Value) -> Value {
        use engram::storage::get_entity_stats;

        self.storage
            .with_connection(|conn| {
                let stats = get_entity_stats(conn)?;
                Ok(json!(stats))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Graph Traversal Tools (RML-926)
    // =========================================================================

    fn tool_memory_traverse(&self, params: Value) -> Value {
        use engram::storage::graph_queries::{
            get_related_multi_hop, TraversalDirection, TraversalOptions,
        };
        use engram::types::EdgeType;

        let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let depth = params.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as usize;
        let include_entities = params
            .get("include_entities")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let min_score = params
            .get("min_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        let min_confidence = params
            .get("min_confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        let limit_per_hop = params
            .get("limit_per_hop")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as usize;

        // Parse direction
        let direction = match params.get("direction").and_then(|v| v.as_str()) {
            Some("outgoing") => TraversalDirection::Outgoing,
            Some("incoming") => TraversalDirection::Incoming,
            _ => TraversalDirection::Both,
        };

        // Parse edge types filter
        let edge_types: Vec<EdgeType> = params
            .get("edge_types")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|s| s.parse().ok())
                    .collect()
            })
            .unwrap_or_default();

        let options = TraversalOptions {
            depth,
            edge_types,
            min_score,
            min_confidence,
            limit_per_hop,
            include_entities,
            direction,
        };

        self.storage
            .with_connection(|conn| {
                let result = get_related_multi_hop(conn, id, &options)?;
                Ok(json!(result))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_find_path(&self, params: Value) -> Value {
        use engram::storage::graph_queries::find_path;

        let from_id = params.get("from_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let to_id = params.get("to_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let max_depth = params
            .get("max_depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;

        self.storage
            .with_connection(|conn| {
                let path = find_path(conn, from_id, to_id, max_depth)?;
                match path {
                    Some(node) => Ok(json!({
                        "found": true,
                        "path": node.path,
                        "edge_path": node.edge_path,
                        "depth": node.depth,
                        "cumulative_score": node.cumulative_score,
                        "connection_type": node.connection_type
                    })),
                    None => Ok(json!({
                        "found": false,
                        "message": format!("No path found from {} to {} within depth {}", from_id, to_id, max_depth)
                    })),
                }
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // TTL / Expiration Tools (RML-930)

    fn tool_set_expiration(&self, params: Value) -> Value {
        use engram::storage::queries::set_memory_expiration;

        let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let ttl_seconds = params.get("ttl_seconds").and_then(|v| v.as_i64());

        if ttl_seconds.is_none() {
            return json!({"error": "ttl_seconds is required"});
        }

        self.storage
            .with_transaction(|conn| {
                let memory = set_memory_expiration(conn, id, ttl_seconds)?;
                Ok(json!({
                    "success": true,
                    "memory": memory,
                    "message": if ttl_seconds == Some(0) {
                        "Expiration removed".to_string()
                    } else {
                        format!("Expiration set to {} seconds from now", ttl_seconds.unwrap())
                    }
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_cleanup_expired(&self, params: Value) -> Value {
        use engram::storage::queries::{cleanup_expired_memories, count_expired_memories};

        let _ = params; // unused

        self.storage
            .with_transaction(|conn| {
                // First count how many will be cleaned
                let _count_before = count_expired_memories(conn)?;

                // Perform cleanup
                let deleted = cleanup_expired_memories(conn)?;

                Ok(json!({
                    "success": true,
                    "deleted": deleted,
                    "message": format!("Cleaned up {} expired memories", deleted)
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }
}

impl McpHandler for EngramHandler {
    fn handle_request(&self, request: McpRequest) -> McpResponse {
        match request.method.as_str() {
            methods::INITIALIZE => {
                let result = InitializeResult::default();
                McpResponse::success(request.id, json!(result))
            }
            methods::INITIALIZED => {
                // Notification, no response needed
                McpResponse::success(request.id, json!({}))
            }
            methods::LIST_TOOLS => {
                let tools = get_tool_definitions();
                McpResponse::success(request.id, json!({"tools": tools}))
            }
            methods::CALL_TOOL => {
                let name = request
                    .params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let arguments = request
                    .params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(json!({}));

                let result = self.handle_tool_call(name, arguments);
                let tool_result = ToolCallResult::json(&result);
                McpResponse::success(request.id, json!(tool_result))
            }
            _ => McpResponse::error(
                request.id,
                -32601,
                format!("Method not found: {}", request.method),
            ),
        }
    }
}

fn main() -> Result<()> {
    // Initialize logging to stderr (stdout is for MCP protocol)
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(false),
        )
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    // Expand ~ in path
    let db_path = shellexpand::tilde(&args.db_path).to_string();

    // Determine storage mode
    let storage_mode = match args.storage_mode.as_str() {
        "cloud-safe" => StorageMode::CloudSafe,
        _ => StorageMode::Local,
    };

    let config = StorageConfig {
        db_path,
        storage_mode,
        cloud_uri: args.cloud_uri,
        encrypt_cloud: args.encrypt,
        confidence_half_life_days: args.half_life_days,
        auto_sync: true,
        sync_debounce_ms: args.sync_debounce_ms,
    };

    // Open storage
    let storage = Storage::open(config.clone())?;

    // Check for storage mode warning
    if let Some(warning) = storage.storage_mode_warning() {
        tracing::warn!("{}", warning);
    }

    // Create embedder
    let embedding_config = EmbeddingConfig {
        model: args.embedding_model,
        api_key: args.openai_key,
        model_path: None,
        dimensions: 384,
        batch_size: 100,
    };
    let embedder = create_embedder(&embedding_config)?;

    // Create handler and server
    let handler = EngramHandler::new(storage.clone(), embedder);
    let server = McpServer::new(handler);

    // Start background cleanup thread if enabled
    if args.cleanup_interval_seconds > 0 {
        let cleanup_storage = storage.clone();
        let interval = std::time::Duration::from_secs(args.cleanup_interval_seconds);

        std::thread::spawn(move || {
            tracing::info!(
                "Memory cleanup thread started (interval: {}s)",
                interval.as_secs()
            );

            loop {
                std::thread::sleep(interval);

                match cleanup_storage.with_transaction(|conn| {
                    engram::storage::queries::cleanup_expired_memories(conn)
                }) {
                    Ok(deleted) => {
                        if deleted > 0 {
                            tracing::info!("Cleaned up {} expired memories", deleted);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error cleaning up expired memories: {}", e);
                    }
                }
            }
        });
    }

    tracing::info!("Engram MCP server starting...");
    server.run()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_handler() -> EngramHandler {
        let storage = Storage::open_in_memory().unwrap();
        let embedder = create_embedder(&EmbeddingConfig::default()).unwrap();
        EngramHandler {
            storage,
            embedder,
            fuzzy_engine: Arc::new(Mutex::new(FuzzyEngine::new())),
            search_config: SearchConfig::default(),
        }
    }

    #[test]
    fn test_tool_ingest_document_idempotent() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("doc.md");
        std::fs::write(&file_path, "# Title\n\nHello world.\n").unwrap();

        let handler = test_handler();

        let first = handler.tool_ingest_document(json!({
            "path": file_path.to_string_lossy(),
            "format": "md"
        }));
        assert!(first.get("error").is_none(), "first ingest error: {first}");
        assert!(
            first
                .get("chunks_created")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                > 0
        );

        let second = handler.tool_ingest_document(json!({
            "path": file_path.to_string_lossy(),
            "format": "md"
        }));
        assert!(
            second.get("error").is_none(),
            "second ingest error: {second}"
        );
        assert_eq!(
            second
                .get("chunks_created")
                .and_then(|v| v.as_u64())
                .unwrap_or(1),
            0
        );
    }
}
