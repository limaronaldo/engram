//! Integration tests for the gRPC transport.
//!
//! Starts an in-process `serve_grpc()` server on a random port, then exercises
//! all 7 test scenarios via a real tonic gRPC client.
//!
//! Run with:
//!   cargo test --test grpc_transport --features grpc -- --nocapture
//!
//! The tests share a single server spawned by `server_addr()`.

#![cfg(feature = "grpc")]

use std::net::SocketAddr;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;
use tonic::{Code, Request};

use engram::embedding::{create_embedder, EmbeddingCache};
use engram::mcp::grpc_transport::proto::mcp_service_client::McpServiceClient;
use engram::mcp::grpc_transport::proto::{
    McpRequest as ProtoRequest, SubscribeRequest,
};
use engram::mcp::grpc_transport::serve_grpc;
use engram::mcp::{
    get_tool_definitions, handlers, methods, InitializeResult, McpHandler, McpRequest, McpResponse,
    ToolsCapability, ServerCapabilities, MCP_PROTOCOL_VERSION,
};
use engram::search::{AdaptiveCacheConfig, FuzzyEngine, SearchConfig, SearchResultCache};
use engram::storage::Storage;
use engram::types::EmbeddingConfig;

// ---------------------------------------------------------------------------
// Test handler
// ---------------------------------------------------------------------------

/// A complete `McpHandler` backed by in-memory storage, suitable for tests.
struct TestHandler {
    storage: Storage,
    ctx: handlers::HandlerContext,
}

impl TestHandler {
    fn new() -> Self {
        let storage = Storage::open_in_memory().expect("in-memory storage");
        let embedder = create_embedder(&EmbeddingConfig::default()).expect("tfidf embedder");
        let ctx = handlers::HandlerContext {
            storage: storage.clone(),
            embedder,
            fuzzy_engine: Arc::new(Mutex::new(FuzzyEngine::new())),
            search_config: SearchConfig::default(),
            realtime: None,
            embedding_cache: Arc::new(EmbeddingCache::default()),
            search_cache: Arc::new(SearchResultCache::new(AdaptiveCacheConfig::default())),
            #[cfg(feature = "meilisearch")]
            meili: None,
            #[cfg(feature = "meilisearch")]
            meili_indexer: None,
            #[cfg(feature = "meilisearch")]
            meili_sync_interval: 60,
            #[cfg(feature = "langfuse")]
            langfuse_runtime: Arc::new(
                tokio::runtime::Runtime::new().expect("langfuse runtime"),
            ),
        };
        Self { storage, ctx }
    }
}

impl McpHandler for TestHandler {
    fn handle_request(&self, request: McpRequest) -> McpResponse {
        match request.method.as_str() {
            methods::INITIALIZE => {
                let result = InitializeResult {
                    protocol_version: MCP_PROTOCOL_VERSION.to_string(),
                    capabilities: ServerCapabilities {
                        tools: Some(ToolsCapability { list_changed: false }),
                        resources: None,
                        prompts: None,
                    },
                    ..InitializeResult::default()
                };
                McpResponse::success(request.id, json!(result))
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
                let result = handlers::dispatch(&self.ctx, name, arguments);
                use engram::mcp::ToolCallResult;
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

// ---------------------------------------------------------------------------
// Server fixture
// ---------------------------------------------------------------------------

/// Pick an ephemeral port by binding to port 0, then release it.
fn pick_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port 0");
    listener.local_addr().expect("local addr").port()
}

/// Spawn a gRPC server in the background and return its address.
///
/// The server runs for the lifetime of the test process.
async fn start_server(api_key: Option<String>) -> SocketAddr {
    let port = pick_free_port();
    let handler: Arc<dyn McpHandler> = Arc::new(TestHandler::new());
    let api_key_clone = api_key.clone();

    tokio::spawn(async move {
        serve_grpc(handler, port, api_key_clone, None)
            .await
            .expect("grpc server failed");
    });

    // Give the server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    format!("127.0.0.1:{port}").parse().expect("valid addr")
}

/// Connect a tonic client to `addr`.
async fn connect(addr: SocketAddr) -> McpServiceClient<Channel> {
    let endpoint = format!("http://{addr}");
    McpServiceClient::connect(endpoint)
        .await
        .expect("connect to test grpc server")
}

// ---------------------------------------------------------------------------
// Helper: build a plain ProtoRequest
// ---------------------------------------------------------------------------

fn req(id: &str, method: &str, params: Value) -> ProtoRequest {
    ProtoRequest {
        id: id.to_string(),
        method: method.to_string(),
        params_json: serde_json::to_string(&params).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Scenario a: Call `initialize` — returns server info
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scenario_a_initialize_returns_server_info() {
    let addr = start_server(None).await;
    let mut client = connect(addr).await;

    let resp = client
        .call(Request::new(req(
            "1",
            methods::INITIALIZE,
            json!({"protocolVersion": MCP_PROTOCOL_VERSION}),
        )))
        .await
        .expect("call initialize")
        .into_inner();

    assert_eq!(resp.id, "1");

    let result_json = match resp.result.expect("result present") {
        engram::mcp::grpc_transport::proto::mcp_response::Result::ResultJson(j) => j,
        other => panic!("expected ResultJson, got {:?}", other),
    };

    let parsed: Value = serde_json::from_str(&result_json).expect("valid json");
    assert_eq!(
        parsed["protocolVersion"].as_str(),
        Some(MCP_PROTOCOL_VERSION),
        "server must echo the current protocol version"
    );
    assert!(
        parsed["capabilities"]["tools"].is_object(),
        "capabilities.tools must be present"
    );
}

// ---------------------------------------------------------------------------
// Scenario b: Call `tools/list` — returns tool list
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scenario_b_tools_list_returns_tools() {
    let addr = start_server(None).await;
    let mut client = connect(addr).await;

    let resp = client
        .call(Request::new(req("2", methods::LIST_TOOLS, json!({}))))
        .await
        .expect("call tools/list")
        .into_inner();

    assert_eq!(resp.id, "2");

    let result_json = match resp.result.expect("result present") {
        engram::mcp::grpc_transport::proto::mcp_response::Result::ResultJson(j) => j,
        other => panic!("expected ResultJson, got {:?}", other),
    };

    let parsed: Value = serde_json::from_str(&result_json).expect("valid json");
    let tools = parsed["tools"].as_array().expect("tools must be array");
    assert!(
        !tools.is_empty(),
        "tool list must contain at least one tool"
    );
    // Verify at least memory_create is present
    let has_memory_create = tools
        .iter()
        .any(|t| t["name"].as_str() == Some("memory_create"));
    assert!(has_memory_create, "tools/list must include memory_create");
}

// ---------------------------------------------------------------------------
// Scenario c: Call `tools/call` + `memory_create` — creates a memory
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scenario_c_memory_create_returns_id() {
    let addr = start_server(None).await;
    let mut client = connect(addr).await;

    let resp = client
        .call(Request::new(req(
            "3",
            methods::CALL_TOOL,
            json!({
                "name": "memory_create",
                "arguments": {
                    "content": "gRPC integration test memory",
                    "memory_type": "note"
                }
            }),
        )))
        .await
        .expect("call memory_create")
        .into_inner();

    assert_eq!(resp.id, "3");

    let result_json = match resp.result.expect("result present") {
        engram::mcp::grpc_transport::proto::mcp_response::Result::ResultJson(j) => j,
        other => panic!("expected ResultJson, got {:?}", other),
    };

    let parsed: Value = serde_json::from_str(&result_json).expect("valid json");
    // ToolCallResult wraps output in content[0].text
    let text = parsed["content"][0]["text"]
        .as_str()
        .expect("content[0].text must be a string");
    let inner: Value = serde_json::from_str(text).expect("content text must be JSON");
    assert!(
        inner["id"].is_number() || inner["id"].is_string(),
        "memory_create must return an id"
    );
}

// ---------------------------------------------------------------------------
// Scenario d: Call `tools/call` + `memory_search` — returns results
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scenario_d_memory_search_returns_results() {
    // Use a fresh server with a pre-created memory
    let addr = start_server(None).await;
    let mut client = connect(addr).await;

    // Create a memory first
    client
        .call(Request::new(req(
            "seed",
            methods::CALL_TOOL,
            json!({
                "name": "memory_create",
                "arguments": {
                    "content": "searchable gRPC test content alpha",
                    "memory_type": "note"
                }
            }),
        )))
        .await
        .expect("seed memory");

    // Now search for it
    let resp = client
        .call(Request::new(req(
            "4",
            methods::CALL_TOOL,
            json!({
                "name": "memory_search",
                "arguments": {
                    "query": "gRPC test content alpha"
                }
            }),
        )))
        .await
        .expect("call memory_search")
        .into_inner();

    assert_eq!(resp.id, "4");

    let result_json = match resp.result.expect("result present") {
        engram::mcp::grpc_transport::proto::mcp_response::Result::ResultJson(j) => j,
        other => panic!("expected ResultJson, got {:?}", other),
    };

    let parsed: Value = serde_json::from_str(&result_json).expect("valid json");
    let text = parsed["content"][0]["text"]
        .as_str()
        .expect("content[0].text must be a string");
    let inner: Value = serde_json::from_str(text).expect("content text must be JSON");
    // memory_search returns either:
    // - a top-level JSON array of match objects, OR
    // - an object with a `results` or `memories` key
    let results: &Vec<Value> = if let Some(arr) = inner.as_array() {
        arr
    } else if let Some(arr) = inner.get("results").and_then(|v| v.as_array()) {
        arr
    } else if let Some(arr) = inner.get("memories").and_then(|v| v.as_array()) {
        arr
    } else {
        panic!(
            "memory_search response must be an array or have results/memories key, got: {}",
            inner
        );
    };
    assert!(
        !results.is_empty(),
        "memory_search must return at least one result for a seeded memory"
    );
}

// ---------------------------------------------------------------------------
// Scenario e: Auth test — call without token when token required → UNAUTHENTICATED
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scenario_e_missing_token_is_unauthenticated() {
    let addr = start_server(Some("secret-token".to_string())).await;
    let mut client = connect(addr).await;

    let err = client
        .call(Request::new(req("5", methods::LIST_TOOLS, json!({}))))
        .await
        .expect_err("call without token should fail");

    assert_eq!(
        err.code(),
        Code::Unauthenticated,
        "missing token must return UNAUTHENTICATED, got: {:?}",
        err
    );
}

// ---------------------------------------------------------------------------
// Scenario f: Auth test — call with correct token → succeeds
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scenario_f_correct_token_succeeds() {
    let addr = start_server(Some("secret-token".to_string())).await;
    let mut client = connect(addr).await;

    let mut request = Request::new(req("6", methods::LIST_TOOLS, json!({})));
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::from_static("Bearer secret-token"),
    );

    let resp = client.call(request).await.expect("call with correct token");
    assert_eq!(resp.into_inner().id, "6");
}

// ---------------------------------------------------------------------------
// Scenario g: Unknown method → error response (not a transport error)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scenario_g_unknown_method_returns_error_response() {
    let addr = start_server(None).await;
    let mut client = connect(addr).await;

    let resp = client
        .call(Request::new(req("7", "unknown/method/xyz", json!({}))))
        .await
        .expect("transport should succeed even for unknown method")
        .into_inner();

    assert_eq!(resp.id, "7");

    match resp.result.expect("result present") {
        engram::mcp::grpc_transport::proto::mcp_response::Result::Error(err) => {
            assert_eq!(err.code, -32601, "unknown method should return -32601");
            assert!(
                err.message.contains("Method not found"),
                "error message should mention Method not found, got: {}",
                err.message
            );
        }
        other => panic!("expected Error variant for unknown method, got {:?}", other),
    }
}
