//! Integration tests for engram v0.7.0 — Reactive Infrastructure.
//!
//! Tests agent registry round-trip, namespace isolation, SSE event format,
//! MCP dispatch of agent tools, and benchmark compilation.
//!
//! Run with: cargo test --test v070_integration_tests

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::json;

use engram::embedding::{create_embedder, EmbeddingCache};
use engram::mcp::{get_tool_definitions, handlers};
use engram::search::{AdaptiveCacheConfig, FuzzyEngine, SearchConfig, SearchResultCache};
use engram::storage::Storage;
use engram::types::EmbeddingConfig;

fn test_ctx() -> handlers::HandlerContext {
    let storage = Storage::open_in_memory().expect("in-memory storage");
    let embedder = create_embedder(&EmbeddingConfig::default()).expect("tfidf embedder");
    handlers::HandlerContext {
        storage,
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
    }
}

// ---------------------------------------------------------------------------
// Agent registry round-trip via MCP dispatch
// ---------------------------------------------------------------------------

#[test]
fn test_agent_register_and_get_via_dispatch() {
    let ctx = test_ctx();
    let result = handlers::dispatch(
        &ctx,
        "agent_register",
        json!({
            "agent_id": "test-agent-1",
            "display_name": "Test Agent",
            "capabilities": ["search", "create"],
            "namespaces": ["production"]
        }),
    );
    assert_eq!(result["agent_id"], "test-agent-1");
    assert_eq!(result["display_name"], "Test Agent");
    assert_eq!(result["status"], "active");

    let get_result = handlers::dispatch(&ctx, "agent_get", json!({"agent_id": "test-agent-1"}));
    assert_eq!(get_result["display_name"], "Test Agent");
    assert!(get_result.get("error").is_none());
}

#[test]
fn test_agent_register_upsert() {
    let ctx = test_ctx();
    handlers::dispatch(
        &ctx,
        "agent_register",
        json!({"agent_id": "upsert-agent", "display_name": "V1", "capabilities": ["a"]}),
    );
    let result = handlers::dispatch(
        &ctx,
        "agent_register",
        json!({"agent_id": "upsert-agent", "display_name": "V2", "capabilities": ["a", "b"]}),
    );
    assert_eq!(result["display_name"], "V2");
    let caps = result["capabilities"].as_array().unwrap();
    assert_eq!(caps.len(), 2);
}

// ---------------------------------------------------------------------------
// Namespace isolation
// ---------------------------------------------------------------------------

#[test]
fn test_namespace_isolation() {
    let ctx = test_ctx();
    handlers::dispatch(
        &ctx,
        "agent_register",
        json!({"agent_id": "prod-agent", "namespaces": ["production"]}),
    );
    handlers::dispatch(
        &ctx,
        "agent_register",
        json!({"agent_id": "staging-agent", "namespaces": ["staging"]}),
    );
    handlers::dispatch(
        &ctx,
        "agent_register",
        json!({"agent_id": "multi-agent", "namespaces": ["production", "staging"]}),
    );

    let prod = handlers::dispatch(&ctx, "agent_list", json!({"namespace": "production"}));
    assert_eq!(
        prod["count"], 2,
        "production should have prod-agent and multi-agent"
    );

    let staging = handlers::dispatch(&ctx, "agent_list", json!({"namespace": "staging"}));
    assert_eq!(
        staging["count"], 2,
        "staging should have staging-agent and multi-agent"
    );
}

#[test]
fn test_namespace_isolation_empty() {
    let ctx = test_ctx();
    handlers::dispatch(
        &ctx,
        "agent_register",
        json!({"agent_id": "isolated", "namespaces": ["private"]}),
    );

    let result = handlers::dispatch(&ctx, "agent_list", json!({"namespace": "nonexistent"}));
    assert_eq!(result["count"], 0);
}

#[test]
fn test_namespace_with_status_filter() {
    let ctx = test_ctx();
    handlers::dispatch(
        &ctx,
        "agent_register",
        json!({"agent_id": "active-prod", "namespaces": ["prod"]}),
    );
    handlers::dispatch(
        &ctx,
        "agent_register",
        json!({"agent_id": "inactive-prod", "namespaces": ["prod"]}),
    );
    // Deregister one agent (sets status to inactive)
    handlers::dispatch(
        &ctx,
        "agent_deregister",
        json!({"agent_id": "inactive-prod"}),
    );

    // Should find only inactive agents in prod namespace
    let result = handlers::dispatch(
        &ctx,
        "agent_list",
        json!({"status": "inactive", "namespace": "prod"}),
    );
    assert_eq!(result["count"], 1, "should find 1 inactive agent in prod");
    assert_eq!(result["agents"][0]["agent_id"], "inactive-prod");

    // Active filter should find 1
    let active = handlers::dispatch(
        &ctx,
        "agent_list",
        json!({"status": "active", "namespace": "prod"}),
    );
    assert_eq!(active["count"], 1, "should find 1 active agent in prod");
}

// ---------------------------------------------------------------------------
// Agent lifecycle (heartbeat, deregister)
// ---------------------------------------------------------------------------

#[test]
fn test_agent_heartbeat_and_deregister() {
    let ctx = test_ctx();
    handlers::dispatch(
        &ctx,
        "agent_register",
        json!({"agent_id": "lifecycle-agent"}),
    );

    let hb = handlers::dispatch(
        &ctx,
        "agent_heartbeat",
        json!({"agent_id": "lifecycle-agent"}),
    );
    assert!(hb.get("last_heartbeat").is_some());
    assert_eq!(hb["status"], "active");

    let dereg = handlers::dispatch(
        &ctx,
        "agent_deregister",
        json!({"agent_id": "lifecycle-agent"}),
    );
    assert_eq!(dereg["success"], true);

    // After deregister, status should be inactive
    let get = handlers::dispatch(&ctx, "agent_get", json!({"agent_id": "lifecycle-agent"}));
    assert_eq!(get["status"], "inactive");
}

// ---------------------------------------------------------------------------
// Agent capabilities update
// ---------------------------------------------------------------------------

#[test]
fn test_agent_capabilities_update() {
    let ctx = test_ctx();
    handlers::dispatch(
        &ctx,
        "agent_register",
        json!({"agent_id": "cap-agent", "capabilities": ["search"]}),
    );
    let result = handlers::dispatch(
        &ctx,
        "agent_capabilities",
        json!({"agent_id": "cap-agent", "capabilities": ["search", "create", "analyze"]}),
    );
    let caps = result["capabilities"].as_array().unwrap();
    assert_eq!(caps.len(), 3);
}

// ---------------------------------------------------------------------------
// Tool definitions include agent tools
// ---------------------------------------------------------------------------

#[test]
fn test_agent_tools_in_definitions() {
    let tools = get_tool_definitions();
    let agent_tool_names = [
        "agent_register",
        "agent_deregister",
        "agent_heartbeat",
        "agent_list",
        "agent_get",
        "agent_capabilities",
    ];

    for name in &agent_tool_names {
        let found = tools.iter().any(|t| t.name == *name);
        assert!(found, "tool '{}' should be in TOOL_DEFINITIONS", name);
    }
}

// ---------------------------------------------------------------------------
// SSE event format (verify type serialization)
// ---------------------------------------------------------------------------

#[test]
fn test_sse_event_type_serialization() {
    // Verify RealtimeEvent can serialize to JSON (used by SSE handler)
    use engram::realtime::RealtimeEvent;

    let event = RealtimeEvent::memory_created(42, "test content".to_string());
    let serialized = serde_json::to_string(&event).expect("event should serialize");
    assert!(serialized.contains("memory_created"));
    assert!(serialized.contains("42"));
    assert!(serialized.contains("test content"));
}

// ---------------------------------------------------------------------------
// MCP dispatch benchmark compiles (smoke test)
// ---------------------------------------------------------------------------

#[test]
fn test_mcp_dispatch_unknown_tool_returns_error() {
    let ctx = test_ctx();
    let result = handlers::dispatch(&ctx, "nonexistent_tool", json!({}));
    assert!(result.get("error").is_some());
    assert!(result["error"].as_str().unwrap().contains("Unknown tool"));
}
