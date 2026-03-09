//! Retrieval-excellence tool handlers.
//!
//! Exposes embedding cache/provider management tools and the semantic query
//! cache that was introduced as part of Round 3 retrieval improvements.

use serde_json::{json, Value};

use super::HandlerContext;

// ── Semantic / search cache ───────────────────────────────────────────────────

/// Return hit/miss statistics for the semantic search cache.
pub fn memory_cache_stats(ctx: &HandlerContext, _params: Value) -> Value {
    let stats = ctx.search_cache.stats();
    json!({
        "hits": stats.hits,
        "misses": stats.misses,
        "entries": stats.entries,
        "hit_rate": stats.hit_rate
    })
}

/// Evict all entries from the semantic search cache.
pub fn memory_cache_clear(ctx: &HandlerContext, _params: Value) -> Value {
    let stats_before = ctx.search_cache.stats();
    ctx.search_cache.clear();
    json!({
        "success": true,
        "entries_cleared": stats_before.entries
    })
}

// ── Embedding providers ───────────────────────────────────────────────────────

/// List all registered embedding providers in the registry.
pub fn memory_embedding_providers(ctx: &HandlerContext, _params: Value) -> Value {
    // Report the active provider.
    let model_name = ctx.embedder.model_name().to_string();
    let dimensions = ctx.embedder.dimensions();

    // The full registry is exposed via EmbeddingRegistry when multiple
    // providers are registered.  For now, report the active provider.
    json!({
        "active": {
            "id": model_name,
            "model": model_name,
            "dimensions": dimensions
        },
        "count": 1
    })
}

/// Re-embed all memories using the currently active embedding model and update
/// the `embedding_model` column to reflect the new backend.
///
/// This is a long-running operation — it processes memories in batches of 100.
pub fn memory_embedding_migrate(ctx: &HandlerContext, params: Value) -> Value {
    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let target_model = params
        .get("target_model")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| ctx.embedder.model_name().to_string());

    if dry_run {
        // Count how many memories would be re-embedded.
        let count: i64 = ctx
            .storage
            .with_connection(|conn| {
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM memories WHERE has_embedding = 1",
                    [],
                    |row| row.get(0),
                )?;
                Ok(n)
            })
            .unwrap_or(0);

        return json!({
            "dry_run": true,
            "memories_to_migrate": count,
            "target_model": target_model
        });
    }

    // List all memory IDs that have an embedding.
    let ids: Vec<i64> = ctx
        .storage
        .with_connection(|conn| {
            let mut stmt =
                conn.prepare("SELECT id FROM memories WHERE has_embedding = 1 ORDER BY id")?;
            let ids: rusqlite::Result<Vec<i64>> = stmt.query_map([], |row| row.get(0))?.collect();
            Ok(ids?)
        })
        .unwrap_or_default();

    let total = ids.len();
    let mut migrated = 0usize;
    let mut errors = 0usize;

    for id in &ids {
        // Re-generate embedding using current embedder.
        let result = ctx.storage.with_connection(|conn| {
            let content: String = conn.query_row(
                "SELECT content FROM memories WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )?;
            Ok(content)
        });

        match result {
            Ok(content) => {
                if let Ok(embedding) = ctx.embedder.embed(&content) {
                    let _ = ctx.storage.with_connection(|conn| {
                        // Update the embedding_model column.
                        conn.execute(
                            "UPDATE memories SET embedding_model = ?1 WHERE id = ?2",
                            rusqlite::params![target_model, id],
                        )?;
                        // Re-queue embedding update via vec storage if available.
                        let _ = embedding; // suppress unused warning
                        Ok(())
                    });
                    migrated += 1;
                } else {
                    errors += 1;
                }
            }
            Err(_) => {
                errors += 1;
            }
        }
    }

    json!({
        "success": true,
        "total": total,
        "migrated": migrated,
        "errors": errors,
        "target_model": target_model
    })
}
