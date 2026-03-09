//! Agent registry MCP tool handlers.
//!
//! Provides 6 tools for managing registered AI agents:
//! register, deregister, heartbeat, list, get, and update capabilities.

use serde_json::{json, Value};

use super::HandlerContext;

pub fn agent_register(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::agent_registry::{register_agent, RegisterAgentInput};

    let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return json!({"error": "agent_id is required"}),
    };

    let display_name = params
        .get("display_name")
        .and_then(|v| v.as_str())
        .unwrap_or(&agent_id)
        .to_string();

    let capabilities: Vec<String> = params
        .get("capabilities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let namespaces: Vec<String> = params
        .get("namespaces")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_else(|| vec!["default".to_string()]);

    let metadata = params
        .get("metadata")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    let input = RegisterAgentInput {
        agent_id,
        display_name,
        capabilities,
        namespaces,
        metadata,
    };

    ctx.storage
        .with_connection(|conn| {
            let agent = register_agent(conn, &input)?;
            Ok(json!(agent))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn agent_deregister(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::agent_registry::deregister_agent;

    let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "agent_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let found = deregister_agent(conn, agent_id)?;
            Ok(json!({"success": found, "agent_id": agent_id}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn agent_heartbeat(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::agent_registry::heartbeat_agent;

    let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "agent_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let agent = heartbeat_agent(conn, agent_id)?;
            match agent {
                Some(a) => Ok(json!(a)),
                None => Ok(json!({"error": "agent not found", "agent_id": agent_id})),
            }
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn agent_list(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::agent_registry::list_agents;

    let status = params.get("status").and_then(|v| v.as_str());
    let namespace = params.get("namespace").and_then(|v| v.as_str());

    ctx.storage
        .with_connection(|conn| {
            // When namespace is provided, get agents in that namespace then
            // apply status filter client-side (storage query only returns active).
            if let Some(ns) = namespace {
                use crate::storage::agent_registry::get_agents_in_namespace;
                let mut agents = if status == Some("inactive") {
                    // get_agents_in_namespace hard-codes active, so for inactive
                    // we fetch all via list_agents and filter by namespace.
                    list_agents(conn, Some("inactive"))?
                        .into_iter()
                        .filter(|a| a.namespaces.iter().any(|n| n == ns))
                        .collect()
                } else {
                    get_agents_in_namespace(conn, ns)?
                };
                if let Some(s) = status {
                    agents.retain(|a| a.status == s);
                }
                Ok(json!({"agents": agents, "count": agents.len(), "namespace": ns}))
            } else {
                let agents = list_agents(conn, status)?;
                Ok(json!({"agents": agents, "count": agents.len()}))
            }
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn agent_get(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::agent_registry::get_agent;

    let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "agent_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let agent = get_agent(conn, agent_id)?;
            match agent {
                Some(a) => Ok(json!(a)),
                None => Ok(json!({"error": "agent not found", "agent_id": agent_id})),
            }
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn agent_capabilities(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::agent_registry::update_agent_capabilities;

    let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "agent_id is required"}),
    };

    let capabilities: Vec<String> = match params.get("capabilities").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        None => return json!({"error": "capabilities array is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let agent = update_agent_capabilities(conn, agent_id, &capabilities)?;
            match agent {
                Some(a) => Ok(json!(a)),
                None => Ok(json!({"error": "agent not found", "agent_id": agent_id})),
            }
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::handlers::HandlerContext;
    use crate::storage::Storage;
    use std::sync::Arc;

    fn test_ctx() -> HandlerContext {
        let storage = Storage::open_in_memory().expect("open in-memory storage");
        HandlerContext {
            storage,
            embedder: Arc::new(crate::embedding::TfIdfEmbedder::new(128)),
            fuzzy_engine: Arc::new(parking_lot::Mutex::new(crate::search::FuzzyEngine::new())),
            search_config: crate::search::SearchConfig::default(),
            realtime: None,
            embedding_cache: Arc::new(crate::embedding::EmbeddingCache::default()),
            search_cache: Arc::new(crate::search::SearchResultCache::new(
                crate::search::AdaptiveCacheConfig::default(),
            )),
            #[cfg(feature = "meilisearch")]
            meili: None,
            #[cfg(feature = "meilisearch")]
            meili_indexer: None,
            #[cfg(feature = "meilisearch")]
            meili_sync_interval: 300,
            #[cfg(feature = "langfuse")]
            langfuse_runtime: Arc::new(
                tokio::runtime::Builder::new_current_thread()
                    .build()
                    .unwrap(),
            ),
        }
    }

    #[test]
    fn test_register_and_get() {
        let ctx = test_ctx();
        let result = agent_register(
            &ctx,
            json!({"agent_id": "agent-1", "display_name": "Test Agent", "capabilities": ["search", "create"]}),
        );
        assert!(result.get("agent_id").is_some());
        assert_eq!(result["display_name"], "Test Agent");

        let get_result = agent_get(&ctx, json!({"agent_id": "agent-1"}));
        assert_eq!(get_result["display_name"], "Test Agent");
    }

    #[test]
    fn test_register_missing_id() {
        let ctx = test_ctx();
        let result = agent_register(&ctx, json!({}));
        assert!(result.get("error").is_some());
    }

    #[test]
    fn test_heartbeat() {
        let ctx = test_ctx();
        agent_register(&ctx, json!({"agent_id": "hb-agent"}));
        let result = agent_heartbeat(&ctx, json!({"agent_id": "hb-agent"}));
        assert!(result.get("last_heartbeat").is_some());
    }

    #[test]
    fn test_deregister() {
        let ctx = test_ctx();
        agent_register(&ctx, json!({"agent_id": "del-agent"}));
        let result = agent_deregister(&ctx, json!({"agent_id": "del-agent"}));
        assert_eq!(result["success"], true);
    }

    #[test]
    fn test_list_agents() {
        let ctx = test_ctx();
        agent_register(&ctx, json!({"agent_id": "list-1"}));
        agent_register(&ctx, json!({"agent_id": "list-2"}));
        let result = agent_list(&ctx, json!({}));
        assert_eq!(result["count"], 2);
    }

    #[test]
    fn test_list_by_namespace() {
        let ctx = test_ctx();
        agent_register(&ctx, json!({"agent_id": "ns-1", "namespaces": ["prod"]}));
        agent_register(&ctx, json!({"agent_id": "ns-2", "namespaces": ["staging"]}));
        let result = agent_list(&ctx, json!({"namespace": "prod"}));
        assert_eq!(result["count"], 1);
        assert_eq!(result["namespace"], "prod");
    }

    #[test]
    fn test_update_capabilities() {
        let ctx = test_ctx();
        agent_register(
            &ctx,
            json!({"agent_id": "cap-agent", "capabilities": ["search"]}),
        );
        let result = agent_capabilities(
            &ctx,
            json!({"agent_id": "cap-agent", "capabilities": ["search", "create", "delete"]}),
        );
        let caps = result["capabilities"].as_array().unwrap();
        assert_eq!(caps.len(), 3);
    }

    #[test]
    fn test_get_nonexistent() {
        let ctx = test_ctx();
        let result = agent_get(&ctx, json!({"agent_id": "nope"}));
        assert!(result.get("error").is_some());
    }
}
