//! Context-engineering and memory-block tool handlers (Round 3 — T8/T9/T10).
//!
//! Covers:
//! - Fact extraction from memory content (SPO triples)
//! - Fact retrieval and subject graphs
//! - Prompt-context assembly via ContextBuilder
//! - Self-editing memory blocks (Letta/MemGPT-style)

use serde_json::{json, Value};

use super::HandlerContext;

// ── Utilities ─────────────────────────────────────────────────────────────────

/// Truncate `s` to at most `max_bytes` bytes, always landing on a valid UTF-8
/// char boundary. Avoids panics on multibyte (emoji, CJK, accented) input.
fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &s[..boundary]
}

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
                format!("{}…", safe_truncate(&m.content, chars_per_content))
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
            format!("{}…", safe_truncate(&input_str, 200))
        } else {
            input_str
        };
        let output_preview = if tool_output.len() > 200 {
            format!("{}…", safe_truncate(&tool_output, 200))
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

// ── Endless Mode: tool output archival ───────────────────────────────────────

/// Archive a tool's full raw output as an Episodic memory and return a compact
/// summary, solving the O(N²) context window growth problem.
///
/// Params:
/// - `tool_name` (string, required) — name of the tool whose output is being archived
/// - `raw_output` (string, required) — full raw output string
/// - `session_id` (string, optional, default: "unknown") — session identifier
/// - `compress_summary` (bool, optional, default: true) — whether to generate a summary
/// - `summary_tokens` (usize, optional, default: 500) — max tokens for the summary
pub fn memory_archive_tool_output(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::create_memory;
    use crate::types::{CreateMemoryInput, MemoryType};

    let tool_name = match params.get("tool_name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return json!({"error": "tool_name is required"}),
    };

    let raw_output = match params.get("raw_output").and_then(|v| v.as_str()) {
        Some(o) => o.to_string(),
        None => return json!({"error": "raw_output is required"}),
    };

    let session_id = params
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let compress_summary = params
        .get("compress_summary")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let summary_tokens = params
        .get("summary_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(500) as usize;

    // Step 1: Store the full raw output as an Episodic memory in workspace "archive".
    let tags = vec![
        "tool-archive".to_string(),
        format!("session:{}", session_id),
        tool_name.clone(),
    ];

    let input = CreateMemoryInput {
        content: raw_output.clone(),
        memory_type: MemoryType::Episodic,
        tags,
        workspace: Some("archive".to_string()),
        ..Default::default()
    };

    let archive_memory = match ctx.storage.with_transaction(|conn| create_memory(conn, &input)) {
        Ok(m) => m,
        Err(e) => return json!({"error": e.to_string()}),
    };

    let archive_id = archive_memory.id;

    // Step 2: Build summary.
    let summary = if compress_summary {
        let max_chars = summary_tokens * 4;
        let slice = safe_truncate(&raw_output, max_chars);

        // Find last sentence boundary within the slice.
        let boundary = slice
            .rfind(['.', '!', '?', '\n'])
            .map(|pos| pos + 1)
            .unwrap_or(slice.len());

        let trimmed = slice[..boundary].trim_end();
        format!("[{} summary] {}", tool_name, trimmed)
    } else {
        raw_output.clone()
    };

    // Step 3: Compute token estimates.
    let raw_tokens_estimate = raw_output.len() / 4;
    let summary_tokens_estimate = summary.len() / 4;
    let compression_ratio = summary_tokens_estimate as f64
        / (raw_tokens_estimate.max(1)) as f64;

    json!({
        "archive_id": archive_id,
        "summary": summary,
        "raw_tokens_estimate": raw_tokens_estimate,
        "summary_tokens_estimate": summary_tokens_estimate,
        "compression_ratio": compression_ratio
    })
}

/// Retrieve the full raw output for a previously archived tool output.
///
/// Params:
/// - `archive_id` (i64, required) — ID returned by `memory_archive_tool_output`
pub fn memory_get_archived_output(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::get_memory;

    let archive_id = match params.get("archive_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "archive_id is required"}),
    };

    let memory = match ctx.storage.with_connection(|conn| {
        match get_memory(conn, archive_id) {
            Ok(m) => Ok(Some(m)),
            Err(crate::error::EngramError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }) {
        Ok(Some(m)) => m,
        Ok(None) => return json!({"error": "Archive not found", "archive_id": archive_id}),
        Err(e) => return json!({"error": e.to_string()}),
    };

    // Extract tool_name from tags: the first tag that isn't "tool-archive" or starts with "session:".
    let tool_name = memory
        .tags
        .iter()
        .find(|t| *t != "tool-archive" && !t.starts_with("session:"))
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());

    json!({
        "archive_id": archive_id,
        "tool_name": tool_name,
        "content": memory.content,
        "created_at": memory.created_at.to_rfc3339()
    })
}

/// Assemble a structured working-memory markdown block for the current session.
///
/// Combines compact tool-observations with references to archived full outputs,
/// keeping context growth O(1) per tool call instead of O(N).
///
/// Params:
/// - `session_id` (string, required)
/// - `token_budget` (usize, optional, default: 4000)
/// - `include_tool_names` (array of string, optional) — whitelist of tool names to include
/// - `since_minutes` (u64, optional) — only include observations from the last N minutes
pub fn memory_get_working_memory(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::list_memories;
    use crate::types::{ListOptions, SortField, SortOrder};
    use chrono::{Duration, Utc};

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "session_id is required"}),
    };

    let token_budget = params
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .unwrap_or(4000) as usize;

    let include_tool_names: Vec<String> = params
        .get("include_tool_names")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    // Optional recency filter: only observations created within the last N minutes.
    let since_cutoff = params
        .get("since_minutes")
        .and_then(|v| v.as_u64())
        .map(|mins| Utc::now() - Duration::minutes(mins as i64));

    let session_tag = format!("session:{}", session_id);

    // Helper: check whether a memory's tags contain the session tag.
    let has_session_tag = |tags: &[String]| tags.contains(&session_tag);

    // Helper: extract the tool name from a memory's tags.
    let extract_tool_name = |tags: &[String], exclude_prefix: &str| -> String {
        tags.iter()
            .find(|t| *t != exclude_prefix && !t.starts_with("session:"))
            .cloned()
            .unwrap_or_else(|| "unknown".to_string())
    };

    // Helper: check include_tool_names filter.
    let passes_tool_filter = |tags: &[String]| -> bool {
        if include_tool_names.is_empty() {
            return true;
        }
        tags.iter().any(|t| include_tool_names.contains(t))
    };

    // Fetch tool observations from workspace "default".
    let obs_options = ListOptions {
        workspace: Some("default".to_string()),
        tags: Some(vec!["tool-observation".to_string()]),
        sort_by: Some(SortField::CreatedAt),
        sort_order: Some(SortOrder::Asc),
        limit: Some(1000),
        ..Default::default()
    };

    let all_observations = match ctx
        .storage
        .with_connection(|conn| list_memories(conn, &obs_options))
    {
        Ok(mems) => mems,
        Err(e) => return json!({"error": e.to_string()}),
    };

    // Filter observations by session tag, optional tool name whitelist, and recency.
    let observations: Vec<_> = all_observations
        .into_iter()
        .filter(|m| {
            has_session_tag(&m.tags)
                && passes_tool_filter(&m.tags)
                && since_cutoff.is_none_or(|cutoff| m.created_at >= cutoff)
        })
        .collect();

    // Fetch archive entries from workspace "archive".
    let archive_options = ListOptions {
        workspace: Some("archive".to_string()),
        tags: Some(vec!["tool-archive".to_string()]),
        sort_by: Some(SortField::CreatedAt),
        sort_order: Some(SortOrder::Asc),
        limit: Some(1000),
        ..Default::default()
    };

    let all_archives = match ctx
        .storage
        .with_connection(|conn| list_memories(conn, &archive_options))
    {
        Ok(mems) => mems,
        Err(e) => return json!({"error": e.to_string()}),
    };

    // Filter archive entries by session tag, optional tool name whitelist, and recency.
    let archives: Vec<_> = all_archives
        .into_iter()
        .filter(|m| {
            has_session_tag(&m.tags)
                && passes_tool_filter(&m.tags)
                && since_cutoff.is_none_or(|cutoff| m.created_at >= cutoff)
        })
        .collect();

    // Build archive_refs for the return value.
    let archive_refs: Vec<Value> = archives
        .iter()
        .map(|m| {
            let tool_name = extract_tool_name(&m.tags, "tool-archive");
            json!({"id": m.id, "tool_name": tool_name})
        })
        .collect();

    // Pre-compute the archive-refs section so we can reserve its size before
    // budgeting observation content (fixes P2: archive refs previously appended
    // without any budget check, allowing overflow past token_budget).
    let archive_section: String = archives
        .iter()
        .map(|m| {
            let tn = extract_tool_name(&m.tags, "tool-archive");
            format!(
                "**Archive ref:** [{}] ID={} — call `memory_get_archived_output` with archive_id={} to retrieve full output\n",
                tn, m.id, m.id
            )
        })
        .collect();

    // Reserve 500 tokens for structural markdown + archive section, then split
    // the rest evenly across observations.
    let archive_reserved = archive_section.len() / 4;
    let obs_count = observations.len();
    let content_budget_chars =
        (token_budget.saturating_sub(500 + archive_reserved)) * 4;
    let chars_per_obs = if obs_count > 0 {
        content_budget_chars / obs_count
    } else {
        content_budget_chars
    };

    // Build markdown.
    let mut md = format!(
        "# Working Memory — Session {}\n\n## Tool Observations ({} total)\n\n",
        session_id,
        obs_count
    );

    for (i, m) in observations.iter().enumerate() {
        let tool_name = extract_tool_name(&m.tags, "tool-observation");
        let content = if m.content.len() > chars_per_obs && chars_per_obs > 0 {
            format!("{}…", safe_truncate(&m.content, chars_per_obs))
        } else {
            m.content.clone()
        };
        md.push_str(&format!(
            "### {} (observation #{})\n{}\n\n---\n",
            tool_name,
            i + 1,
            content
        ));
    }

    md.push_str(&archive_section);

    let tokens_estimate = md.len() / 4;

    json!({
        "working_memory": md,
        "observation_count": obs_count,
        "archive_count": archives.len(),
        "archive_refs": archive_refs,
        "tokens_estimate": tokens_estimate
    })
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

#[cfg(test)]
mod context_tests {
    use super::safe_truncate;

    // safe_truncate tests
    #[test]
    fn test_safe_truncate_ascii() {
        assert_eq!(safe_truncate("hello world", 5), "hello");
    }

    #[test]
    fn test_safe_truncate_within_limit() {
        assert_eq!(safe_truncate("hi", 100), "hi");
    }

    #[test]
    fn test_safe_truncate_empty() {
        assert_eq!(safe_truncate("", 10), "");
    }

    #[test]
    fn test_safe_truncate_multibyte_emoji() {
        // "😀" is 4 bytes (U+1F600). Truncating at byte 5 should back up to byte 4
        // (the char boundary), not panic.
        let s = "😀hello";
        // 😀 = 4 bytes, 'h' starts at byte 4
        // max_bytes=5 should land at the char boundary at byte 4 (before 'h')
        let result = safe_truncate(s, 5);
        assert!(s.is_char_boundary(result.len()), "result must end on char boundary");
        assert!(!result.contains('\u{FFFD}'), "must not produce replacement chars");
    }

    #[test]
    fn test_safe_truncate_multibyte_cjk() {
        // "日" is 3 bytes. Truncating at byte 4 should back up to byte 3.
        let s = "日本語";
        let result = safe_truncate(s, 4);
        assert!(s.is_char_boundary(result.len()));
        // should contain exactly one CJK char ("日") or be empty
        assert!(result == "日" || result.is_empty());
    }

    #[test]
    fn test_safe_truncate_exact_boundary() {
        // Exactly at a char boundary should not back up
        let s = "abcdef";
        assert_eq!(safe_truncate(s, 3), "abc");
    }

    #[test]
    fn test_safe_truncate_zero() {
        assert_eq!(safe_truncate("hello", 0), "");
    }
}
