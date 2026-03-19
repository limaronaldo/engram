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
use engram::mcp::{
    get_prompt, get_tool_definitions, handlers, http_transport, list_prompts, list_resources,
    methods, read_resource, InitializeResult, McpHandler, McpRequest, McpResponse, McpServer,
    PromptCapabilities, ResourceCapabilities, ServerCapabilities, ToolCallResult, ToolsCapability,
    MCP_PROTOCOL_VERSION, MCP_PROTOCOL_VERSION_LEGACY,
};
use engram::realtime::{RealtimeManager, RealtimeServer};
use engram::search::{FuzzyEngine, SearchConfig};
use engram::storage::Storage;
#[cfg(feature = "meilisearch")]
use engram::storage::{MeilisearchBackend, MeilisearchIndexer, SqliteBackend};
use engram::types::*;

/// Transport mode for the MCP server.
#[derive(Debug, Clone, clap::ValueEnum)]
enum TransportMode {
    /// JSON-RPC over stdio (default, for MCP clients like Claude)
    Stdio,
    /// Streamable HTTP transport (JSON-RPC over HTTP)
    Http,
    /// Both stdio and HTTP transports simultaneously
    Both,
    /// gRPC transport only (requires the `grpc` feature)
    #[cfg(feature = "grpc")]
    Grpc,
}

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

    /// OpenAI-compatible API base URL (for OpenRouter, Azure, etc.)
    #[arg(
        long,
        env = "OPENAI_BASE_URL",
        default_value = "https://api.openai.com/v1"
    )]
    openai_base_url: String,

    /// Embedding model name (e.g., text-embedding-3-small, openai/text-embedding-3-small for OpenRouter)
    #[arg(
        long,
        env = "OPENAI_EMBEDDING_MODEL",
        default_value = "text-embedding-3-small"
    )]
    openai_embedding_model: String,

    /// Embedding dimensions (must match model output; 1536 for text-embedding-3-small)
    #[arg(long, env = "OPENAI_EMBEDDING_DIMENSIONS")]
    openai_embedding_dimensions: Option<usize>,

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

    /// Compression scheduler interval in seconds (0 = disabled)
    /// Auto-summarizes old, rarely-accessed memories at this interval
    #[arg(long, env = "ENGRAM_COMPRESSION_INTERVAL", default_value = "0")]
    compression_interval_seconds: u64,

    /// Max age in days before a memory is eligible for auto-compression
    #[arg(long, env = "ENGRAM_COMPRESSION_MAX_AGE_DAYS", default_value = "90")]
    compression_max_age_days: i64,

    /// Max importance for auto-compression eligibility (0.0-1.0)
    #[arg(long, env = "ENGRAM_COMPRESSION_MAX_IMPORTANCE", default_value = "0.3")]
    compression_max_importance: f32,

    /// Min access count to skip auto-compression
    #[arg(long, env = "ENGRAM_COMPRESSION_MIN_ACCESS", default_value = "3")]
    compression_min_access: i32,

    /// WebSocket server port for real-time events (0 = disabled)
    #[arg(long, env = "ENGRAM_WS_PORT", default_value = "0")]
    ws_port: u16,

    /// Transport mode: stdio (default), http, or both
    #[arg(long, env = "ENGRAM_TRANSPORT", value_enum, default_value = "stdio")]
    transport: TransportMode,

    /// HTTP transport port (used when --transport is http or both)
    #[arg(long, env = "ENGRAM_HTTP_PORT", default_value = "3100")]
    http_port: u16,

    /// API key for HTTP transport authentication (optional)
    #[arg(long, env = "ENGRAM_HTTP_API_KEY")]
    http_api_key: Option<String>,

    /// gRPC transport port (used when --transport is grpc)
    #[cfg(feature = "grpc")]
    #[arg(long, env = "ENGRAM_GRPC_PORT", default_value = "50051")]
    grpc_port: u16,

    /// API key for gRPC transport authentication (optional Bearer token)
    #[cfg(feature = "grpc")]
    #[arg(long, env = "ENGRAM_GRPC_API_KEY")]
    grpc_api_key: Option<String>,

    /// Meilisearch URL for optional search indexing
    #[cfg(feature = "meilisearch")]
    #[arg(long, env = "MEILISEARCH_URL")]
    meilisearch_url: Option<String>,

    /// Meilisearch API key (optional)
    #[cfg(feature = "meilisearch")]
    #[arg(long, env = "MEILISEARCH_API_KEY")]
    meilisearch_api_key: Option<String>,

    /// Enable Meilisearch indexer service
    #[cfg(feature = "meilisearch")]
    #[arg(long, env = "MEILISEARCH_INDEXER", default_value_t = false)]
    meilisearch_indexer: bool,

    /// Meilisearch sync interval in seconds
    #[cfg(feature = "meilisearch")]
    #[arg(long, env = "MEILISEARCH_SYNC_INTERVAL", default_value = "60")]
    meilisearch_sync_interval: u64,
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
    /// Search result cache (Phase 4 - ENG-36)
    search_cache: Arc<engram::search::SearchResultCache>,
    /// Meilisearch backend for Phase 7 MCP tools
    #[cfg(feature = "meilisearch")]
    meili: Option<Arc<engram::storage::MeilisearchBackend>>,
    /// Meilisearch indexer for reindex operations
    #[cfg(feature = "meilisearch")]
    meili_indexer: Option<Arc<MeilisearchIndexer>>,
    /// Meilisearch sync interval config
    #[cfg(feature = "meilisearch")]
    meili_sync_interval: u64,
    /// Dedicated Tokio runtime for async operations (Langfuse sync)
    #[cfg(feature = "langfuse")]
    langfuse_runtime: tokio::runtime::Runtime,
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
            search_cache: Arc::new(engram::search::SearchResultCache::new(
                engram::search::AdaptiveCacheConfig::default(),
            )),
            #[cfg(feature = "meilisearch")]
            meili: None,
            #[cfg(feature = "meilisearch")]
            meili_indexer: None,
            #[cfg(feature = "meilisearch")]
            meili_sync_interval: 60,
            #[cfg(feature = "langfuse")]
            langfuse_runtime: tokio::runtime::Runtime::new()
                .expect("Failed to create Langfuse runtime"),
        }
    }

    fn with_realtime(mut self, manager: RealtimeManager) -> Self {
        self.realtime = Some(manager);
        self
    }

    /// Build a `HandlerContext` from this handler's shared state and delegate
    /// to the domain-module dispatch function.
    fn handle_tool_call(&self, name: &str, params: Value) -> Value {
        let ctx = self.make_context();
        handlers::dispatch(&ctx, name, params)
    }

    /// Construct a `HandlerContext` from this handler's shared state.
    fn make_context(&self) -> handlers::HandlerContext {
        handlers::HandlerContext {
            storage: self.storage.clone(),
            embedder: self.embedder.clone(),
            fuzzy_engine: self.fuzzy_engine.clone(),
            search_config: self.search_config.clone(),
            realtime: self.realtime.clone(),
            embedding_cache: self.embedding_cache.clone(),
            search_cache: self.search_cache.clone(),
            #[cfg(feature = "meilisearch")]
            meili: self.meili.clone(),
            #[cfg(feature = "meilisearch")]
            meili_indexer: self.meili_indexer.clone(),
            #[cfg(feature = "meilisearch")]
            meili_sync_interval: self.meili_sync_interval,
            #[cfg(feature = "langfuse")]
            langfuse_runtime: Arc::new(
                tokio::runtime::Runtime::new()
                    .expect("Failed to create per-request Langfuse runtime"),
            ),
        }
    }
}

impl McpHandler for EngramHandler {
    fn handle_request(&self, request: McpRequest) -> McpResponse {
        match request.method.as_str() {
            methods::INITIALIZE => {
                // Negotiate protocol version: if the client requests the legacy version, respond
                // with that version and omit resources/prompts from capabilities.
                let client_version = request
                    .params
                    .get("protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or(MCP_PROTOCOL_VERSION);

                let result = if client_version == MCP_PROTOCOL_VERSION_LEGACY {
                    // Legacy mode: respond with 2024-11-05, no resources/prompts capabilities
                    InitializeResult {
                        protocol_version: MCP_PROTOCOL_VERSION_LEGACY.to_string(),
                        capabilities: ServerCapabilities {
                            tools: Some(ToolsCapability {
                                list_changed: false,
                            }),
                            resources: None,
                            prompts: None,
                        },
                        ..InitializeResult::default()
                    }
                } else {
                    // Current mode: 2025-11-25 with full capabilities
                    InitializeResult {
                        protocol_version: MCP_PROTOCOL_VERSION.to_string(),
                        capabilities: ServerCapabilities {
                            tools: Some(ToolsCapability {
                                list_changed: false,
                            }),
                            resources: Some(ResourceCapabilities {
                                subscribe: false,
                                list_changed: false,
                            }),
                            prompts: Some(PromptCapabilities {
                                list_changed: false,
                            }),
                        },
                        ..InitializeResult::default()
                    }
                };

                McpResponse::success(request.id, json!(result))
            }
            methods::INITIALIZED => {
                // Notification — MCP spec says no response should be sent.
                // Return a response with id=None so the server loop can skip it.
                McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: None,
                }
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
            methods::LIST_RESOURCES => {
                let templates = list_resources();
                let resources: Vec<Value> = templates
                    .into_iter()
                    .map(|t| {
                        json!({
                            "uri": t.uri_template,
                            "name": t.name,
                            "description": t.description,
                            "mimeType": t.mime_type,
                        })
                    })
                    .collect();
                McpResponse::success(request.id, json!({"resources": resources}))
            }
            methods::READ_RESOURCE => {
                let uri = match request.params.get("uri").and_then(|v| v.as_str()) {
                    Some(u) => u.to_string(),
                    None => {
                        return McpResponse::error(
                            request.id,
                            -32602,
                            "Missing required parameter: uri".to_string(),
                        )
                    }
                };

                match read_resource(&self.storage, &uri) {
                    Ok(content) => {
                        let text = serde_json::to_string_pretty(&content)
                            .unwrap_or_else(|_| content.to_string());
                        McpResponse::success(
                            request.id,
                            json!({
                                "contents": [{
                                    "uri": uri,
                                    "mimeType": "application/json",
                                    "text": text,
                                }]
                            }),
                        )
                    }
                    Err(msg) => McpResponse::error(request.id, -32602, msg),
                }
            }
            methods::LIST_PROMPTS => {
                let prompts = list_prompts();
                McpResponse::success(request.id, json!({"prompts": prompts}))
            }
            methods::GET_PROMPT => {
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
                match get_prompt(name, &arguments) {
                    Ok(messages) => McpResponse::success(request.id, json!({"messages": messages})),
                    Err(e) => McpResponse::error(request.id, -32002, e),
                }
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

    #[cfg(feature = "meilisearch")]
    let mut meili_backend_for_handler: Option<Arc<MeilisearchBackend>> = None;
    #[cfg(feature = "meilisearch")]
    let mut meili_indexer_for_handler: Option<Arc<MeilisearchIndexer>> = None;
    #[cfg(feature = "meilisearch")]
    let meili_sync_interval = args.meilisearch_sync_interval;

    #[cfg(feature = "meilisearch")]
    {
        if let Some(url) = args.meilisearch_url.as_deref() {
            let meili = Arc::new(MeilisearchBackend::new(
                url,
                args.meilisearch_api_key.as_deref(),
            )?);
            meili_backend_for_handler = Some(meili.clone());

            if args.meilisearch_indexer {
                let sqlite_backend = SqliteBackend::new(config.clone())?;
                let indexer = Arc::new(MeilisearchIndexer::new(
                    Arc::new(sqlite_backend),
                    meili.clone(),
                    args.meilisearch_sync_interval,
                ));
                meili_indexer_for_handler = Some(indexer.clone());

                let indexer_bg = indexer.clone();
                std::thread::spawn(move || {
                    let rt =
                        tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                    rt.block_on(indexer_bg.start());
                });
            } else {
                tracing::info!(
                    "Meilisearch URL provided but indexer disabled. Set --meilisearch-indexer to enable."
                );
            }
        }
    }

    // Create embedder
    // Determine dimensions: use explicit config, or default based on model
    let dimensions = args.openai_embedding_dimensions.unwrap_or_else(|| {
        if args.embedding_model == "openai" {
            1536 // Default for text-embedding-3-small
        } else {
            384 // Default for TF-IDF
        }
    });

    let embedding_config = EmbeddingConfig {
        model: args.embedding_model,
        api_key: args.openai_key,
        base_url: if args.openai_base_url == "https://api.openai.com/v1" {
            None // Use default
        } else {
            Some(args.openai_base_url)
        },
        embedding_model: Some(args.openai_embedding_model),
        model_path: None,
        dimensions,
        batch_size: 100,
    };
    let embedder = create_embedder(&embedding_config)?;

    // Create real-time manager.
    // Always created so both the WebSocket server (when ws_port > 0) and
    // the HTTP SSE endpoint (GET /v1/events) can share the same broadcast channel.
    let realtime_manager = Some(RealtimeManager::new());

    // Create handler and server
    let mut handler = EngramHandler::new(storage.clone(), embedder);
    if let Some(ref manager) = realtime_manager {
        handler = handler.with_realtime(manager.clone());
    }
    #[cfg(feature = "meilisearch")]
    {
        handler.meili = meili_backend_for_handler;
        handler.meili_indexer = meili_indexer_for_handler;
        handler.meili_sync_interval = meili_sync_interval;
    }
    let handler = Arc::new(handler);
    let server = McpServer::new(handler.clone());

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

    // Start background compression scheduler if enabled
    if args.compression_interval_seconds > 0 {
        let compression_storage = storage.clone();
        let interval = std::time::Duration::from_secs(args.compression_interval_seconds);
        let max_age = args.compression_max_age_days;
        let max_imp = args.compression_max_importance;
        let min_acc = args.compression_min_access;

        std::thread::spawn(move || {
            tracing::info!(
                "Compression scheduler started (interval: {}s, max_age: {}d, max_importance: {}, min_access: {})",
                interval.as_secs(),
                max_age,
                max_imp,
                min_acc,
            );

            loop {
                std::thread::sleep(interval);

                match compression_storage.with_transaction(|conn| {
                    engram::storage::queries::compress_old_memories(
                        conn, max_age, max_imp, min_acc, 100, // batch limit per cycle
                    )
                }) {
                    Ok(archived) => {
                        if archived > 0 {
                            tracing::info!("Compression scheduler archived {} memories", archived);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Compression scheduler error: {}", e);
                    }
                }
            }
        });
    }

    // Start WebSocket server in background if ws_port > 0.
    // Clone the manager so it can also be shared with the HTTP transport SSE endpoint.
    if args.ws_port > 0 {
        if let Some(ref manager) = realtime_manager {
            let ws_manager = manager.clone();
            let ws_port = args.ws_port;
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                rt.block_on(async {
                    let ws_server = RealtimeServer::new(ws_manager, ws_port);
                    tracing::info!("WebSocket server starting on port {}...", ws_port);
                    if let Err(e) = ws_server.start().await {
                        tracing::error!("WebSocket server error: {}", e);
                    }
                });
            });
        }
    }

    tracing::info!("Engram MCP server starting...");

    match args.transport {
        TransportMode::Stdio => {
            server.run()?;
        }
        TransportMode::Http => {
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| engram::error::EngramError::Internal(e.to_string()))?;
            rt.block_on(async {
                http_transport::serve_http(
                    handler,
                    args.http_port,
                    args.http_api_key,
                    realtime_manager,
                )
                .await
                .map_err(|e| engram::error::EngramError::Internal(e.to_string()))
            })?;
        }
        TransportMode::Both => {
            let http_handler = handler.clone();
            let http_port = args.http_port;
            let http_api_key = args.http_api_key.clone();
            let http_realtime = realtime_manager.clone();

            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new()
                    .expect("Failed to create HTTP transport runtime");
                rt.block_on(async {
                    if let Err(e) = http_transport::serve_http(
                        http_handler,
                        http_port,
                        http_api_key,
                        http_realtime,
                    )
                    .await
                    {
                        tracing::error!("HTTP transport error: {}", e);
                    }
                });
            });

            // Run stdio in the main thread
            server.run()?;
        }
        #[cfg(feature = "grpc")]
        TransportMode::Grpc => {
            use engram::mcp::grpc_transport;

            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| engram::error::EngramError::Internal(e.to_string()))?;
            rt.block_on(async {
                grpc_transport::serve_grpc(
                    handler,
                    args.grpc_port,
                    args.grpc_api_key,
                    realtime_manager,
                )
                .await
                .map_err(|e| engram::error::EngramError::Internal(e.to_string()))
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_handler() -> EngramHandler {
        let storage = Storage::open_in_memory().unwrap();
        let embedder = create_embedder(&EmbeddingConfig::default()).unwrap();
        EngramHandler {
            storage: storage.clone(),
            search_cache: Arc::new(engram::search::result_cache::SearchResultCache::new(
                Default::default(),
            )),
            embedder,
            fuzzy_engine: Arc::new(Mutex::new(FuzzyEngine::new())),
            search_config: SearchConfig::default(),
            realtime: None,
            embedding_cache: Arc::new(engram::embedding::EmbeddingCache::default()),
            #[cfg(feature = "langfuse")]
            langfuse_runtime: tokio::runtime::Runtime::new()
                .expect("Failed to create Langfuse runtime"),
            #[cfg(feature = "meilisearch")]
            meili: None,
            #[cfg(feature = "meilisearch")]
            meili_indexer: None,
            #[cfg(feature = "meilisearch")]
            meili_sync_interval: 300,
        }
    }

    #[test]
    fn test_tool_ingest_document_idempotent() {
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let file_path = dir.path().join("doc.md");
        std::fs::write(&file_path, "# Title\n\nHello world.\n").unwrap();

        let handler = test_handler();

        let first = handler.handle_tool_call(
            "memory_ingest_document",
            json!({
                "path": file_path.to_string_lossy(),
                "format": "md"
            }),
        );
        assert!(first.get("error").is_none(), "first ingest error: {first}");
        assert!(
            first
                .get("chunks_created")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                > 0
        );

        let second = handler.handle_tool_call(
            "memory_ingest_document",
            json!({
                "path": file_path.to_string_lossy(),
                "format": "md"
            }),
        );
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
