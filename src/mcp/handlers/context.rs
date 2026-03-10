//! Context-engineering and memory-block tool handlers (Round 3 — T8/T9/T10).
//!
//! Covers:
//! - Fact extraction from memory content (SPO triples)
//! - Fact retrieval and subject graphs
//! - Prompt-context assembly via ContextBuilder
//! - Self-editing memory blocks (Letta/MemGPT-style)

use serde_json::{json, Value};

use super::HandlerContext;

// ── Fact extraction ───────────────────────────────────────────────────────────

/// Extract SPO facts from a memory's content and persist them.
///
/// Params:
/// - `memory_id` (i64, required) — source memory to extract from
pub fn memory_extract_facts(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::fact_extraction::{
        create_fact, ConversationProcessor, RuleBasedExtractor,
    };

    let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "memory_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            // Fetch the memory content.
            let content: Option<String> = conn
                .query_row(
                    "SELECT content FROM memories WHERE id = ?1",
                    rusqlite::params![memory_id],
                    |row| row.get(0),
                )
                .ok();

            let content = match content {
                Some(c) => c,
                None => {
                    return Ok(json!({"error": format!("memory {} not found", memory_id)}));
                }
            };

            // Extract facts.
            let processor = ConversationProcessor::new(Box::new(RuleBasedExtractor::new()));
            let extracted = processor.process_text(&content, Some(memory_id));

            // Persist each fact.
            let mut stored = Vec::new();
            for fact in &extracted {
                if let Ok(f) = create_fact(conn, fact, Some(memory_id)) {
                    stored.push(json!({
                        "id": f.id,
                        "subject": f.subject,
                        "predicate": f.predicate,
                        "object": f.object,
                        "confidence": f.confidence
                    }));
                }
            }

            Ok(json!({
                "memory_id": memory_id,
                "facts_extracted": extracted.len(),
                "facts_stored": stored.len(),
                "facts": stored
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// List facts, optionally filtered by source memory.
///
/// Params:
/// - `memory_id` (i64, optional) — filter to facts from this memory
/// - `limit` (u64, optional) — max rows to return (0 = unlimited)
pub fn memory_list_facts(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::fact_extraction::list_facts;

    let source_id = params.get("memory_id").and_then(|v| v.as_i64());
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;

    ctx.storage
        .with_connection(|conn| {
            let facts = list_facts(conn, source_id, limit)?;
            let items: Vec<Value> = facts
                .iter()
                .map(|f| {
                    json!({
                        "id": f.id,
                        "subject": f.subject,
                        "predicate": f.predicate,
                        "object": f.object,
                        "confidence": f.confidence,
                        "source_memory_id": f.source_memory_id,
                        "created_at": f.created_at
                    })
                })
                .collect();
            Ok(json!({"facts": items, "count": items.len()}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Return all facts for a given subject.
///
/// Params:
/// - `subject` (string, required) — the entity to look up
pub fn memory_fact_graph(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::fact_extraction::get_fact_graph;

    let subject = match params.get("subject").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "subject is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let facts = get_fact_graph(conn, &subject)?;
            let items: Vec<Value> = facts
                .iter()
                .map(|f| {
                    json!({
                        "id": f.id,
                        "subject": f.subject,
                        "predicate": f.predicate,
                        "object": f.object,
                        "confidence": f.confidence,
                        "source_memory_id": f.source_memory_id
                    })
                })
                .collect();
            Ok(json!({"subject": subject, "facts": items, "count": items.len()}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Build a structured prompt context from memories.
///
/// Params:
/// - `query` (string, required) — search query to retrieve relevant memories
/// - `total_budget` (u64, optional) — max tokens for the entire prompt (default: 4096)
/// - `strategy` (string, optional) — "greedy" | "balanced" | "recency" (default: "greedy")
/// - `workspace` (string, optional) — workspace to search in
/// - `limit` (u64, optional) — max memories to retrieve (default: 20)
pub fn memory_build_context(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::context_builder::{
        ContextBuilder, MemoryEntry, PromptTemplate, Section, SimpleTokenCounter, Strategy,
    };
    use crate::search::hybrid_search;
    use crate::types::SearchOptions;

    let query = match params.get("query").and_then(|v| v.as_str()) {
        Some(q) => q.to_string(),
        None => return json!({"error": "query is required"}),
    };

    let total_budget = params
        .get("total_budget")
        .and_then(|v| v.as_u64())
        .unwrap_or(4096) as usize;

    let strategy = match params.get("strategy").and_then(|v| v.as_str()) {
        Some("balanced") => Strategy::Balanced,
        Some("recency") => Strategy::Recency,
        _ => Strategy::Greedy,
    };

    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

    let search_opts = SearchOptions {
        workspace: params
            .get("workspace")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        limit: Some(limit as i64),
        ..Default::default()
    };

    let query_embedding = ctx.embedder.embed(&query).ok();
    let embedding_ref = query_embedding.as_deref();

    let search_result = ctx.storage.with_connection(|conn| {
        hybrid_search(
            conn,
            &query,
            embedding_ref,
            &search_opts,
            &ctx.search_config,
        )
    });

    let memories = match search_result {
        Ok(results) => results,
        Err(e) => return json!({"error": e.to_string()}),
    };

    // Convert to MemoryEntry items.
    let entries: Vec<MemoryEntry> = memories
        .iter()
        .map(|r| MemoryEntry::new(r.memory.content.clone(), r.memory.created_at))
        .collect();

    let template = PromptTemplate {
        sections: vec![Section {
            name: "Memories".to_string(),
            content: String::new(),
            max_tokens: total_budget,
            priority: 0,
        }],
        total_budget,
        separator: "\n\n---\n\n".to_string(),
    };

    let builder = ContextBuilder::new(Box::new(SimpleTokenCounter));
    let prompt = builder.build(&template, &entries, strategy);
    let token_estimate = builder.estimate_tokens(&prompt);

    json!({
        "prompt": prompt,
        "token_estimate": token_estimate,
        "memories_used": entries.len(),
        "total_budget": total_budget
    })
}

// ── Memory blocks ─────────────────────────────────────────────────────────────

/// Get a memory block by name.
///
/// Params:
/// - `name` (string, required)
pub fn memory_block_get(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::memory_blocks::get_block;

    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return json!({"error": "name is required"}),
    };

    ctx.storage
        .with_connection(|conn| match get_block(conn, &name)? {
            Some(block) => Ok(json!(block)),
            None => Ok(json!({"error": format!("block '{}' not found", name)})),
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Edit (update) a memory block's content.
///
/// Params:
/// - `name` (string, required)
/// - `content` (string, required)
/// - `reason` (string, optional) — description of the edit
pub fn memory_block_edit(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::memory_blocks::update_block;

    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return json!({"error": "name is required"}),
    };

    let content = match params.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json!({"error": "content is required"}),
    };

    let reason = params
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    ctx.storage
        .with_connection(|conn| {
            let block = update_block(conn, &name, &content, &reason)?;
            Ok(json!(block))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// List all memory blocks.
pub fn memory_block_list(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::memory_blocks::list_blocks;

    ctx.storage
        .with_connection(|conn| {
            let blocks = list_blocks(conn)?;
            Ok(json!({"blocks": blocks, "count": blocks.len()}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Create a new memory block.
///
/// Params:
/// - `name` (string, required)
/// - `content` (string, optional, default: "")
/// - `max_tokens` (u64, optional, default: 4096)
pub fn memory_block_create(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::memory_blocks::create_block;

    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return json!({"error": "name is required"}),
    };

    let content = params
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let max_tokens = params
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(4096) as usize;

    ctx.storage
        .with_connection(|conn| {
            let block = create_block(conn, &name, &content, max_tokens)?;
            Ok(json!(block))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Archive a memory block (delete it and return the final content).
///
/// Params:
/// - `name` (string, required)
pub fn memory_block_archive(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::memory_blocks::{delete_block, get_block};

    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return json!({"error": "name is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let block = get_block(conn, &name)?;
            let final_content = block
                .as_ref()
                .map(|b| b.content.clone())
                .unwrap_or_default();
            let final_version = block.as_ref().map(|b| b.version).unwrap_or(0);

            if block.is_none() {
                return Ok(json!({"error": format!("block '{}' not found", name)}));
            }

            delete_block(conn, &name)?;

            Ok(json!({
                "success": true,
                "name": name,
                "final_content": final_content,
                "final_version": final_version
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── Injection prompt ──────────────────────────────────────────────────────────

/// Build a ready-to-inject prompt string from memories relevant to a query.
///
/// Params:
/// - `query` (string, required) — search query to retrieve relevant memories
/// - `token_budget` (u64, optional, default: 2000) — maximum tokens for the output prompt
/// - `workspace` (string, optional) — workspace to search in
/// - `include_types` (array of string, optional) — filter by memory type (e.g. ["note","episodic"])
pub fn memory_get_injection_prompt(ctx: &HandlerContext, params: Value) -> Value {
    use crate::search::hybrid_search;
    use crate::types::SearchOptions;

    let query = match params.get("query").and_then(|v| v.as_str()) {
        Some(q) => q.to_string(),
        None => return json!({"error": "query is required"}),
    };

    let token_budget = params
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .unwrap_or(2000) as usize;

    let include_types: Vec<String> = params
        .get("include_types")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let search_opts = SearchOptions {
        workspace: params
            .get("workspace")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        limit: Some(20),
        ..Default::default()
    };

    let query_embedding = ctx.embedder.embed(&query).ok();
    let embedding_ref = query_embedding.as_deref();

    let search_result = ctx.storage.with_connection(|conn| {
        hybrid_search(
            conn,
            &query,
            embedding_ref,
            &search_opts,
            &ctx.search_config,
        )
    });

    let memories = match search_result {
        Ok(results) => results,
        Err(e) => return json!({"error": e.to_string()}),
    };

    // Filter by memory type if include_types is specified.
    let memories: Vec<_> = if include_types.is_empty() {
        memories
    } else {
        memories
            .into_iter()
            .filter(|r| include_types.contains(&r.memory.memory_type.as_str().to_string()))
            .collect()
    };

    if memories.is_empty() {
        return json!({
            "prompt": "# Relevant Context\n\n*(No memories found)*",
            "memory_count": 0,
            "tokens_used": 0
        });
    }

    // Build per-memory markdown blocks.
    let blocks: Vec<String> = memories
        .iter()
        .map(|r| {
            let m = &r.memory;
            let tags_str = m.tags.join(", ");
            format!(
                "## [{}] Memory #{}\nCreated: {} | Tags: {}\n\n{}\n\n---",
                m.memory_type.as_str(),
                m.id,
                m.created_at.to_rfc3339(),
                tags_str,
                m.content
            )
        })
        .collect();

    // Estimate tokens for the full prompt.
    let header = "# Relevant Context\n\n";
    let joined = blocks.join("\n\n");
    let full_prompt = format!("{}{}", header, joined);
    let total_chars = full_prompt.len();
    let estimated_tokens = total_chars / 4;

    if estimated_tokens <= token_budget {
        return json!({
            "prompt": full_prompt,
            "memory_count": memories.len(),
            "tokens_used": estimated_tokens
        });
    }

    // Budget exceeded — proportionally truncate each memory's content.
    // tokens_per_memory = token_budget / count  → chars_per_content = that * 4 - overhead_chars
    let count = memories.len();
    let budget_chars = token_budget * 4;
    // Reserve chars for header + separators + per-block overhead (type, id, created_at, tags lines)
    let header_chars = header.len();
    let separator_chars = "\n\n".len() * (count.saturating_sub(1));
    let overhead_per_block = 80usize; // conservative estimate for the header line of each block
    let total_overhead = header_chars + separator_chars + overhead_per_block * count;
    let available_content_chars = budget_chars.saturating_sub(total_overhead);
    let chars_per_content = if count > 0 {
        available_content_chars / count
    } else {
        0
    };

    let truncated_blocks: Vec<String> = memories
        .iter()
        .map(|r| {
            let m = &r.memory;
            let tags_str = m.tags.join(", ");
            let content = if m.content.len() > chars_per_content && chars_per_content > 0 {
                format!("{}…", &m.content[..chars_per_content])
            } else {
                m.content.clone()
            };
            format!(
                "## [{}] Memory #{}\nCreated: {} | Tags: {}\n\n{}\n\n---",
                m.memory_type.as_str(),
                m.id,
                m.created_at.to_rfc3339(),
                tags_str,
                content
            )
        })
        .collect();

    let final_prompt = format!("{}{}", header, truncated_blocks.join("\n\n"));
    let tokens_used = final_prompt.len() / 4;

    json!({
        "prompt": final_prompt,
        "memory_count": count,
        "tokens_used": tokens_used
    })
}

// ── Tool-use observation ───────────────────────────────────────────────────────

/// Observe and record a tool invocation as an Episodic memory.
///
/// Params:
/// - `tool_name` (string, required) — name of the tool that was called
/// - `tool_input` (any JSON value, required) — the input passed to the tool
/// - `tool_output` (string, required) — the output returned by the tool
/// - `session_id` (string, optional, default: "unknown") — session identifier for grouping
/// - `compress` (bool, optional, default: true) — compact vs full JSON storage
pub fn memory_observe_tool_use(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::create_memory;
    use crate::types::{CreateMemoryInput, MemoryType};

    let tool_name = match params.get("tool_name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return json!({"error": "tool_name is required"}),
    };

    let tool_input = match params.get("tool_input") {
        Some(v) => v.clone(),
        None => return json!({"error": "tool_input is required"}),
    };

    let tool_output = match params.get("tool_output").and_then(|v| v.as_str()) {
        Some(o) => o.to_string(),
        None => return json!({"error": "tool_output is required"}),
    };

    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let compress = params
        .get("compress")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let content = if compress {
        let input_str = serde_json::to_string(&tool_input).unwrap_or_default();
        let input_preview = if input_str.len() > 200 {
            format!("{}…", &input_str[..200])
        } else {
            input_str
        };
        let output_preview = if tool_output.len() > 200 {
            format!("{}…", &tool_output[..200])
        } else {
            tool_output.clone()
        };
        format!(
            "[{}] input→{} output→{}",
            tool_name, input_preview, output_preview
        )
    } else {
        serde_json::to_string(&json!({
            "tool_name": tool_name,
            "input": tool_input,
            "output": tool_output
        }))
        .unwrap_or_else(|_| format!("[{}] observation", tool_name))
    };

    let tags = vec![
        "tool-observation".to_string(),
        format!("session:{}", session_id),
        tool_name.clone(),
    ];

    let input = CreateMemoryInput {
        content,
        memory_type: MemoryType::Episodic,
        tags,
        workspace: Some("default".to_string()),
        ..Default::default()
    };

    let result = ctx
        .storage
        .with_transaction(|conn| create_memory(conn, &input));

    match result {
        Ok(memory) => json!({
            "id": memory.id,
            "compressed": compress
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

/// Get the edit history for a memory block.
///
/// Params:
/// - `name` (string, required)
/// - `limit` (u64, optional, default: 20)
pub fn memory_block_history(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::memory_blocks::get_block_history;

    let name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return json!({"error": "name is required"}),
    };

    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

    ctx.storage
        .with_connection(|conn| {
            let history = get_block_history(conn, &name, limit)?;
            Ok(json!({"name": name, "history": history, "count": history.len()}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
