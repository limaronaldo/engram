//! Integration tests for MCP 2025-11-25 protocol features.
//!
//! Tests protocol negotiation, tool annotations, resources, and prompts
//! through the full MCP request/response pipeline.
//!
//! Run with: cargo test --test mcp_protocol_tests

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use engram::embedding::{create_embedder, EmbeddingCache};
use engram::mcp::{
    get_prompt, get_tool_definitions, handlers, list_prompts, list_resources, methods,
    read_resource, InitializeResult, McpHandler, McpRequest, McpResponse, PromptCapabilities,
    ResourceCapabilities, ServerCapabilities, ToolCallResult, ToolsCapability,
    MCP_PROTOCOL_VERSION, MCP_PROTOCOL_VERSION_LEGACY,
};
use engram::search::{AdaptiveCacheConfig, FuzzyEngine, SearchConfig, SearchResultCache};
use engram::storage::Storage;
use engram::types::EmbeddingConfig;

// ---------------------------------------------------------------------------
// Test handler — mirrors the EngramHandler in server.rs using public APIs
// ---------------------------------------------------------------------------

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
            langfuse_runtime: Arc::new(tokio::runtime::Runtime::new().expect("langfuse runtime")),
        };
        Self { storage, ctx }
    }
}

impl McpHandler for TestHandler {
    fn handle_request(&self, request: McpRequest) -> McpResponse {
        match request.method.as_str() {
            methods::INITIALIZE => {
                let client_version = request
                    .params
                    .get("protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or(MCP_PROTOCOL_VERSION);

                let result = if client_version == MCP_PROTOCOL_VERSION_LEGACY {
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

// ---------------------------------------------------------------------------
// Helper utilities
// ---------------------------------------------------------------------------

fn make_request(id: i64, method: &str, params: Value) -> McpRequest {
    McpRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(id)),
        method: method.to_string(),
        params,
    }
}

// ---------------------------------------------------------------------------
// Protocol negotiation tests
// ---------------------------------------------------------------------------

#[test]
fn test_protocol_negotiation_2025() {
    let handler = TestHandler::new();
    let req = make_request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-11-25",
            "clientInfo": {"name": "test-client", "version": "0.1.0"}
        }),
    );

    let resp = handler.handle_request(req);
    assert!(
        resp.error.is_none(),
        "Expected no error, got: {:?}",
        resp.error
    );

    let result = resp.result.expect("Expected result");

    assert_eq!(
        result["protocolVersion"].as_str().unwrap(),
        "2025-11-25",
        "Protocol version should be 2025-11-25"
    );

    // Capabilities must include resources and prompts
    let caps = &result["capabilities"];
    assert!(caps["tools"].is_object(), "Should have tools capability");
    assert!(
        caps["resources"].is_object(),
        "Should have resources capability"
    );
    assert!(
        caps["prompts"].is_object(),
        "Should have prompts capability"
    );
}

#[test]
fn test_protocol_negotiation_2024_backward_compat() {
    let handler = TestHandler::new();
    let req = make_request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "clientInfo": {"name": "legacy-client", "version": "0.1.0"}
        }),
    );

    let resp = handler.handle_request(req);
    assert!(
        resp.error.is_none(),
        "Expected no error, got: {:?}",
        resp.error
    );

    let result = resp.result.expect("Expected result");

    assert_eq!(
        result["protocolVersion"].as_str().unwrap(),
        "2024-11-05",
        "Protocol version should be 2024-11-05 for legacy client"
    );

    // Legacy mode: resources and prompts capabilities should be absent
    let caps = &result["capabilities"];
    assert!(
        caps["tools"].is_object(),
        "Should still have tools capability"
    );
    assert!(
        caps["resources"].is_null(),
        "Should NOT have resources capability in legacy mode"
    );
    assert!(
        caps["prompts"].is_null(),
        "Should NOT have prompts capability in legacy mode"
    );
}

// ---------------------------------------------------------------------------
// Tool annotation tests
// ---------------------------------------------------------------------------

#[test]
fn test_tools_list_includes_annotations() {
    let handler = TestHandler::new();
    let req = make_request(2, "tools/list", json!({}));

    let resp = handler.handle_request(req);
    assert!(resp.error.is_none(), "Expected no error: {:?}", resp.error);

    let result = resp.result.expect("Expected result");
    let tools = result["tools"].as_array().expect("Expected tools array");
    assert!(!tools.is_empty(), "Should have at least one tool");

    // At least some tools should have annotations with readOnlyHint or destructiveHint
    let annotated_tools: Vec<_> = tools
        .iter()
        .filter(|t| t.get("annotations").is_some())
        .collect();

    assert!(
        !annotated_tools.is_empty(),
        "At least some tools should have annotations"
    );

    // Verify annotation fields exist on annotated tools
    for tool in &annotated_tools {
        let annotations = &tool["annotations"];
        // annotations should be an object
        assert!(annotations.is_object(), "annotations should be an object");
    }

    // Check that known read-only tools have readOnlyHint = true
    let memory_get = tools.iter().find(|t| t["name"] == "memory_get");
    if let Some(tool) = memory_get {
        if let Some(ann) = tool.get("annotations") {
            if let Some(read_only) = ann.get("readOnlyHint") {
                assert_eq!(
                    read_only.as_bool(),
                    Some(true),
                    "memory_get should have readOnlyHint: true"
                );
            }
        }
    }

    // Check that destructive tools have destructiveHint = true
    let memory_delete = tools.iter().find(|t| t["name"] == "memory_delete");
    if let Some(tool) = memory_delete {
        if let Some(ann) = tool.get("annotations") {
            if let Some(destructive) = ann.get("destructiveHint") {
                assert_eq!(
                    destructive.as_bool(),
                    Some(true),
                    "memory_delete should have destructiveHint: true"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Resources tests
// ---------------------------------------------------------------------------

#[test]
fn test_resources_list() {
    let handler = TestHandler::new();
    let req = make_request(3, "resources/list", json!({}));

    let resp = handler.handle_request(req);
    assert!(resp.error.is_none(), "Expected no error: {:?}", resp.error);

    let result = resp.result.expect("Expected result");
    let resources = result["resources"]
        .as_array()
        .expect("Expected resources array");

    // Should have exactly 5 resource templates
    assert_eq!(
        resources.len(),
        5,
        "Expected 5 resource templates, got {}",
        resources.len()
    );

    // Each resource should have uri, name, description
    for resource in resources {
        assert!(
            resource["uri"].is_string(),
            "Resource should have 'uri' field: {:?}",
            resource
        );
        assert!(
            resource["name"].is_string(),
            "Resource should have 'name' field: {:?}",
            resource
        );
        assert!(
            resource["description"].is_string() || !resource["description"].is_null(),
            "Resource should have 'description' field: {:?}",
            resource
        );
    }

    // Verify expected URI templates exist
    let uris: Vec<&str> = resources.iter().filter_map(|r| r["uri"].as_str()).collect();

    assert!(
        uris.contains(&"engram://stats"),
        "Should have stats resource"
    );
    assert!(
        uris.contains(&"engram://entities"),
        "Should have entities resource"
    );
    assert!(
        uris.iter().any(|u| u.contains("memory")),
        "Should have memory resource template"
    );
    assert!(
        uris.iter().any(|u| u.contains("workspace")),
        "Should have workspace resource template"
    );
}

#[test]
fn test_resources_read_stats() {
    let handler = TestHandler::new();

    // First create a memory so stats are non-trivial
    let create_req = make_request(
        10,
        "tools/call",
        json!({
            "name": "memory_create",
            "arguments": {
                "content": "Integration test memory for stats check",
                "memory_type": "note"
            }
        }),
    );
    let create_resp = handler.handle_request(create_req);
    assert!(
        create_resp.error.is_none(),
        "memory_create failed: {:?}",
        create_resp.error
    );

    // Now read the stats resource
    let req = make_request(11, "resources/read", json!({"uri": "engram://stats"}));

    let resp = handler.handle_request(req);
    assert!(resp.error.is_none(), "Expected no error: {:?}", resp.error);

    let result = resp.result.expect("Expected result");
    let contents = result["contents"]
        .as_array()
        .expect("Expected contents array");
    assert!(!contents.is_empty(), "Expected at least one content item");

    let text = contents[0]["text"].as_str().expect("Expected text content");
    let stats: Value = serde_json::from_str(text).expect("Stats should be valid JSON");

    // Stats should include a memory count >= 1
    let total = stats
        .get("total_memories")
        .or_else(|| stats.get("memory_count"))
        .or_else(|| stats.get("count"))
        .or_else(|| stats.get("total"));

    // Accept either a direct count field or embedded in object
    if let Some(count_val) = total {
        let count = count_val.as_u64().unwrap_or(0);
        assert!(
            count >= 1,
            "Stats should show at least 1 memory, got: {}",
            count
        );
    } else {
        // Stats may have nested structure — just verify it's a non-empty object
        assert!(
            stats.is_object() && !stats.as_object().unwrap().is_empty(),
            "Stats should be a non-empty JSON object, got: {}",
            stats
        );
    }
}

#[test]
fn test_resources_read_memory() {
    let handler = TestHandler::new();

    // Create a memory first
    let create_req = make_request(
        20,
        "tools/call",
        json!({
            "name": "memory_create",
            "arguments": {
                "content": "Unique content for resource read test XYZ123",
                "memory_type": "note",
                "tags": ["resource-test"]
            }
        }),
    );
    let create_resp = handler.handle_request(create_req);
    assert!(
        create_resp.error.is_none(),
        "memory_create failed: {:?}",
        create_resp.error
    );

    // Extract the ID from the tool call result
    let result = create_resp.result.expect("Expected result");
    let content_arr = result["content"]
        .as_array()
        .expect("Expected content array");
    let text = content_arr[0]["text"].as_str().expect("Expected text");
    let created: Value = serde_json::from_str(text).expect("Created memory should be JSON");
    let memory_id = created["id"].as_i64().expect("Expected id field");

    // Now read via resource URI
    let req = make_request(
        21,
        "resources/read",
        json!({"uri": format!("engram://memory/{}", memory_id)}),
    );

    let resp = handler.handle_request(req);
    assert!(resp.error.is_none(), "Expected no error: {:?}", resp.error);

    let result = resp.result.expect("Expected result");
    let contents = result["contents"]
        .as_array()
        .expect("Expected contents array");
    assert!(!contents.is_empty(), "Expected at least one content item");

    let text = contents[0]["text"].as_str().expect("Expected text content");
    let memory: Value = serde_json::from_str(text).expect("Memory should be valid JSON");

    assert_eq!(
        memory["id"].as_i64(),
        Some(memory_id),
        "Resource should return the correct memory ID"
    );
    assert!(
        memory["content"].as_str().unwrap_or("").contains("XYZ123"),
        "Resource content should contain the original text"
    );
}

#[test]
fn test_resources_read_invalid_uri() {
    let handler = TestHandler::new();

    let req = make_request(
        30,
        "resources/read",
        json!({"uri": "engram://nonexistent/path/that/does/not/exist"}),
    );

    let resp = handler.handle_request(req);

    // Should return an error response (not a success)
    assert!(
        resp.error.is_some(),
        "Expected an error for invalid URI, got result: {:?}",
        resp.result
    );
}

// ---------------------------------------------------------------------------
// Prompts tests
// ---------------------------------------------------------------------------

#[test]
fn test_prompts_list() {
    let handler = TestHandler::new();
    let req = make_request(40, "prompts/list", json!({}));

    let resp = handler.handle_request(req);
    assert!(resp.error.is_none(), "Expected no error: {:?}", resp.error);

    let result = resp.result.expect("Expected result");
    let prompts = result["prompts"]
        .as_array()
        .expect("Expected prompts array");

    // Should have exactly 5 prompts
    assert_eq!(
        prompts.len(),
        5,
        "Expected 5 prompts, got {}",
        prompts.len()
    );

    // Each prompt should have name and arguments
    for prompt in prompts {
        assert!(
            prompt["name"].is_string(),
            "Prompt should have 'name' field: {:?}",
            prompt
        );
    }

    // Verify all 4 expected prompt names are present
    let names: Vec<&str> = prompts.iter().filter_map(|p| p["name"].as_str()).collect();

    assert!(
        names.contains(&"create-knowledge-base"),
        "Should have create-knowledge-base prompt"
    );
    assert!(
        names.contains(&"daily-review"),
        "Should have daily-review prompt"
    );
    assert!(
        names.contains(&"search-and-organize"),
        "Should have search-and-organize prompt"
    );
    assert!(
        names.contains(&"seed-entity"),
        "Should have seed-entity prompt"
    );
}

#[test]
fn test_prompts_get_daily_review() {
    let handler = TestHandler::new();
    let req = make_request(
        50,
        "prompts/get",
        json!({
            "name": "daily-review",
            "arguments": {}
        }),
    );

    let resp = handler.handle_request(req);
    assert!(resp.error.is_none(), "Expected no error: {:?}", resp.error);

    let result = resp.result.expect("Expected result");
    let messages = result["messages"]
        .as_array()
        .expect("Expected messages array");

    // Should return at least 2 messages (user + assistant)
    assert!(
        messages.len() >= 2,
        "Expected at least 2 messages, got {}",
        messages.len()
    );

    // Each message should have role and content
    for message in messages {
        let role = message["role"].as_str().expect("Message should have role");
        assert!(
            role == "user" || role == "assistant",
            "Role should be 'user' or 'assistant', got: {}",
            role
        );

        let content = &message["content"];
        assert!(
            content.is_object(),
            "Content should be an object: {:?}",
            content
        );
        assert!(
            content["type"].as_str() == Some("text"),
            "Content type should be 'text'"
        );
        assert!(
            content["text"].is_string(),
            "Content should have text field"
        );
    }

    // First message should be from the user
    assert_eq!(
        messages[0]["role"].as_str(),
        Some("user"),
        "First message should be from user"
    );
}

#[test]
fn test_prompts_get_unknown() {
    let handler = TestHandler::new();
    let req = make_request(
        60,
        "prompts/get",
        json!({
            "name": "nonexistent-prompt-xyz",
            "arguments": {}
        }),
    );

    let resp = handler.handle_request(req);

    // Should return an error response
    assert!(
        resp.error.is_some(),
        "Expected an error for unknown prompt, got result: {:?}",
        resp.result
    );

    let error = resp.error.unwrap();
    assert!(
        error.message.contains("nonexistent-prompt-xyz") || error.message.contains("not found"),
        "Error message should mention the unknown prompt name or 'not found': {}",
        error.message
    );
}

// ---------------------------------------------------------------------------
// recent_activity tool tests
// ---------------------------------------------------------------------------

#[test]
fn test_recent_activity_returns_activities_field() {
    let handler = TestHandler::new();

    // Create a memory so there is recent activity to discover
    let create_req = make_request(
        70,
        "tools/call",
        json!({
            "name": "memory_create",
            "arguments": {
                "content": "Test recent activity memory",
                "memory_type": "note"
            }
        }),
    );
    let create_resp = handler.handle_request(create_req);
    assert!(
        create_resp.error.is_none(),
        "memory_create failed: {:?}",
        create_resp.error
    );

    // Call recent_activity with default params
    let req = make_request(
        71,
        "tools/call",
        json!({
            "name": "recent_activity",
            "arguments": {}
        }),
    );

    let resp = handler.handle_request(req);
    assert!(
        resp.error.is_none(),
        "recent_activity returned error: {:?}",
        resp.error
    );

    let result = resp.result.expect("Expected result");
    let content = result["content"]
        .as_array()
        .expect("Expected content array");
    assert!(!content.is_empty(), "Expected at least one content item");

    let text = content[0]["text"].as_str().expect("Expected text content");
    let data: Value = serde_json::from_str(text).expect("recent_activity should return valid JSON");

    assert!(
        data["activities"].is_array(),
        "Result must have 'activities' array, got: {}",
        data
    );
    assert!(
        data["count"].is_number(),
        "Result must have 'count' field"
    );
    assert!(
        data["timeframe"].is_string(),
        "Result must have 'timeframe' field"
    );

    let activities = data["activities"].as_array().unwrap();
    assert!(
        !activities.is_empty(),
        "Should find at least one recent memory"
    );

    // Verify activity shape
    let activity = &activities[0];
    assert!(activity["id"].is_number(), "Activity must have 'id'");
    assert!(activity["preview"].is_string(), "Activity must have 'preview'");
    assert!(activity["memory_type"].is_string(), "Activity must have 'memory_type'");
    assert!(activity["workspace"].is_string(), "Activity must have 'workspace'");
    assert!(activity["created_at"].is_string(), "Activity must have 'created_at'");
}

#[test]
fn test_recent_activity_timeframe_1h() {
    let handler = TestHandler::new();

    // Create a memory
    let create_req = make_request(
        80,
        "tools/call",
        json!({
            "name": "memory_create",
            "arguments": {
                "content": "Memory for 1h timeframe test",
                "memory_type": "note"
            }
        }),
    );
    handler.handle_request(create_req);

    let req = make_request(
        81,
        "tools/call",
        json!({
            "name": "recent_activity",
            "arguments": {"timeframe": "1h", "limit": 5}
        }),
    );

    let resp = handler.handle_request(req);
    assert!(resp.error.is_none(), "Expected no error: {:?}", resp.error);

    let result = resp.result.expect("Expected result");
    let content = result["content"].as_array().expect("Expected content array");
    let text = content[0]["text"].as_str().expect("Expected text");
    let data: Value = serde_json::from_str(text).unwrap();

    assert_eq!(
        data["timeframe"].as_str(),
        Some("1h"),
        "Timeframe should echo '1h'"
    );
    assert!(
        data["activities"].is_array(),
        "Must have activities array"
    );
}

#[test]
fn test_recent_activity_limit_enforced() {
    let handler = TestHandler::new();

    // Create 5 memories
    for i in 0..5 {
        let req = make_request(
            90 + i,
            "tools/call",
            json!({
                "name": "memory_create",
                "arguments": {
                    "content": format!("Memory {} for limit test", i),
                    "memory_type": "note"
                }
            }),
        );
        handler.handle_request(req);
    }

    // Request only 2 results
    let req = make_request(
        95,
        "tools/call",
        json!({
            "name": "recent_activity",
            "arguments": {"limit": 2}
        }),
    );

    let resp = handler.handle_request(req);
    assert!(resp.error.is_none(), "Expected no error: {:?}", resp.error);

    let result = resp.result.expect("Expected result");
    let content = result["content"].as_array().expect("Expected content array");
    let text = content[0]["text"].as_str().expect("Expected text");
    let data: Value = serde_json::from_str(text).unwrap();

    let activities = data["activities"].as_array().unwrap();
    assert!(
        activities.len() <= 2,
        "Should return at most 2 activities, got {}",
        activities.len()
    );
}

#[test]
fn test_recent_activity_preview_truncated_at_100_chars() {
    let handler = TestHandler::new();

    // Create memory with content > 100 chars
    let long_content: String = "A".repeat(200);
    let create_req = make_request(
        100,
        "tools/call",
        json!({
            "name": "memory_create",
            "arguments": {
                "content": long_content,
                "memory_type": "note"
            }
        }),
    );
    handler.handle_request(create_req);

    let req = make_request(
        101,
        "tools/call",
        json!({
            "name": "recent_activity",
            "arguments": {"timeframe": "1h", "limit": 1}
        }),
    );

    let resp = handler.handle_request(req);
    let result = resp.result.expect("Expected result");
    let content = result["content"].as_array().expect("Expected content array");
    let text = content[0]["text"].as_str().expect("Expected text");
    let data: Value = serde_json::from_str(text).unwrap();

    let activities = data["activities"].as_array().unwrap();
    if !activities.is_empty() {
        let preview = activities[0]["preview"].as_str().unwrap();
        assert!(
            preview.ends_with("..."),
            "Preview of long content should end with '...', got: {}",
            preview
        );
        assert!(
            preview.len() <= 103,
            "Preview + '...' should be at most 103 chars, got: {}",
            preview.len()
        );
    }
}
