//! Compression and consolidation tool handlers.
//!
//! Provides MCP tools for semantic compression, context-window packing,
//! offline consolidation, and synthesis overlap detection.

use serde_json::{json, Value};

use super::HandlerContext;

// ── memory_compress ───────────────────────────────────────────────────────────

/// Compress a memory's content using rule-based semantic compression.
pub fn memory_compress(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::compression_semantic::{CompressionConfig, SemanticCompressor};
    use crate::storage::queries::get_memory;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };
    let target_ratio: f32 = params
        .get("target_ratio")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(0.1);

    ctx.storage
        .with_connection(|conn| {
            let memory = get_memory(conn, id)?;
            let config = CompressionConfig {
                target_ratio,
                ..CompressionConfig::default()
            };
            let compressor = SemanticCompressor::new(config);
            let result = compressor.compress(&memory.content);
            Ok(json!({
                "memory_id": id,
                "original_tokens": result.original_tokens,
                "compressed_tokens": result.compressed_tokens,
                "compression_ratio": result.ratio,
                "structured_content": result.structured_content,
                "key_entities": result.key_entities,
                "key_facts": result.key_facts,
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_decompress ─────────────────────────────────────────────────────────

/// Retrieve the original (uncompressed) content of a memory.
pub fn memory_decompress(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::get_memory;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let memory = get_memory(conn, id)?;
            Ok(json!({
                "memory_id": id,
                "content": memory.content,
                "status": "ok",
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_compress_for_context ───────────────────────────────────────────────

/// Compress a set of memories to fit within a token budget for LLM context.
pub fn memory_compress_for_context(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::context_compression::{ContextCompressor, MemoryInput};
    use crate::storage::queries::get_memory;

    let ids: Vec<i64> = params
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    if ids.is_empty() {
        return json!({"error": "ids array is required and must not be empty"});
    }

    let token_budget: usize = params
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(4096);

    let result = ctx.storage.with_connection(|conn| {
        let mut inputs: Vec<MemoryInput> = Vec::new();
        for &id in &ids {
            match get_memory(conn, id) {
                Ok(m) => inputs.push(MemoryInput {
                    id: m.id,
                    content: m.content,
                    importance: m.importance,
                }),
                Err(_) => {} // skip missing memories
            }
        }

        let entries = ContextCompressor::compress_for_context(&inputs, token_budget);
        let total_tokens: usize = entries.iter().map(|e| e.tokens_used).sum();

        Ok(json!({
            "token_budget": token_budget,
            "memories_input": inputs.len(),
            "memories_included": entries.len(),
            "total_tokens": total_tokens,
            "entries": entries,
        }))
    });

    result.unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_consolidate ────────────────────────────────────────────────────────

/// Run offline consolidation over a workspace to merge similar memories.
pub fn memory_consolidate(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::consolidation_offline::{
        ConsolidationConfig, GroupingStrategy, OfflineConsolidator,
    };

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let strategy_str = params
        .get("strategy")
        .and_then(|v| v.as_str())
        .unwrap_or("content_overlap");
    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let strategy = match strategy_str {
        "tag_similarity" => GroupingStrategy::TagSimilarity,
        "temporal_proximity" => GroupingStrategy::TemporalProximity,
        _ => GroupingStrategy::ContentOverlap,
    };

    let result = ctx.storage.with_connection(|conn| {
        let config = ConsolidationConfig::default();
        let consolidator = OfflineConsolidator::new(config);
        let report = consolidator.consolidate_with_strategy(conn, workspace, strategy)?;
        Ok(json!({
            "workspace": workspace,
            "strategy": strategy_str,
            "dry_run": dry_run,
            "groups_found": report.groups_found,
            "memories_merged": report.memories_merged,
            "memories_archived": report.memories_archived,
            "tokens_before": report.tokens_before,
            "tokens_after": report.tokens_after,
            "tokens_saved": report.tokens_saved,
        }))
    });

    result.unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_synthesis ──────────────────────────────────────────────────────────

/// Check whether two pieces of content overlap semantically (Jaccard-based).
pub fn memory_synthesis(_ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::synthesis::{SynthesisConfig, SynthesisEngine, SynthesisStrategy};

    let content_a = match params.get("content_a").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json!({"error": "content_a is required"}),
    };
    let content_b = match params.get("content_b").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json!({"error": "content_b is required"}),
    };
    let id_a: i64 = params.get("id_a").and_then(|v| v.as_i64()).unwrap_or(0);
    let strategy_str = params
        .get("strategy")
        .and_then(|v| v.as_str())
        .unwrap_or("merge");

    let strategy = match strategy_str {
        "replace" => SynthesisStrategy::Replace,
        "append" => SynthesisStrategy::Append,
        _ => SynthesisStrategy::Merge,
    };

    let mut engine = SynthesisEngine::new(SynthesisConfig::default());
    engine.add_to_buffer(id_a, &content_a);

    match engine.check_and_synthesize(&content_b, strategy) {
        Some(synth) => json!({
            "overlap_detected": true,
            "overlap_score": synth.overlap_score,
            "strategy_used": format!("{:?}", synth.strategy_used),
            "synthesized_content": synth.content,
            "source_ids": synth.sources,
            "tokens_saved": synth.tokens_saved,
        }),
        None => json!({
            "overlap_detected": false,
            "message": "No significant overlap detected between the two contents",
        }),
    }
}
