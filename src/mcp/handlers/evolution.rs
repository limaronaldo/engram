//! Agentic evolution tool handlers.
//!
//! Provides MCP tools for memory update detection, utility scoring,
//! sentiment analysis, sentiment timelines, and reflective synthesis.

use serde_json::{json, Value};

use super::HandlerContext;

// ── memory_detect_updates ─────────────────────────────────────────────────────

/// Detect memories that may need updating given new content.
pub fn memory_detect_updates(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::memory_update::UpdateDetector;

    let content = match params.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json!({"error": "content is required"}),
    };
    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    let detector = UpdateDetector::new();

    ctx.storage
        .with_connection(|conn| {
            let candidates = detector.detect_updates(conn, &content, workspace)?;
            Ok(json!({
                "workspace": workspace,
                "candidates": candidates,
                "count": candidates.len(),
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_utility_score ──────────────────────────────────────────────────────

/// Compute the Q-value utility score for a memory from its feedback history.
pub fn memory_utility_score(ctx: &HandlerContext, params: Value) -> Value {
    use crate::search::utility::UtilityTracker;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    let tracker = UtilityTracker::new();

    ctx.storage
        .with_connection(|conn| {
            let score = tracker.get_utility(conn, id)?;
            Ok(json!({
                "memory_id": id,
                "utility_score": score.score,
                "retrievals": score.retrievals,
                "useful_count": score.useful_count,
                "last_retrieved": score.last_retrieved,
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_sentiment_analyze ──────────────────────────────────────────────────

/// Analyze sentiment of a memory's content.
pub fn memory_sentiment_analyze(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::emotional::SentimentAnalyzer;
    use crate::storage::queries::get_memory;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let memory = get_memory(conn, id)?;
            let analyzer = SentimentAnalyzer::new();
            let sentiment = analyzer.analyze(&memory.content);
            Ok(json!({
                "memory_id": id,
                "score": sentiment.score,
                "label": sentiment.label.as_str(),
                "confidence": sentiment.confidence,
                "keywords": sentiment.keywords,
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_sentiment_timeline ─────────────────────────────────────────────────

/// Compute a sentiment timeline over memories in a workspace and time range.
pub fn memory_sentiment_timeline(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::emotional::SentimentAnalyzer;

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let from = params
        .get("from")
        .and_then(|v| v.as_str())
        .unwrap_or("1970-01-01T00:00:00Z");
    let to = params
        .get("to")
        .and_then(|v| v.as_str())
        .unwrap_or("9999-12-31T23:59:59Z");
    let limit: i64 = params
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(50);

    let analyzer = SentimentAnalyzer::new();

    ctx.storage
        .with_connection(|conn| {
            // Fetch memories in the time range
            let mut stmt = conn.prepare(
                "SELECT id, content, created_at FROM memories
                 WHERE workspace = ?1
                   AND created_at >= ?2
                   AND created_at <= ?3
                 ORDER BY created_at ASC
                 LIMIT ?4",
            )?;

            let rows = stmt
                .query_map(
                    rusqlite::params![workspace, from, to, limit],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()
                .map_err(crate::error::EngramError::Database)?;

            let timeline: Vec<serde_json::Value> = rows
                .iter()
                .map(|(id, content, ts)| {
                    let s = analyzer.analyze(content);
                    json!({
                        "memory_id": id,
                        "timestamp": ts,
                        "score": s.score,
                        "label": s.label.as_str(),
                    })
                })
                .collect();

            Ok(json!({
                "workspace": workspace,
                "from": from,
                "to": to,
                "entries": timeline,
                "count": timeline.len(),
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_reflect ────────────────────────────────────────────────────────────

/// Generate a reflection over a set of memories.
pub fn memory_reflect(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::emotional::{ReflectionDepth, ReflectionEngine};
    use crate::storage::queries::get_memory;

    let ids: Vec<i64> = params
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    if ids.is_empty() {
        return json!({"error": "ids array is required and must not be empty"});
    }

    let depth_str = params
        .get("depth")
        .and_then(|v| v.as_str())
        .unwrap_or("surface");

    let depth = match depth_str {
        "analytical" => ReflectionDepth::Analytical,
        "meta" => ReflectionDepth::Meta,
        _ => ReflectionDepth::Surface,
    };

    ctx.storage
        .with_connection(|conn| {
            let mut pairs: Vec<(i64, String)> = Vec::new();
            for &id in &ids {
                if let Ok(m) = get_memory(conn, id) {
                    pairs.push((id, m.content));
                }
            }

            let memory_refs: Vec<(i64, &str)> =
                pairs.iter().map(|(id, c)| (*id, c.as_str())).collect();

            let engine = ReflectionEngine::new();
            let reflection = engine.create_reflection(conn, &memory_refs, depth)?;

            Ok(json!({
                "reflection": reflection.content,
                "source_ids": reflection.source_ids,
                "depth": reflection.depth.as_str(),
                "insights": reflection.insights,
                "created_at": reflection.created_at,
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
