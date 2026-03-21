//! Stats, versions, embedding cache, and compact-list handlers.

use serde_json::{json, Value};

use super::HandlerContext;

// ── Stats / Versions ──────────────────────────────────────────────────────────

pub fn memory_stats(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::queries::get_stats;

    ctx.storage
        .with_connection(|conn| {
            let stats = get_stats(conn)?;
            Ok(json!(stats))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_versions(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::get_memory_versions;

    let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);

    ctx.storage
        .with_connection(|conn| {
            let versions = get_memory_versions(conn, id)?;
            Ok(json!(versions))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── Embedding Cache ───────────────────────────────────────────────────────────

pub fn embedding_cache_stats(ctx: &HandlerContext, _params: Value) -> Value {
    let stats = ctx.embedding_cache.stats();
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

pub fn embedding_cache_clear(ctx: &HandlerContext, _params: Value) -> Value {
    let stats_before = ctx.embedding_cache.stats();
    ctx.embedding_cache.clear();
    let stats_after = ctx.embedding_cache.stats();
    json!({
        "success": true,
        "entries_cleared": stats_before.entries,
        "bytes_freed": stats_before.bytes_used,
        "bytes_freed_mb": stats_before.bytes_used as f64 / (1024.0 * 1024.0),
        "entries_after": stats_after.entries
    })
}

// ── Content Utilities ─────────────────────────────────────────────────────────

pub fn memory_soft_trim(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{soft_trim, SoftTrimConfig};
    use crate::storage::queries::get_memory;

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

    ctx.storage
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

pub fn memory_list_compact(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::list_memories_compact;
    use crate::types::ListOptions;

    let options: ListOptions = serde_json::from_value(params.clone()).unwrap_or_default();
    let preview_chars = params
        .get("preview_chars")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    ctx.storage
        .with_connection(|conn| {
            let memories = list_memories_compact(conn, &options, preview_chars)?;
            Ok(json!({
                "count": memories.len(),
                "memories": memories
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_content_stats(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::content_stats;
    use crate::storage::queries::get_memory;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    ctx.storage
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
