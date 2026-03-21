//! Summarization, full-memory retrieval, context budget, and archival handlers.

use serde_json::{json, Value};

use super::HandlerContext;

pub fn memory_summarize(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::{create_memory, get_memory};
    use crate::types::{CreateMemoryInput, MemoryTier, MemoryType};

    let memory_ids: Vec<i64> = match params.get("memory_ids") {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(ids) => ids,
            Err(e) => return json!({"error": format!("Invalid memory_ids: {}", e)}),
        },
        None => return json!({"error": "memory_ids is required"}),
    };

    if memory_ids.is_empty() {
        return json!({"error": "memory_ids cannot be empty"});
    }

    let provided_summary = params.get("summary").and_then(|v| v.as_str());
    let max_length = params
        .get("max_length")
        .and_then(|v| v.as_u64())
        .unwrap_or(500) as usize;
    let workspace = params.get("workspace").and_then(|v| v.as_str());
    let tags: Option<Vec<String>> = params
        .get("tags")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    ctx.storage
        .with_connection(|conn| {
            let mut contents: Vec<String> = Vec::with_capacity(memory_ids.len());
            let mut first_memory_workspace: Option<String> = None;

            for id in &memory_ids {
                match get_memory(conn, *id) {
                    Ok(mem) => {
                        contents.push(mem.content);
                        if first_memory_workspace.is_none() {
                            first_memory_workspace = Some(mem.workspace);
                        }
                    }
                    Err(e) => {
                        return Err(crate::error::EngramError::Internal(format!(
                            "Memory {} not found: {}",
                            id, e
                        )));
                    }
                }
            }

            let summary_text = if let Some(s) = provided_summary {
                s.to_string()
            } else {
                let combined = contents.join("\n\n---\n\n");
                if combined.len() <= max_length {
                    combined
                } else {
                    let head_len = (max_length as f64 * 0.6) as usize;
                    let tail_len = (max_length as f64 * 0.3) as usize;
                    let head: String = combined.chars().take(head_len).collect();
                    let tail: String = combined
                        .chars()
                        .rev()
                        .take(tail_len)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
                    let truncated = combined.len() - head_len - tail_len;
                    format!("{}...[{} chars truncated]...{}", head, truncated, tail)
                }
            };

            let input = CreateMemoryInput {
                content: summary_text,
                memory_type: MemoryType::Summary,
                importance: Some(0.6),
                tags: tags.unwrap_or_default(),
                workspace: workspace.map(|s| s.to_string()).or(first_memory_workspace),
                tier: MemoryTier::Permanent,
                summary_of_id: Some(memory_ids[0]),
                ..Default::default()
            };

            let memory = create_memory(conn, &input)?;

            Ok(json!({
                "id": memory.id,
                "memory_type": "summary",
                "summarized_count": memory_ids.len(),
                "original_ids": memory_ids,
                "summary_length": memory.content.len()
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_get_full(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::get_memory;
    use crate::types::MemoryType;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let memory = match get_memory(conn, id) {
                Ok(m) => m,
                Err(_) => return Ok(json!({"error": "Memory not found"})),
            };

            if memory.memory_type == MemoryType::Summary {
                if let Some(original_id) = memory.summary_of_id {
                    match get_memory(conn, original_id) {
                        Ok(original) => {
                            return Ok(json!({
                                "id": id,
                                "is_summary": true,
                                "original_id": original_id,
                                "original_content": original.content,
                                "summary_content": memory.content
                            }));
                        }
                        Err(_) => {
                            return Ok(json!({
                                "error": "original_deleted",
                                "id": id,
                                "original_id": original_id,
                                "summary": memory.content
                            }));
                        }
                    }
                }
            }

            Ok(json!({
                "id": id,
                "is_summary": false,
                "content": memory.content
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn context_budget_check(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::compression::check_context_budget;
    use crate::storage::queries::get_memory;

    let memory_ids: Vec<i64> = match params.get("memory_ids") {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(ids) => ids,
            Err(e) => return json!({"error": format!("Invalid memory_ids: {}", e)}),
        },
        None => return json!({"error": "memory_ids is required"}),
    };

    let model = match params.get("model").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return json!({"error": "model is required"}),
    };

    let encoding = params.get("encoding").and_then(|v| v.as_str());

    let budget = match params.get("budget").and_then(|v| v.as_u64()) {
        Some(b) => b as usize,
        None => return json!({"error": "budget is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let mut contents: Vec<(i64, String)> = Vec::with_capacity(memory_ids.len());

            for id in &memory_ids {
                match get_memory(conn, *id) {
                    Ok(mem) => contents.push((*id, mem.content)),
                    Err(_) => return Ok(json!({"error": format!("Memory {} not found", id)})),
                }
            }

            match check_context_budget(&contents, model, encoding, budget) {
                Ok(result) => Ok(json!(result)),
                Err(e) => Ok(json!({"error": e.to_string()})),
            }
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_archive_old(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::{create_memory, list_memories};
    use crate::types::{CreateMemoryInput, ListOptions, MemoryTier, MemoryType};
    use chrono::{Duration, Utc};
    use rusqlite::params;

    let max_age_days = params
        .get("max_age_days")
        .and_then(|v| v.as_i64())
        .unwrap_or(90);
    let max_importance = params
        .get("max_importance")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5) as f32;
    let min_access_count = params
        .get("min_access_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(5);
    let workspace = params.get("workspace").and_then(|v| v.as_str());
    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let cutoff_date = Utc::now() - Duration::days(max_age_days);

    ctx.storage
        .with_connection(|conn| {
            let options = ListOptions {
                workspace: workspace.map(|s| s.to_string()),
                limit: Some(1000),
                ..Default::default()
            };

            let all_memories = list_memories(conn, &options)?;

            let candidates: Vec<_> = all_memories
                .into_iter()
                .filter(|m| {
                    m.created_at < cutoff_date
                        && m.importance <= max_importance
                        && m.access_count < min_access_count as i32
                        && m.memory_type != MemoryType::Summary
                        && m.memory_type != MemoryType::Checkpoint
                })
                .collect();

            if dry_run {
                let summaries: Vec<_> = candidates
                    .iter()
                    .map(|m| {
                        json!({
                            "id": m.id,
                            "memory_type": m.memory_type,
                            "importance": m.importance,
                            "access_count": m.access_count,
                            "created_at": m.created_at.to_rfc3339(),
                            "content_preview": m.content.chars().take(100).collect::<String>()
                        })
                    })
                    .collect();

                return Ok(json!({
                    "dry_run": true,
                    "would_archive": candidates.len(),
                    "candidates": summaries
                }));
            }

            let mut archived = 0;
            let mut errors: Vec<String> = Vec::new();

            for memory in candidates {
                let summary_text = if memory.content.len() > 200 {
                    let head: String = memory.content.chars().take(120).collect();
                    let tail: String = memory
                        .content
                        .chars()
                        .rev()
                        .take(60)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
                    format!("{}...{}", head, tail)
                } else {
                    memory.content.clone()
                };

                let input = CreateMemoryInput {
                    content: format!("[Archived {:?}] {}", memory.memory_type, summary_text),
                    memory_type: MemoryType::Summary,
                    importance: Some(memory.importance),
                    tags: memory.tags.clone(),
                    workspace: Some(memory.workspace.clone()),
                    tier: MemoryTier::Permanent,
                    summary_of_id: Some(memory.id),
                    ..Default::default()
                };

                match create_memory(conn, &input) {
                    Ok(_) => {
                        match conn.execute(
                            "UPDATE memories SET lifecycle_state = 'archived' WHERE id = ? AND valid_to IS NULL",
                            params![memory.id],
                        ) {
                            Ok(_) => archived += 1,
                            Err(e) => errors.push(format!(
                                "Memory {}: summary created but failed to mark archived: {}",
                                memory.id, e
                            )),
                        }
                    }
                    Err(e) => errors.push(format!("Memory {}: {}", memory.id, e)),
                }
            }

            Ok(json!({
                "dry_run": false,
                "archived": archived,
                "errors": errors
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
