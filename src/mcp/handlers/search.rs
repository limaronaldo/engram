//! Search tool handlers.

use serde_json::{json, Value};

use crate::search::{hybrid_search, RerankConfig, RerankStrategy, Reranker};
use crate::types::*;

use super::HandlerContext;

pub fn memory_search(ctx: &HandlerContext, params: Value) -> Value {
    use crate::search::result_cache::CacheFilterParams;

    let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let options: SearchOptions = serde_json::from_value(params.clone()).unwrap_or_default();

    let rerank_enabled = params
        .get("rerank")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let rerank_strategy = match params.get("rerank_strategy").and_then(|v| v.as_str()) {
        Some("none") => RerankStrategy::None,
        Some("multi_signal") => RerankStrategy::MultiSignal,
        _ => RerankStrategy::Heuristic,
    };

    let query_embedding = ctx.embedder.embed(query).ok();
    let embedding_ref = query_embedding.as_deref();

    let cache_filters = CacheFilterParams {
        workspace: options.workspace.clone(),
        tier: options.tier.map(|t| t.as_str().to_string()),
        memory_types: options.memory_type.map(|t| vec![t]),
        include_archived: options.include_archived,
        include_transcripts: options.include_transcripts,
        tags: options.tags.clone(),
    };

    let skip_cache = params
        .get("skip_cache")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !skip_cache && !rerank_enabled {
        if let Some(cached_results) = ctx.search_cache.get(query, embedding_ref, &cache_filters) {
            return json!({"results": cached_results, "cached": true});
        }
    }

    let mut search_config = ctx.search_config.clone();
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(canonical) = cwd.canonicalize() {
            search_config.project_context_path = Some(canonical.to_string_lossy().to_string());
        }
    }

    ctx.storage
        .with_connection(|conn| {
            let results = hybrid_search(conn, query, embedding_ref, &options, &search_config)?;

            if !rerank_enabled && !skip_cache {
                ctx.search_cache.put(
                    query,
                    query_embedding.clone(),
                    cache_filters.clone(),
                    results.clone(),
                );
            }

            if rerank_enabled && rerank_strategy != RerankStrategy::None {
                let config = RerankConfig {
                    enabled: true,
                    strategy: rerank_strategy,
                    ..Default::default()
                };
                let reranker = Reranker::with_config(config);
                let reranked = reranker.rerank(results, query, None);

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

pub fn search_suggest(ctx: &HandlerContext, params: Value) -> Value {
    let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let fuzzy = ctx.fuzzy_engine.lock();
    let result = fuzzy.correct_query(query);
    json!(result)
}

pub fn memory_search_by_identity(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::search_by_identity;

    let identity = match params.get("identity").and_then(|v| v.as_str()) {
        Some(i) => i,
        None => return json!({"error": "identity is required"}),
    };

    let workspace = params.get("workspace").and_then(|v| v.as_str());
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    ctx.storage
        .with_connection(|conn| {
            let memories = search_by_identity(conn, identity, workspace, limit)?;
            Ok(json!({"memories": memories}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_session_search(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::search_sessions;

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

    ctx.storage
        .with_connection(|conn| {
            let memories = search_sessions(conn, query, session_id, workspace, limit)?;
            Ok(json!({"memories": memories}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn find_duplicates(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::find_duplicates;

    let threshold = params
        .get("threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.9);

    ctx.storage
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

pub fn find_semantic_duplicates(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::find_duplicates_by_embedding;

    let threshold = params
        .get("threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.92) as f32;
    let workspace = params.get("workspace").and_then(|v| v.as_str());
    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(50) as usize;

    ctx.storage
        .with_connection(|conn| {
            let duplicates = find_duplicates_by_embedding(conn, threshold, workspace, limit)?;
            Ok(json!({
                "count": duplicates.len(),
                "threshold": threshold,
                "method": "embedding_cosine_similarity",
                "duplicates": duplicates
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn search_cache_feedback(ctx: &HandlerContext, params: Value) -> Value {
    use crate::search::CacheFilterParams;

    let query = match params.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return json!({"error": "query is required"}),
    };

    let positive = match params.get("positive").and_then(|v| v.as_bool()) {
        Some(p) => p,
        None => return json!({"error": "positive is required"}),
    };

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let filters = CacheFilterParams {
        workspace,
        ..Default::default()
    };

    ctx.search_cache.record_feedback(query, &filters, positive);
    let new_threshold = ctx.search_cache.current_threshold();

    json!({
        "recorded": true,
        "feedback": if positive { "positive" } else { "negative" },
        "current_threshold": new_threshold
    })
}

pub fn search_cache_stats(ctx: &HandlerContext, _params: Value) -> Value {
    let stats = ctx.search_cache.stats();
    json!(stats)
}

pub fn search_cache_clear(ctx: &HandlerContext, params: Value) -> Value {
    let workspace = params.get("workspace").and_then(|v| v.as_str());

    if let Some(ws) = workspace {
        ctx.search_cache.invalidate_for_workspace(Some(ws));
        json!({"cleared": true, "scope": "workspace", "workspace": ws})
    } else {
        ctx.search_cache.clear();
        json!({"cleared": true, "scope": "all"})
    }
}

// ── Search Explainability (RML-1242) ────────────────────────────────────────

pub fn memory_explain_search(_ctx: &HandlerContext, params: Value) -> Value {
    use crate::search::explain::SearchExplainer;

    let results = match params.get("results").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            return json!({"error": "results array is required (each with memory_id, bm25, vector, fuzzy, recency, importance, final_score, and optional rerank_score)"})
        }
    };

    let reranking_active = params
        .get("reranking_active")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let rrf_k = params.get("rrf_k").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32;

    let explainer = SearchExplainer::new(rrf_k, reranking_active);

    let batch: Vec<_> = results
        .iter()
        .filter_map(|r| {
            let memory_id = r.get("memory_id")?.as_i64()?;
            let bm25 = r.get("bm25").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let vector = r.get("vector").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let fuzzy = r.get("fuzzy").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let recency = r.get("recency").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let importance = r.get("importance").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let rerank = r
                .get("rerank_score")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32);
            let final_score = r.get("final_score").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            Some((
                memory_id,
                bm25,
                vector,
                fuzzy,
                recency,
                importance,
                rerank,
                final_score,
            ))
        })
        .collect();

    let explanations = explainer.explain_batch(batch);
    json!({
        "count": explanations.len(),
        "explanations": explanations
    })
}

// ── Relevance Feedback (RML-1243) ───────────────────────────────────────────

pub fn memory_feedback(ctx: &HandlerContext, params: Value) -> Value {
    use crate::search::feedback::{record_feedback, FeedbackSignal};

    let query = match params.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return json!({"error": "query is required"}),
    };

    let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "memory_id is required"}),
    };

    let signal = match params.get("signal").and_then(|v| v.as_str()) {
        Some("useful") => FeedbackSignal::Useful,
        Some("irrelevant") => FeedbackSignal::Irrelevant,
        _ => return json!({"error": "signal must be 'useful' or 'irrelevant'"}),
    };

    let rank_position = params
        .get("rank_position")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let original_score = params
        .get("original_score")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32);
    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    ctx.storage
        .with_connection(|conn| {
            let fb = record_feedback(
                conn,
                query,
                memory_id,
                signal,
                rank_position,
                original_score,
                workspace,
            )?;
            Ok(json!(fb))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_feedback_stats(ctx: &HandlerContext, params: Value) -> Value {
    use crate::search::feedback::feedback_stats;

    let workspace = params.get("workspace").and_then(|v| v.as_str());

    ctx.storage
        .with_connection(|conn| {
            let stats = feedback_stats(conn, workspace)?;
            Ok(json!(stats))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
