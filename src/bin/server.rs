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
use engram::realtime::{RealtimeEvent, RealtimeManager, RealtimeServer};
use engram::search::{hybrid_search, FuzzyEngine, SearchConfig};
use engram::storage::queries::*;
use engram::storage::Storage;
#[cfg(feature = "cloud")]
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

    /// WebSocket server port for real-time events (0 = disabled)
    #[arg(long, env = "ENGRAM_WS_PORT", default_value = "0")]
    ws_port: u16,
}

/// MCP request handler
struct EngramHandler {
    storage: Storage,
    embedder: Arc<dyn engram::embedding::Embedder>,
    fuzzy_engine: Arc<Mutex<FuzzyEngine>>,
    search_config: SearchConfig,
    /// Real-time event manager for WebSocket broadcasting
    realtime: Option<RealtimeManager>,
    /// Embedding cache for performance optimization
    embedding_cache: Arc<engram::embedding::EmbeddingCache>,
}

impl EngramHandler {
    fn new(storage: Storage, embedder: Arc<dyn engram::embedding::Embedder>) -> Self {
        Self {
            storage,
            embedder,
            fuzzy_engine: Arc::new(Mutex::new(FuzzyEngine::new())),
            search_config: SearchConfig::default(),
            realtime: None,
            embedding_cache: Arc::new(engram::embedding::EmbeddingCache::default()),
        }
    }

    fn with_realtime(mut self, manager: RealtimeManager) -> Self {
        self.realtime = Some(manager);
        self
    }

    /// Broadcast a real-time event if manager is configured
    fn broadcast_event(&self, event: RealtimeEvent) {
        if let Some(ref manager) = self.realtime {
            manager.broadcast(event);
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
            // Deduplication tools (RML-931)
            "memory_find_duplicates" => self.tool_find_duplicates(params),
            // Workspace management tools
            "workspace_list" => self.tool_workspace_list(params),
            "workspace_stats" => self.tool_workspace_stats(params),
            "workspace_move" => self.tool_workspace_move(params),
            "workspace_delete" => self.tool_workspace_delete(params),
            // Memory tiering tools
            "memory_create_daily" => self.tool_memory_create_daily(params),
            "memory_promote_to_permanent" => self.tool_memory_promote_to_permanent(params),
            // Embedding cache tools
            "embedding_cache_stats" => self.tool_embedding_cache_stats(params),
            "embedding_cache_clear" => self.tool_embedding_cache_clear(params),
            // Session indexing tools
            "session_index" => self.tool_session_index(params),
            "session_index_delta" => self.tool_session_index_delta(params),
            "session_get" => self.tool_session_get(params),
            "session_list" => self.tool_session_list(params),
            "session_delete" => self.tool_session_delete(params),
            // Identity management tools
            "identity_create" => self.tool_identity_create(params),
            "identity_get" => self.tool_identity_get(params),
            "identity_update" => self.tool_identity_update(params),
            "identity_delete" => self.tool_identity_delete(params),
            "identity_add_alias" => self.tool_identity_add_alias(params),
            "identity_remove_alias" => self.tool_identity_remove_alias(params),
            "identity_resolve" => self.tool_identity_resolve(params),
            "identity_list" => self.tool_identity_list(params),
            "identity_search" => self.tool_identity_search(params),
            "identity_link" => self.tool_identity_link(params),
            "identity_unlink" => self.tool_identity_unlink(params),
            // Content utility tools
            "memory_soft_trim" => self.tool_memory_soft_trim(params),
            "memory_list_compact" => self.tool_memory_list_compact(params),
            "memory_content_stats" => self.tool_memory_content_stats(params),
            // Batch operations
            "memory_create_batch" => self.tool_memory_create_batch(params),
            "memory_delete_batch" => self.tool_memory_delete_batch(params),
            // Tag utilities
            "memory_tags" => self.tool_memory_tags(params),
            "memory_tag_hierarchy" => self.tool_memory_tag_hierarchy(params),
            "memory_validate_tags" => self.tool_memory_validate_tags(params),
            // Import/Export
            "memory_export" => self.tool_memory_export(params),
            "memory_import" => self.tool_memory_import(params),
            // Maintenance
            "memory_rebuild_embeddings" => self.tool_memory_rebuild_embeddings(params),
            "memory_rebuild_crossrefs" => self.tool_memory_rebuild_crossrefs(params),
            // Special memory types
            "memory_create_section" => self.tool_memory_create_section(params),
            "memory_checkpoint" => self.tool_memory_checkpoint(params),
            "memory_boost" => self.tool_memory_boost(params),
            // Event system
            "memory_events_poll" => self.tool_memory_events_poll(params),
            "memory_events_clear" => self.tool_memory_events_clear(params),
            // Advanced sync
            "sync_version" => self.tool_sync_version(params),
            "sync_delta" => self.tool_sync_delta(params),
            "sync_state" => self.tool_sync_state(params),
            "sync_cleanup" => self.tool_sync_cleanup(params),
            // Multi-agent sharing
            "memory_share" => self.tool_memory_share(params),
            "memory_shared_poll" => self.tool_memory_shared_poll(params),
            "memory_share_ack" => self.tool_memory_share_ack(params),
            // Search variants
            "memory_search_by_identity" => self.tool_memory_search_by_identity(params),
            "memory_session_search" => self.tool_memory_session_search(params),
            // Image handling
            "memory_upload_image" => self.tool_memory_upload_image(params),
            "memory_migrate_images" => self.tool_memory_migrate_images(params),
            // Auto-tagging tools
            "memory_suggest_tags" => self.tool_memory_suggest_tags(params),
            "memory_auto_tag" => self.tool_memory_auto_tag(params),
            _ => json!({"error": format!("Unknown tool: {}", name)}),
        }
    }

    fn tool_memory_create(&self, params: Value) -> Value {
        use engram::storage::queries::find_similar_by_embedding;

        let input: CreateMemoryInput = match serde_json::from_value(params) {
            Ok(i) => i,
            Err(e) => return json!({"error": e.to_string()}),
        };

        // Semantic deduplication: if threshold is set and mode is not Allow,
        // check for similar memories using embeddings before creating
        if input.dedup_mode != DedupMode::Allow {
            if let Some(threshold) = input.dedup_threshold {
                // Generate embedding for new content
                if let Ok(query_embedding) = self.embedder.embed(&input.content) {
                    // Check for similar memories (scoped to same workspace)
                    let workspace = input.workspace.as_deref();
                    let similar_result = self.storage.with_connection(|conn| {
                        find_similar_by_embedding(
                            conn,
                            &query_embedding,
                            &input.scope,
                            workspace,
                            threshold,
                        )
                    });

                    if let Ok(Some((existing, similarity))) = similar_result {
                        // Found a similar memory - handle based on dedup_mode
                        match input.dedup_mode {
                            DedupMode::Reject => {
                                return json!({
                                    "error": format!(
                                        "Similar memory detected (id={}, similarity={:.3}). Use dedup_mode='allow' to create anyway.",
                                        existing.id, similarity
                                    ),
                                    "existing_id": existing.id,
                                    "similarity": similarity
                                });
                            }
                            DedupMode::Skip => {
                                // Return existing memory without modification
                                return json!(existing);
                            }
                            DedupMode::Merge => {
                                // Merge: update existing memory with new tags and metadata
                                let merge_result = self.storage.with_transaction(|conn| {
                                    let mut merged_tags = existing.tags.clone();
                                    for tag in &input.tags {
                                        if !merged_tags.contains(tag) {
                                            merged_tags.push(tag.clone());
                                        }
                                    }

                                    let mut merged_metadata = existing.metadata.clone();
                                    for (key, value) in &input.metadata {
                                        merged_metadata.insert(key.clone(), value.clone());
                                    }

                                    let update_input = UpdateMemoryInput {
                                        content: None,
                                        memory_type: None,
                                        tags: Some(merged_tags),
                                        metadata: Some(merged_metadata),
                                        importance: input.importance,
                                        scope: None,
                                        ttl_seconds: input.ttl_seconds,
                                    };

                                    update_memory(conn, existing.id, &update_input)
                                });

                                return match merge_result {
                                    Ok(memory) => json!(memory),
                                    Err(e) => json!({"error": e.to_string()}),
                                };
                            }
                            DedupMode::Allow => {} // Continue to create_memory
                        }
                    }
                }
            }
        }

        let result = self.storage.with_transaction(|conn| {
            let memory = create_memory(conn, &input)?;

            // Add to fuzzy engine vocabulary
            let mut fuzzy = self.fuzzy_engine.lock();
            fuzzy.add_to_vocabulary(&memory.content);

            Ok(memory)
        });

        match result {
            Ok(memory) => {
                // Broadcast real-time event
                self.broadcast_event(RealtimeEvent::memory_created(
                    memory.id,
                    memory.content.clone(),
                ));
                json!(memory)
            }
            Err(e) => json!({"error": e.to_string()}),
        }
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
        let input: UpdateMemoryInput = match serde_json::from_value(params.clone()) {
            Ok(i) => i,
            Err(e) => return json!({"error": e.to_string()}),
        };

        // Track which fields are being changed
        let mut changes = Vec::new();
        if input.content.is_some() {
            changes.push("content".to_string());
        }
        if input.memory_type.is_some() {
            changes.push("memory_type".to_string());
        }
        if input.tags.is_some() {
            changes.push("tags".to_string());
        }
        if input.metadata.is_some() {
            changes.push("metadata".to_string());
        }
        if input.importance.is_some() {
            changes.push("importance".to_string());
        }

        let result = self.storage.with_transaction(|conn| {
            let memory = update_memory(conn, id, &input)?;
            Ok(memory)
        });

        match result {
            Ok(memory) => {
                // Broadcast real-time event
                self.broadcast_event(RealtimeEvent::memory_updated(memory.id, changes));
                json!(memory)
            }
            Err(e) => json!({"error": e.to_string()}),
        }
    }

    fn tool_memory_delete(&self, params: Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);

        let result = self.storage.with_transaction(|conn| {
            delete_memory(conn, id)?;
            Ok(id)
        });

        match result {
            Ok(deleted_id) => {
                // Broadcast real-time event
                self.broadcast_event(RealtimeEvent::memory_deleted(deleted_id));
                json!({"deleted": deleted_id})
            }
            Err(e) => json!({"error": e.to_string()}),
        }
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
            workspace: None,
            tier: Default::default(),
            defer_embedding: false,
            ttl_seconds: None,
            dedup_mode: Default::default(),
            dedup_threshold: None,
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
            workspace: None,
            tier: Default::default(),
            defer_embedding: false,
            ttl_seconds: None,
            dedup_mode: Default::default(),
            dedup_threshold: None,
        };

        self.tool_memory_create(json!(input))
    }

    fn tool_sync_status(&self, _params: Value) -> Value {
        #[cfg(feature = "cloud")]
        {
            self.storage
                .with_connection(|conn| {
                    let status = get_sync_status(conn)?;
                    Ok(json!(status))
                })
                .unwrap_or_else(|e| json!({"error": e.to_string()}))
        }
        #[cfg(not(feature = "cloud"))]
        {
            json!({"error": "Cloud sync requires the 'cloud' feature to be enabled"})
        }
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
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: false,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
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
                            workspace: None,
                            tier: Default::default(),
                            defer_embedding: false,
                            ttl_seconds: None,
                            dedup_mode: Default::default(),
                            dedup_threshold: None,
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

    fn tool_find_duplicates(&self, params: Value) -> Value {
        use engram::storage::queries::find_duplicates;

        let threshold = params
            .get("threshold")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.9);

        self.storage
            .with_connection(|conn| {
                let duplicates = find_duplicates(conn, threshold)?;
                Ok(json!({
                    "count": duplicates.len(),
                    "threshold": threshold,
                    "duplicates": duplicates
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // Workspace management tools

    fn tool_workspace_list(&self, _params: Value) -> Value {
        use engram::storage::queries::list_workspaces;

        self.storage
            .with_connection(|conn| {
                let workspaces = list_workspaces(conn)?;
                Ok(json!({
                    "count": workspaces.len(),
                    "workspaces": workspaces
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_workspace_stats(&self, params: Value) -> Value {
        use engram::storage::queries::get_workspace_stats;

        let workspace = match params.get("workspace").and_then(|v| v.as_str()) {
            Some(ws) => ws,
            None => return json!({"error": "workspace is required"}),
        };

        self.storage
            .with_connection(|conn| {
                let stats = get_workspace_stats(conn, workspace)?;
                Ok(json!(stats))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_workspace_move(&self, params: Value) -> Value {
        use engram::storage::queries::move_to_workspace;

        let id = match params.get("id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return json!({"error": "id is required"}),
        };
        let workspace = match params.get("workspace").and_then(|v| v.as_str()) {
            Some(ws) => ws,
            None => return json!({"error": "workspace is required"}),
        };

        self.storage
            .with_connection(|conn| {
                let memory = move_to_workspace(conn, id, workspace)?;
                Ok(json!({
                    "success": true,
                    "memory": memory
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_workspace_delete(&self, params: Value) -> Value {
        use engram::storage::queries::delete_workspace;

        let workspace = match params.get("workspace").and_then(|v| v.as_str()) {
            Some(ws) => ws,
            None => return json!({"error": "workspace is required"}),
        };
        let move_to_default = params
            .get("move_to_default")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        self.storage
            .with_connection(|conn| {
                let affected = delete_workspace(conn, workspace, move_to_default)?;
                Ok(json!({
                    "success": true,
                    "affected": affected,
                    "move_to_default": move_to_default
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // Memory tiering tools

    fn tool_memory_create_daily(&self, params: Value) -> Value {
        use engram::storage::queries::create_memory;
        use engram::types::{CreateMemoryInput, MemoryTier, MemoryType};

        let content = match params.get("content").and_then(|v| v.as_str()) {
            Some(c) => c.to_string(),
            None => return json!({"error": "content is required"}),
        };

        let memory_type = params
            .get("type")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(MemoryType::Note);

        let tags: Vec<String> = params
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let metadata: std::collections::HashMap<String, serde_json::Value> = params
            .get("metadata")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let importance = params
            .get("importance")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32);

        let ttl_seconds = params
            .get("ttl_seconds")
            .and_then(|v| v.as_i64())
            .unwrap_or(86400); // Default: 24 hours

        let workspace = params
            .get("workspace")
            .and_then(|v| v.as_str())
            .map(String::from);

        let input = CreateMemoryInput {
            content,
            memory_type,
            tags,
            metadata,
            importance,
            scope: Default::default(),
            workspace,
            tier: MemoryTier::Daily,
            defer_embedding: false,
            ttl_seconds: Some(ttl_seconds),
            dedup_mode: Default::default(),
            dedup_threshold: None,
        };

        self.storage
            .with_connection(|conn| {
                let memory = create_memory(conn, &input)?;
                Ok(json!(memory))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_promote_to_permanent(&self, params: Value) -> Value {
        use engram::storage::queries::promote_to_permanent;

        let id = match params.get("id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return json!({"error": "id is required"}),
        };

        self.storage
            .with_connection(|conn| {
                let memory = promote_to_permanent(conn, id)?;
                Ok(json!({
                    "success": true,
                    "memory": memory
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // Embedding cache tools

    fn tool_embedding_cache_stats(&self, _params: Value) -> Value {
        let stats = self.embedding_cache.stats();
        json!({
            "hits": stats.hits,
            "misses": stats.misses,
            "entries": stats.entries,
            "bytes_used": stats.bytes_used,
            "max_bytes": stats.max_bytes,
            "hit_rate": stats.hit_rate,
            "bytes_used_mb": stats.bytes_used as f64 / (1024.0 * 1024.0),
            "max_bytes_mb": stats.max_bytes as f64 / (1024.0 * 1024.0)
        })
    }

    fn tool_embedding_cache_clear(&self, _params: Value) -> Value {
        let stats_before = self.embedding_cache.stats();
        self.embedding_cache.clear();
        let stats_after = self.embedding_cache.stats();
        json!({
            "success": true,
            "entries_cleared": stats_before.entries,
            "bytes_freed": stats_before.bytes_used,
            "bytes_freed_mb": stats_before.bytes_used as f64 / (1024.0 * 1024.0),
            "entries_after": stats_after.entries
        })
    }

    // Session indexing tools

    fn tool_session_index(&self, params: Value) -> Value {
        use engram::intelligence::session_indexing::{index_conversation, ChunkingConfig, Message};

        let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({"error": "session_id is required"}),
        };

        let messages: Vec<Message> = match params.get("messages").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|m| {
                    let role = m.get("role")?.as_str()?.to_string();
                    let content = m.get("content")?.as_str()?.to_string();
                    let timestamp = m
                        .get("timestamp")
                        .and_then(|t| t.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now);
                    let id = m.get("id").and_then(|i| i.as_str()).map(String::from);
                    Some(Message {
                        role,
                        content,
                        timestamp,
                        id,
                    })
                })
                .collect(),
            None => return json!({"error": "messages array is required"}),
        };

        if messages.is_empty() {
            return json!({"error": "messages array cannot be empty"});
        }

        let title = params.get("title").and_then(|v| v.as_str());
        let workspace = params.get("workspace").and_then(|v| v.as_str());
        let agent_id = params.get("agent_id").and_then(|v| v.as_str());

        let config = ChunkingConfig {
            max_messages: params
                .get("max_messages")
                .and_then(|v| v.as_i64())
                .unwrap_or(10) as usize,
            overlap_messages: params.get("overlap").and_then(|v| v.as_i64()).unwrap_or(2) as usize,
            max_chars: params
                .get("max_chars")
                .and_then(|v| v.as_i64())
                .unwrap_or(8000) as usize,
            default_ttl_seconds: params.get("ttl_days").and_then(|v| v.as_i64()).unwrap_or(7)
                * 24
                * 60
                * 60,
        };

        self.storage
            .with_connection(|conn| {
                let session = index_conversation(
                    conn, session_id, &messages, &config, workspace, title, agent_id,
                )?;
                Ok(json!({
                    "success": true,
                    "session": session
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_session_index_delta(&self, params: Value) -> Value {
        use engram::intelligence::session_indexing::{
            index_conversation_delta, ChunkingConfig, Message,
        };

        let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({"error": "session_id is required"}),
        };

        let messages: Vec<Message> = match params.get("messages").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|m| {
                    let role = m.get("role")?.as_str()?.to_string();
                    let content = m.get("content")?.as_str()?.to_string();
                    let timestamp = m
                        .get("timestamp")
                        .and_then(|t| t.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now);
                    let id = m.get("id").and_then(|i| i.as_str()).map(String::from);
                    Some(Message {
                        role,
                        content,
                        timestamp,
                        id,
                    })
                })
                .collect(),
            None => return json!({"error": "messages array is required"}),
        };

        let config = ChunkingConfig::default();

        self.storage
            .with_connection(|conn| {
                let session = index_conversation_delta(conn, session_id, &messages, &config)?;
                Ok(json!({
                    "success": true,
                    "session": session
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_session_get(&self, params: Value) -> Value {
        use engram::intelligence::session_indexing::get_session;

        let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({"error": "session_id is required"}),
        };

        self.storage
            .with_connection(|conn| {
                let session = get_session(conn, session_id)?;
                Ok(json!(session))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_session_list(&self, params: Value) -> Value {
        use engram::intelligence::session_indexing::list_sessions;

        let workspace = params.get("workspace").and_then(|v| v.as_str());
        let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);

        self.storage
            .with_connection(|conn| {
                let sessions = list_sessions(conn, workspace, limit)?;
                Ok(json!({
                    "count": sessions.len(),
                    "sessions": sessions
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_session_delete(&self, params: Value) -> Value {
        use engram::intelligence::session_indexing::delete_session;

        let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({"error": "session_id is required"}),
        };

        self.storage
            .with_connection(|conn| {
                delete_session(conn, session_id)?;
                Ok(json!({
                    "success": true,
                    "session_id": session_id
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // Identity management tools

    fn tool_identity_create(&self, params: Value) -> Value {
        use engram::storage::identity_links::{create_identity, CreateIdentityInput, IdentityType};

        let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => return json!({"error": "canonical_id is required"}),
        };

        let display_name = match params.get("display_name").and_then(|v| v.as_str()) {
            Some(name) => name.to_string(),
            None => return json!({"error": "display_name is required"}),
        };

        let entity_type = params
            .get("entity_type")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(IdentityType::Person);

        let description = params
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);

        let aliases: Vec<String> = params
            .get("aliases")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let metadata: std::collections::HashMap<String, serde_json::Value> = params
            .get("metadata")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let input = CreateIdentityInput {
            canonical_id,
            display_name,
            entity_type,
            description,
            metadata,
            aliases,
        };

        self.storage
            .with_connection(|conn| {
                let identity = create_identity(conn, &input)?;
                Ok(json!(identity))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_identity_get(&self, params: Value) -> Value {
        use engram::storage::identity_links::get_identity;

        let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({"error": "canonical_id is required"}),
        };

        self.storage
            .with_connection(|conn| {
                let identity = get_identity(conn, canonical_id)?;
                Ok(json!(identity))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_identity_update(&self, params: Value) -> Value {
        use engram::storage::identity_links::{update_identity, IdentityType};

        let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({"error": "canonical_id is required"}),
        };

        let display_name = params.get("display_name").and_then(|v| v.as_str());
        let description = params.get("description").and_then(|v| v.as_str());
        let entity_type: Option<IdentityType> = params
            .get("entity_type")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok());

        self.storage
            .with_connection(|conn| {
                let identity =
                    update_identity(conn, canonical_id, display_name, description, entity_type)?;
                Ok(json!(identity))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_identity_delete(&self, params: Value) -> Value {
        use engram::storage::identity_links::delete_identity;

        let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({"error": "canonical_id is required"}),
        };

        self.storage
            .with_connection(|conn| {
                delete_identity(conn, canonical_id)?;
                Ok(json!({
                    "success": true,
                    "canonical_id": canonical_id
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_identity_add_alias(&self, params: Value) -> Value {
        use engram::storage::identity_links::add_alias;

        let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({"error": "canonical_id is required"}),
        };

        let alias = match params.get("alias").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return json!({"error": "alias is required"}),
        };

        let source = params.get("source").and_then(|v| v.as_str());

        self.storage
            .with_connection(|conn| {
                let alias_obj = add_alias(conn, canonical_id, alias, source)?;
                Ok(json!(alias_obj))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_identity_remove_alias(&self, params: Value) -> Value {
        use engram::storage::identity_links::remove_alias;

        let alias = match params.get("alias").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return json!({"error": "alias is required"}),
        };

        self.storage
            .with_connection(|conn| {
                remove_alias(conn, alias)?;
                Ok(json!({
                    "success": true,
                    "alias": alias
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_identity_resolve(&self, params: Value) -> Value {
        use engram::storage::identity_links::resolve_alias;

        let alias = match params.get("alias").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return json!({"error": "alias is required"}),
        };

        self.storage
            .with_connection(|conn| {
                let identity = resolve_alias(conn, alias)?;
                Ok(json!({
                    "found": identity.is_some(),
                    "identity": identity
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_identity_list(&self, params: Value) -> Value {
        use engram::storage::identity_links::{list_identities, IdentityType};

        let entity_type: Option<IdentityType> = params
            .get("entity_type")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok());

        let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);

        self.storage
            .with_connection(|conn| {
                let identities = list_identities(conn, entity_type, limit)?;
                Ok(json!({
                    "count": identities.len(),
                    "identities": identities
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_identity_search(&self, params: Value) -> Value {
        use engram::storage::identity_links::search_identities_by_alias;

        let query = match params.get("query").and_then(|v| v.as_str()) {
            Some(q) => q,
            None => return json!({"error": "query is required"}),
        };

        let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);

        self.storage
            .with_connection(|conn| {
                let identities = search_identities_by_alias(conn, query, limit)?;
                Ok(json!({
                    "count": identities.len(),
                    "identities": identities
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_identity_link(&self, params: Value) -> Value {
        use engram::storage::identity_links::link_identity_to_memory;

        let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return json!({"error": "memory_id is required"}),
        };

        let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({"error": "canonical_id is required"}),
        };

        let mention_text = params.get("mention_text").and_then(|v| v.as_str());

        self.storage
            .with_connection(|conn| {
                let link = link_identity_to_memory(conn, memory_id, canonical_id, mention_text)?;
                Ok(json!(link))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_identity_unlink(&self, params: Value) -> Value {
        use engram::storage::identity_links::unlink_identity_from_memory;

        let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return json!({"error": "memory_id is required"}),
        };

        let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return json!({"error": "canonical_id is required"}),
        };

        self.storage
            .with_connection(|conn| {
                unlink_identity_from_memory(conn, memory_id, canonical_id)?;
                Ok(json!({
                    "success": true,
                    "memory_id": memory_id,
                    "canonical_id": canonical_id
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // Content utility tools

    fn tool_memory_soft_trim(&self, params: Value) -> Value {
        use engram::intelligence::{soft_trim, SoftTrimConfig};

        let id = match params.get("id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return json!({"error": "id is required"}),
        };

        let max_chars = params
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .unwrap_or(500) as usize;
        let head_percent = params
            .get("head_percent")
            .and_then(|v| v.as_u64())
            .unwrap_or(60) as usize;
        let tail_percent = params
            .get("tail_percent")
            .and_then(|v| v.as_u64())
            .unwrap_or(30) as usize;
        let ellipsis = params
            .get("ellipsis")
            .and_then(|v| v.as_str())
            .unwrap_or("\n...\n")
            .to_string();
        let preserve_words = params
            .get("preserve_words")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let config = SoftTrimConfig {
            max_chars,
            head_percent,
            tail_percent,
            ellipsis,
            preserve_words,
        };

        self.storage
            .with_connection(|conn| {
                let memory = get_memory(conn, id)?;
                let result = soft_trim(&memory.content, &config);
                Ok(json!({
                    "id": id,
                    "trimmed_content": result.content,
                    "was_trimmed": result.was_trimmed,
                    "original_chars": result.original_chars,
                    "trimmed_chars": result.trimmed_chars,
                    "chars_removed": result.chars_removed
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_list_compact(&self, params: Value) -> Value {
        use engram::storage::list_memories_compact;

        let options: ListOptions = serde_json::from_value(params.clone()).unwrap_or_default();
        let preview_chars = params
            .get("preview_chars")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        self.storage
            .with_connection(|conn| {
                let memories = list_memories_compact(conn, &options, preview_chars)?;
                Ok(json!({
                    "count": memories.len(),
                    "memories": memories
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_content_stats(&self, params: Value) -> Value {
        use engram::intelligence::content_stats;

        let id = match params.get("id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return json!({"error": "id is required"}),
        };

        self.storage
            .with_connection(|conn| {
                let memory = get_memory(conn, id)?;
                let stats = content_stats(&memory.content);
                Ok(json!({
                    "id": id,
                    "stats": stats
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Batch Operations
    // =========================================================================

    fn tool_memory_create_batch(&self, params: Value) -> Value {
        use engram::storage::create_memory_batch;

        let memories = match params.get("memories").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return json!({"error": "memories array is required"}),
        };

        let inputs: Vec<CreateMemoryInput> = memories
            .iter()
            .filter_map(|m| serde_json::from_value(m.clone()).ok())
            .collect();

        if inputs.is_empty() {
            return json!({"error": "No valid memory inputs provided"});
        }

        self.storage
            .with_connection(|conn| {
                let result = create_memory_batch(conn, &inputs)?;
                Ok(json!(result))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_delete_batch(&self, params: Value) -> Value {
        use engram::storage::delete_memory_batch;

        let ids: Vec<i64> = match params.get("ids").and_then(|v| v.as_array()) {
            Some(arr) => arr.iter().filter_map(|v| v.as_i64()).collect(),
            None => return json!({"error": "ids array is required"}),
        };

        if ids.is_empty() {
            return json!({"error": "No valid IDs provided"});
        }

        self.storage
            .with_connection(|conn| {
                let result = delete_memory_batch(conn, &ids)?;
                Ok(json!(result))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Tag Utilities
    // =========================================================================

    fn tool_memory_tags(&self, _params: Value) -> Value {
        use engram::storage::list_tags;

        self.storage
            .with_connection(|conn| {
                let tags = list_tags(conn)?;
                Ok(json!({"tags": tags}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_tag_hierarchy(&self, _params: Value) -> Value {
        use engram::storage::get_tag_hierarchy;

        self.storage
            .with_connection(|conn| {
                let hierarchy = get_tag_hierarchy(conn)?;
                Ok(json!({"hierarchy": hierarchy}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_validate_tags(&self, _params: Value) -> Value {
        use engram::storage::validate_tags;

        self.storage
            .with_connection(|conn| {
                let result = validate_tags(conn)?;
                Ok(json!(result))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Import/Export
    // =========================================================================

    fn tool_memory_export(&self, _params: Value) -> Value {
        use engram::storage::export_memories;

        self.storage
            .with_connection(|conn| {
                let data = export_memories(conn)?;
                Ok(json!(data))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_import(&self, params: Value) -> Value {
        use engram::storage::{import_memories, ExportData};

        let data: ExportData = match params
            .get("data")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            Some(d) => d,
            None => return json!({"error": "data object is required"}),
        };

        let skip_duplicates = params
            .get("skip_duplicates")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        self.storage
            .with_connection(|conn| {
                let result = import_memories(conn, &data, skip_duplicates)?;
                Ok(json!(result))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Maintenance
    // =========================================================================

    fn tool_memory_rebuild_embeddings(&self, _params: Value) -> Value {
        use engram::storage::rebuild_embeddings;

        self.storage
            .with_connection(|conn| {
                let count = rebuild_embeddings(conn)?;
                Ok(json!({"rebuilt": count}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_rebuild_crossrefs(&self, _params: Value) -> Value {
        use engram::storage::rebuild_crossrefs;

        self.storage
            .with_connection(|conn| {
                let count = rebuild_crossrefs(conn)?;
                Ok(json!({"rebuilt": count}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Special Memory Types
    // =========================================================================

    fn tool_memory_create_section(&self, params: Value) -> Value {
        use engram::storage::create_section_memory;

        let title = match params.get("title").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return json!({"error": "title is required"}),
        };

        let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let parent_id = params.get("parent_id").and_then(|v| v.as_i64());
        let level = params.get("level").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
        let workspace = params.get("workspace").and_then(|v| v.as_str());

        self.storage
            .with_connection(|conn| {
                let memory =
                    create_section_memory(conn, title, content, parent_id, level, workspace)?;
                Ok(json!(memory))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_checkpoint(&self, params: Value) -> Value {
        use engram::storage::create_checkpoint;
        use std::collections::HashMap;

        let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return json!({"error": "session_id is required"}),
        };

        let summary = match params.get("summary").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return json!({"error": "summary is required"}),
        };

        let context: HashMap<String, Value> = params
            .get("context")
            .and_then(|v| v.as_object())
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        let workspace = params.get("workspace").and_then(|v| v.as_str());

        self.storage
            .with_connection(|conn| {
                let memory = create_checkpoint(conn, session_id, summary, &context, workspace)?;
                Ok(json!(memory))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_boost(&self, params: Value) -> Value {
        use engram::storage::boost_memory;

        let id = match params.get("id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return json!({"error": "id is required"}),
        };

        let boost_amount = params
            .get("boost_amount")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.2) as f32;
        let duration_seconds = params.get("duration_seconds").and_then(|v| v.as_i64());

        self.storage
            .with_connection(|conn| {
                let memory = boost_memory(conn, id, boost_amount, duration_seconds)?;
                Ok(json!(memory))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Event System
    // =========================================================================

    fn tool_memory_events_poll(&self, params: Value) -> Value {
        use chrono::DateTime;
        use engram::storage::poll_events;

        let since_id = params.get("since_id").and_then(|v| v.as_i64());
        let since_time = params
            .get("since_time")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));
        let agent_id = params.get("agent_id").and_then(|v| v.as_str());
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        self.storage
            .with_connection(|conn| {
                let events = poll_events(conn, since_id, since_time, agent_id, limit)?;
                Ok(json!({"events": events}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_events_clear(&self, params: Value) -> Value {
        use chrono::DateTime;
        use engram::storage::clear_events;

        let before_id = params.get("before_id").and_then(|v| v.as_i64());
        let before_time = params
            .get("before_time")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc));
        let keep_recent = params
            .get("keep_recent")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        self.storage
            .with_connection(|conn| {
                let deleted = clear_events(conn, before_id, before_time, keep_recent)?;
                Ok(json!({"deleted": deleted}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Advanced Sync
    // =========================================================================

    fn tool_sync_version(&self, _params: Value) -> Value {
        use engram::storage::get_sync_version;

        self.storage
            .with_connection(|conn| {
                let version = get_sync_version(conn)?;
                Ok(json!(version))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_sync_delta(&self, params: Value) -> Value {
        use engram::storage::get_sync_delta;

        let since_version = match params.get("since_version").and_then(|v| v.as_i64()) {
            Some(v) => v,
            None => return json!({"error": "since_version is required"}),
        };

        self.storage
            .with_connection(|conn| {
                let delta = get_sync_delta(conn, since_version)?;
                Ok(json!(delta))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_sync_state(&self, params: Value) -> Value {
        use engram::storage::{get_agent_sync_state, update_agent_sync_state};

        let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return json!({"error": "agent_id is required"}),
        };

        // If update_version is provided, update the state first
        if let Some(version) = params.get("update_version").and_then(|v| v.as_i64()) {
            if let Err(e) = self
                .storage
                .with_connection(|conn| update_agent_sync_state(conn, agent_id, version))
            {
                return json!({"error": e.to_string()});
            }
        }

        self.storage
            .with_connection(|conn| {
                let state = get_agent_sync_state(conn, agent_id)?;
                Ok(json!(state))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_sync_cleanup(&self, params: Value) -> Value {
        use engram::storage::cleanup_sync_data;

        let older_than_days = params
            .get("older_than_days")
            .and_then(|v| v.as_i64())
            .unwrap_or(30);

        self.storage
            .with_connection(|conn| {
                let deleted = cleanup_sync_data(conn, older_than_days)?;
                Ok(json!({"deleted": deleted}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Multi-Agent Sharing
    // =========================================================================

    fn tool_memory_share(&self, params: Value) -> Value {
        use engram::storage::share_memory;

        let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return json!({"error": "memory_id is required"}),
        };

        let from_agent = match params.get("from_agent").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return json!({"error": "from_agent is required"}),
        };

        let to_agent = match params.get("to_agent").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return json!({"error": "to_agent is required"}),
        };

        let message = params.get("message").and_then(|v| v.as_str());

        self.storage
            .with_connection(|conn| {
                let share_id = share_memory(conn, memory_id, from_agent, to_agent, message)?;
                Ok(json!({"share_id": share_id}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_shared_poll(&self, params: Value) -> Value {
        use engram::storage::poll_shared_memories;

        let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return json!({"error": "agent_id is required"}),
        };

        let include_acknowledged = params
            .get("include_acknowledged")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        self.storage
            .with_connection(|conn| {
                let shares = poll_shared_memories(conn, agent_id, include_acknowledged)?;
                Ok(json!({"shares": shares}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_share_ack(&self, params: Value) -> Value {
        use engram::storage::acknowledge_share;

        let share_id = match params.get("share_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return json!({"error": "share_id is required"}),
        };

        let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => return json!({"error": "agent_id is required"}),
        };

        self.storage
            .with_connection(|conn| {
                acknowledge_share(conn, share_id, agent_id)?;
                Ok(json!({"acknowledged": true}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Search Variants
    // =========================================================================

    fn tool_memory_search_by_identity(&self, params: Value) -> Value {
        use engram::storage::search_by_identity;

        let identity = match params.get("identity").and_then(|v| v.as_str()) {
            Some(i) => i,
            None => return json!({"error": "identity is required"}),
        };

        let workspace = params.get("workspace").and_then(|v| v.as_str());
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        self.storage
            .with_connection(|conn| {
                let memories = search_by_identity(conn, identity, workspace, limit)?;
                Ok(json!({"memories": memories}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_session_search(&self, params: Value) -> Value {
        use engram::storage::search_sessions;

        let query = match params.get("query").and_then(|v| v.as_str()) {
            Some(q) => q,
            None => return json!({"error": "query is required"}),
        };

        let session_id = params.get("session_id").and_then(|v| v.as_str());
        let workspace = params.get("workspace").and_then(|v| v.as_str());
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        self.storage
            .with_connection(|conn| {
                let memories = search_sessions(conn, query, session_id, workspace, limit)?;
                Ok(json!({"memories": memories}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Image Handling
    // =========================================================================

    fn tool_memory_upload_image(&self, params: Value) -> Value {
        use engram::storage::{upload_image, ImageStorageConfig, LocalImageStorage};

        let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
            Some(id) => id,
            None => return json!({"error": "memory_id is required"}),
        };

        let file_path = match params.get("file_path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return json!({"error": "file_path is required"}),
        };

        let image_index = params
            .get("image_index")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let caption = params.get("caption").and_then(|v| v.as_str());

        // Initialize local image storage
        let config = ImageStorageConfig::default();
        let image_storage = match LocalImageStorage::new(config.local_dir) {
            Ok(s) => s,
            Err(e) => return json!({"error": format!("Failed to initialize image storage: {}", e)}),
        };

        self.storage
            .with_connection(|conn| {
                let image_ref = upload_image(
                    conn,
                    &image_storage,
                    memory_id,
                    file_path,
                    image_index,
                    caption,
                )?;
                Ok(json!({
                    "success": true,
                    "image": image_ref
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    fn tool_memory_migrate_images(&self, params: Value) -> Value {
        use engram::storage::{migrate_images, ImageStorageConfig, LocalImageStorage};

        let dry_run = params
            .get("dry_run")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Initialize local image storage
        let config = ImageStorageConfig::default();
        let image_storage = match LocalImageStorage::new(config.local_dir) {
            Ok(s) => s,
            Err(e) => return json!({"error": format!("Failed to initialize image storage: {}", e)}),
        };

        self.storage
            .with_connection(|conn| {
                let result = migrate_images(conn, &image_storage, dry_run)?;
                Ok(json!(result))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }

    // =========================================================================
    // Auto-Tagging Tools
    // =========================================================================

    /// Suggest tags for a memory based on content analysis
    fn tool_memory_suggest_tags(&self, params: Value) -> Value {
        use engram::intelligence::{AutoTagConfig, AutoTagger};

        // Can either provide memory_id to analyze existing memory
        // or provide content directly for analysis
        let (content, memory_type, existing_tags) = if let Some(id) = params
            .get("id")
            .or_else(|| params.get("memory_id"))
            .and_then(|v| v.as_i64())
        {
            // Get memory from storage
            match self.storage.with_connection(|conn| get_memory(conn, id)) {
                Ok(memory) => (memory.content, Some(memory.memory_type), memory.tags),
                Err(e) => return json!({"error": e.to_string()}),
            }
        } else if let Some(content) = params.get("content").and_then(|v| v.as_str()) {
            let memory_type = params
                .get("type")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok());
            let existing: Vec<String> = params
                .get("existing_tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            (content.to_string(), memory_type, existing)
        } else {
            return json!({"error": "Either 'id'/'memory_id' or 'content' is required"});
        };

        // Build config from params
        let mut config = AutoTagConfig::default();

        if let Some(min_conf) = params.get("min_confidence").and_then(|v| v.as_f64()) {
            config.min_confidence = min_conf as f32;
        }
        if let Some(max) = params.get("max_tags").and_then(|v| v.as_u64()) {
            config.max_tags = max as usize;
        }
        if let Some(v) = params.get("enable_patterns").and_then(|v| v.as_bool()) {
            config.enable_patterns = v;
        }
        if let Some(v) = params.get("enable_keywords").and_then(|v| v.as_bool()) {
            config.enable_keywords = v;
        }
        if let Some(v) = params.get("enable_entities").and_then(|v| v.as_bool()) {
            config.enable_entities = v;
        }
        if let Some(v) = params.get("enable_type_tags").and_then(|v| v.as_bool()) {
            config.enable_type_tags = v;
        }

        // Add custom keyword mappings if provided
        if let Some(mappings) = params.get("keyword_mappings").and_then(|v| v.as_object()) {
            for (keyword, tag) in mappings {
                if let Some(tag_str) = tag.as_str() {
                    config
                        .keyword_mappings
                        .insert(keyword.clone(), tag_str.to_string());
                }
            }
        }

        let tagger = AutoTagger::new(config);
        let result = tagger.suggest_tags(&content, memory_type, &existing_tags);

        json!({
            "suggestions": result.suggestions,
            "analysis_count": result.analysis_count
        })
    }

    /// Automatically tag a memory - suggests and optionally applies tags
    fn tool_memory_auto_tag(&self, params: Value) -> Value {
        use engram::intelligence::{AutoTagConfig, AutoTagger};

        let id = match params
            .get("id")
            .or_else(|| params.get("memory_id"))
            .and_then(|v| v.as_i64())
        {
            Some(id) => id,
            None => return json!({"error": "id or memory_id is required"}),
        };

        let apply = params
            .get("apply")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let merge = params
            .get("merge")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Build config from params
        let mut config = AutoTagConfig::default();

        if let Some(min_conf) = params.get("min_confidence").and_then(|v| v.as_f64()) {
            config.min_confidence = min_conf as f32;
        }
        if let Some(max) = params.get("max_tags").and_then(|v| v.as_u64()) {
            config.max_tags = max as usize;
        }

        // Add custom keyword mappings if provided
        if let Some(mappings) = params.get("keyword_mappings").and_then(|v| v.as_object()) {
            for (keyword, tag) in mappings {
                if let Some(tag_str) = tag.as_str() {
                    config
                        .keyword_mappings
                        .insert(keyword.clone(), tag_str.to_string());
                }
            }
        }

        // Get memory and suggest tags
        let (memory, suggestions) = match self.storage.with_connection(|conn| {
            let memory = get_memory(conn, id)?;
            let tagger = AutoTagger::new(config);
            let result = tagger.suggest_for_memory(&memory);
            Ok((memory, result))
        }) {
            Ok(r) => r,
            Err(e) => return json!({"error": e.to_string()}),
        };

        // If not applying, just return suggestions
        if !apply {
            return json!({
                "memory_id": id,
                "suggestions": suggestions.suggestions,
                "applied": false,
                "message": "Tags suggested but not applied. Set apply=true to apply them."
            });
        }

        // Apply tags
        let suggested_tags: Vec<String> = suggestions
            .suggestions
            .iter()
            .map(|s| s.tag.clone())
            .collect();

        let new_tags = if merge {
            // Merge with existing tags
            let mut tags = memory.tags.clone();
            for tag in suggested_tags.iter() {
                if !tags.iter().any(|t| t.to_lowercase() == tag.to_lowercase()) {
                    tags.push(tag.clone());
                }
            }
            tags
        } else {
            // Replace with suggested tags
            suggested_tags.clone()
        };

        let update_input = UpdateMemoryInput {
            content: None,
            memory_type: None,
            tags: Some(new_tags.clone()),
            metadata: None,
            importance: None,
            scope: None,
            ttl_seconds: None,
        };

        match self
            .storage
            .with_transaction(|conn| update_memory(conn, id, &update_input))
        {
            Ok(updated_memory) => {
                json!({
                    "memory_id": id,
                    "suggestions": suggestions.suggestions,
                    "applied": true,
                    "applied_tags": suggested_tags,
                    "final_tags": updated_memory.tags,
                    "merged": merge
                })
            }
            Err(e) => json!({"error": e.to_string()}),
        }
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

    // Create real-time manager if WebSocket port is specified
    let realtime_manager = if args.ws_port > 0 {
        Some(RealtimeManager::new())
    } else {
        None
    };

    // Create handler and server
    let mut handler = EngramHandler::new(storage.clone(), embedder);
    if let Some(ref manager) = realtime_manager {
        handler = handler.with_realtime(manager.clone());
    }
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

    // Start WebSocket server in background if port is specified
    if let Some(manager) = realtime_manager {
        let ws_port = args.ws_port;
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(async {
                let ws_server = RealtimeServer::new(manager, ws_port);
                tracing::info!("WebSocket server starting on port {}...", ws_port);
                if let Err(e) = ws_server.start().await {
                    tracing::error!("WebSocket server error: {}", e);
                }
            });
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
            realtime: None,
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
