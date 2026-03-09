//! MCP tool handlers for emergent graph features.
//!
//! Exposes six tools for auto-linking and community detection:
//! - `memory_auto_link` — run semantic + temporal auto-linker
//! - `memory_list_auto_links` — list generated links
//! - `memory_auto_link_stats` — statistics about auto-links
//! - `memory_cluster` — run Louvain community detection
//! - `memory_get_cluster` — get the cluster for a memory
//! - `memory_list_clusters` — list all detected clusters
//!
//! All handlers are feature-gated with `#[cfg(feature = "emergent-graph")]`.

#[cfg(feature = "emergent-graph")]
use serde_json::{json, Value};

#[cfg(feature = "emergent-graph")]
use super::HandlerContext;

#[cfg(feature = "emergent-graph")]
use crate::storage::auto_linker::{
    auto_link_stats, list_auto_links, run_semantic_linker, run_temporal_linker,
    SemanticLinkOptions, TemporalLinkOptions,
};

#[cfg(feature = "emergent-graph")]
use crate::storage::clustering::{
    get_cluster, list_clusters, run_louvain_clustering, LouvainOptions,
};

// ── memory_auto_link ─────────────────────────────────────────────────────────

/// Run semantic + temporal auto-linker on a workspace.
///
/// Params:
/// - `workspace` (optional)
/// - `similarity_threshold` (optional, default `0.75`)
/// - `time_window_minutes` (optional, default `30`)
#[cfg(feature = "emergent-graph")]
pub fn memory_auto_link(ctx: &HandlerContext, params: Value) -> Value {
    let workspace: Option<String> = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let similarity_threshold = params
        .get("similarity_threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.75) as f32;
    let time_window_minutes = params
        .get("time_window_minutes")
        .and_then(|v| v.as_u64())
        .unwrap_or(30);

    let ws_display = workspace.clone().unwrap_or_else(|| "all".to_string());
    let embedder = ctx.embedder.clone();

    let semantic_opts = SemanticLinkOptions {
        threshold: similarity_threshold,
        workspace: workspace.clone(),
        ..Default::default()
    };
    let temporal_opts = TemporalLinkOptions {
        window_minutes: time_window_minutes,
        ..Default::default()
    };

    ctx.storage
        .with_transaction(move |conn| {
            let semantic = run_semantic_linker(conn, embedder.as_ref(), &semantic_opts)?;
            let temporal = run_temporal_linker(conn, &temporal_opts)?;
            Ok(json!({
                "workspace": ws_display,
                "semantic_links_added": semantic.links_created,
                "temporal_links_added": temporal.links_created,
                "total_links_added": semantic.links_created + temporal.links_created,
                "memories_processed": semantic.memories_processed + temporal.memories_processed,
                "duration_ms": semantic.duration_ms + temporal.duration_ms
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_list_auto_links ───────────────────────────────────────────────────

/// List auto-generated links.
///
/// Params:
/// - `link_type` (optional) — `"semantic"` or `"temporal"`
/// - `limit` (optional, default `50`)
#[cfg(feature = "emergent-graph")]
pub fn memory_list_auto_links(ctx: &HandlerContext, params: Value) -> Value {
    let link_type = params
        .get("link_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let lt_owned = link_type;

    ctx.storage
        .with_connection(|conn| {
            let links = list_auto_links(conn, lt_owned.as_deref(), limit)?;
            let links_json: Vec<Value> = links
                .iter()
                .map(|l| {
                    json!({
                        "from_id": l.from_id,
                        "to_id": l.to_id,
                        "link_type": l.link_type,
                        "score": l.score,
                        "created_at": l.created_at,
                    })
                })
                .collect();
            Ok(json!({
                "count": links_json.len(),
                "links": links_json
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_auto_link_stats ───────────────────────────────────────────────────

/// Get statistics about auto-links.
#[cfg(feature = "emergent-graph")]
pub fn memory_auto_link_stats(ctx: &HandlerContext, _params: Value) -> Value {
    ctx.storage
        .with_connection(|conn| auto_link_stats(conn))
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_cluster ───────────────────────────────────────────────────────────

/// Run Louvain community detection over the auto-link graph.
///
/// Params:
/// - `min_cluster_size` (optional, default `2`)
/// - `resolution` (optional, default `1.0`)
/// - `link_types` (optional array of strings)
#[cfg(feature = "emergent-graph")]
pub fn memory_cluster(ctx: &HandlerContext, params: Value) -> Value {
    let min_cluster_size = params
        .get("min_cluster_size")
        .and_then(|v| v.as_u64())
        .unwrap_or(2) as usize;
    let resolution = params
        .get("resolution")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let link_types: Option<Vec<String>> = params
        .get("link_types")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        });

    let options = LouvainOptions {
        min_cluster_size,
        resolution,
        link_types,
    };

    ctx.storage
        .with_transaction(|conn| {
            let result = run_louvain_clustering(conn, &options)?;
            Ok(json!({
                "clusters_found": result.clusters.len(),
                "modularity": result.modularity,
                "nodes": result.nodes,
                "clusters": result.clusters.iter().map(|c| json!({
                    "cluster_id": c.cluster_id,
                    "size": c.size,
                    "members": c.members,
                })).collect::<Vec<_>>()
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_get_cluster ───────────────────────────────────────────────────────

/// Get the cluster containing a specific memory.
///
/// Params:
/// - `memory_id` (required)
#[cfg(feature = "emergent-graph")]
pub fn memory_get_cluster(ctx: &HandlerContext, params: Value) -> Value {
    let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "memory_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| match get_cluster(conn, memory_id)? {
            Some(cluster) => Ok(json!({
                "found": true,
                "cluster_id": cluster.cluster_id,
                "size": cluster.size,
                "members": cluster.members,
            })),
            None => Ok(json!({
                "found": false,
                "memory_id": memory_id
            })),
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_list_clusters ─────────────────────────────────────────────────────

/// List all detected clusters.
///
/// Params:
/// - `algorithm` (optional, default `"louvain"`)
#[cfg(feature = "emergent-graph")]
pub fn memory_list_clusters(ctx: &HandlerContext, params: Value) -> Value {
    let algorithm = params
        .get("algorithm")
        .and_then(|v| v.as_str())
        .unwrap_or("louvain")
        .to_string();

    ctx.storage
        .with_connection(|conn| {
            let clusters = list_clusters(conn, &algorithm)?;
            Ok(json!({
                "algorithm": algorithm,
                "count": clusters.len(),
                "clusters": clusters.iter().map(|c| json!({
                    "cluster_id": c.cluster_id,
                    "size": c.size,
                    "members": c.members,
                })).collect::<Vec<_>>()
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[cfg(feature = "emergent-graph")]
mod tests {
    use parking_lot::Mutex;
    use serde_json::json;
    use std::sync::Arc;

    use crate::embedding::{EmbeddingCache, TfIdfEmbedder};
    use crate::mcp::handlers::HandlerContext;
    use crate::search::{AdaptiveCacheConfig, FuzzyEngine, SearchConfig, SearchResultCache};
    use crate::storage::Storage;

    fn make_ctx() -> HandlerContext {
        let storage = Storage::open_in_memory().expect("test storage");
        HandlerContext {
            storage,
            embedder: Arc::new(TfIdfEmbedder::new(128)),
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
                tokio::runtime::Runtime::new().expect("Failed to create langfuse runtime"),
            ),
        }
    }

    #[test]
    fn test_memory_auto_link_stats_empty() {
        let ctx = make_ctx();
        let result = super::memory_auto_link_stats(&ctx, json!({}));
        assert!(result.get("error").is_none(), "should not error on empty db: {result}");
    }

    #[test]
    fn test_memory_list_auto_links_empty() {
        let ctx = make_ctx();
        let result = super::memory_list_auto_links(&ctx, json!({}));
        assert!(result.get("error").is_none(), "unexpected error: {result}");
        assert_eq!(result["count"], json!(0));
    }

    #[test]
    fn test_memory_list_clusters_empty() {
        let ctx = make_ctx();
        let result = super::memory_list_clusters(&ctx, json!({}));
        assert!(result.get("error").is_none(), "unexpected error: {result}");
        assert_eq!(result["count"], json!(0));
        assert_eq!(result["algorithm"], json!("louvain"));
    }

    #[test]
    fn test_memory_cluster_no_links() {
        let ctx = make_ctx();
        let result = super::memory_cluster(&ctx, json!({"min_cluster_size": 2}));
        assert!(result.get("error").is_none(), "unexpected error: {result}");
        assert_eq!(result["clusters_found"], json!(0));
    }

    #[test]
    fn test_memory_get_cluster_requires_memory_id() {
        let ctx = make_ctx();
        let result = super::memory_get_cluster(&ctx, json!({}));
        assert!(result.get("error").is_some());
        assert!(
            result["error"].as_str().unwrap().contains("memory_id"),
            "error should mention missing memory_id"
        );
    }

    #[test]
    fn test_memory_get_cluster_not_found() {
        let ctx = make_ctx();
        let result = super::memory_get_cluster(&ctx, json!({"memory_id": 999}));
        assert!(result.get("error").is_none(), "unexpected error: {result}");
        assert_eq!(result["found"], json!(false));
    }
}
