//! Memory CRUD tool handlers.

use serde_json::{json, Value};

use crate::realtime::RealtimeEvent;
use crate::storage::queries::*;
use crate::types::*;

use super::HandlerContext;

pub fn memory_create(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::find_similar_by_embedding;

    let input: CreateMemoryInput = match serde_json::from_value(params) {
        Ok(i) => i,
        Err(e) => return json!({"error": e.to_string()}),
    };

    // Semantic deduplication
    if input.dedup_mode != DedupMode::Allow {
        if let Some(threshold) = input.dedup_threshold {
            if let Ok(query_embedding) = ctx.embedder.embed(&input.content) {
                let workspace = input.workspace.as_deref();
                let similar_result = ctx.storage.with_connection(|conn| {
                    find_similar_by_embedding(
                        conn,
                        &query_embedding,
                        &input.scope,
                        workspace,
                        threshold,
                    )
                });

                if let Ok(Some((existing, similarity))) = similar_result {
                    match input.dedup_mode {
                        DedupMode::Reject => {
                            return json!({
                                "error": format!(
                                    "Similar memory detected (id={}, similarity={:.3}). Use dedup_mode='allow' to create anyway.",
                                    existing.id, similarity
                                ),
                                "existing_id": existing.id,
                                "similarity": similarity
                            });
                        }
                        DedupMode::Skip => {
                            return json!(existing);
                        }
                        DedupMode::Merge => {
                            let merge_result = ctx.storage.with_transaction(|conn| {
                                let mut merged_tags = existing.tags.clone();
                                for tag in &input.tags {
                                    if !merged_tags.contains(tag) {
                                        merged_tags.push(tag.clone());
                                    }
                                }

                                let mut merged_metadata = existing.metadata.clone();
                                for (key, value) in &input.metadata {
                                    merged_metadata.insert(key.clone(), value.clone());
                                }

                                let update_input = UpdateMemoryInput {
                                    content: None,
                                    memory_type: None,
                                    tags: Some(merged_tags),
                                    metadata: Some(merged_metadata),
                                    importance: input.importance,
                                    scope: None,
                                    ttl_seconds: input.ttl_seconds,
                                    event_time: None,
                                    trigger_pattern: None,
                                };

                                update_memory(conn, existing.id, &update_input)
                            });

                            return match merge_result {
                                Ok(memory) => json!(memory),
                                Err(e) => json!({"error": e.to_string()}),
                            };
                        }
                        DedupMode::Allow => {}
                    }
                }
            }
        }
    }

    let result = ctx.storage.with_transaction(|conn| {
        let memory = create_memory(conn, &input)?;
        let mut fuzzy = ctx.fuzzy_engine.lock();
        fuzzy.add_to_vocabulary(&memory.content);
        Ok(memory)
    });

    match result {
        Ok(memory) => {
            ctx.search_cache
                .invalidate_for_workspace(Some(memory.workspace.as_str()));
            if let Some(ref manager) = ctx.realtime {
                manager.broadcast(RealtimeEvent::memory_created(
                    memory.id,
                    memory.content.clone(),
                ));
            }
            json!(memory)
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

pub fn context_seed(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::create_memory_batch;
    use std::collections::HashMap;

    #[derive(serde::Deserialize)]
    struct ContextSeedFact {
        content: String,
        category: Option<String>,
        confidence: Option<f32>,
    }

    #[derive(serde::Deserialize)]
    struct ContextSeedInput {
        entity_context: Option<String>,
        workspace: Option<String>,
        base_tags: Option<Vec<String>>,
        ttl_seconds: Option<i64>,
        disable_ttl: Option<bool>,
        facts: Vec<ContextSeedFact>,
    }

    let input: ContextSeedInput = match serde_json::from_value(params) {
        Ok(i) => i,
        Err(e) => return json!({"error": e.to_string()}),
    };

    if input.facts.is_empty() {
        return json!({"error": "facts must have at least 1 item"});
    }

    fn norm_tag(tag: &str) -> String {
        tag.trim()
            .trim_start_matches('#')
            .replace(' ', "_")
            .to_lowercase()
    }

    fn norm_entity(entity: &str) -> Option<String> {
        let e = entity.trim();
        if e.is_empty() || e.eq_ignore_ascii_case("general") {
            return None;
        }
        Some(format!("entity:{}", e.replace(' ', "_").to_lowercase()))
    }

    fn clamp_confidence(val: Option<f32>) -> f32 {
        val.unwrap_or(0.7).clamp(0.0, 1.0)
    }

    fn ttl_for_confidence(confidence: f32) -> Option<i64> {
        if confidence >= 0.85 {
            None
        } else if confidence >= 0.6 {
            Some(90 * 24 * 60 * 60)
        } else {
            Some(30 * 24 * 60 * 60)
        }
    }

    let mut entity_context = input
        .entity_context
        .unwrap_or_else(|| "General".to_string());
    if entity_context.len() > 200 {
        entity_context.truncate(200);
    }
    let entity_tag = norm_entity(&entity_context);
    let base_tags: Vec<String> = input
        .base_tags
        .unwrap_or_default()
        .iter()
        .map(|t| norm_tag(t))
        .filter(|t| !t.is_empty())
        .collect();
    let ttl_override = input.ttl_seconds;
    let disable_ttl = input.disable_ttl.unwrap_or(false);

    let mut inputs = Vec::with_capacity(input.facts.len());

    for fact in input.facts {
        let content = fact.content.trim().to_string();
        if content.is_empty() {
            continue;
        }

        let category = fact.category.unwrap_or_else(|| "fact".to_string());
        let confidence = clamp_confidence(fact.confidence);
        let ttl_seconds = if disable_ttl {
            None
        } else if let Some(ttl) = ttl_override {
            if ttl <= 0 {
                None
            } else {
                Some(ttl)
            }
        } else {
            ttl_for_confidence(confidence)
        };
        let (tier, ttl) = if let Some(ttl) = ttl_seconds {
            (MemoryTier::Daily, Some(ttl))
        } else {
            (MemoryTier::Permanent, None)
        };

        let rich_content = if entity_context.eq_ignore_ascii_case("General") {
            content.clone()
        } else {
            format!("[{}] {}", entity_context.trim(), content)
        };

        let mut tags = base_tags.clone();
        tags.push("origin:seed".to_string());
        tags.push("status:unverified".to_string());
        tags.push(format!("category:{}", norm_tag(&category)));
        tags.push(format!("confidence:{:.2}", confidence));
        if let Some(et) = &entity_tag {
            tags.push(et.clone());
        }
        tags.sort();
        tags.dedup();

        let mut metadata: HashMap<String, Value> = HashMap::new();
        metadata.insert("origin".to_string(), json!("seed"));
        metadata.insert("status".to_string(), json!("unverified"));
        metadata.insert("confidence".to_string(), json!(confidence));
        metadata.insert("entity_context".to_string(), json!(entity_context));
        metadata.insert("category".to_string(), json!(category));
        metadata.insert(
            "seeded_at".to_string(),
            json!(chrono::Utc::now().to_rfc3339()),
        );

        inputs.push(CreateMemoryInput {
            content: rich_content,
            memory_type: MemoryType::Context,
            tags,
            metadata,
            importance: None,
            scope: MemoryScope::Global,
            workspace: input.workspace.clone(),
            tier,
            defer_embedding: false,
            ttl_seconds: ttl,
            dedup_mode: DedupMode::Allow,
            dedup_threshold: None,
            event_time: None,
            event_duration_seconds: None,
            trigger_pattern: None,
            summary_of_id: None,
        });
    }

    if inputs.is_empty() {
        return json!({"error": "facts must contain at least one non-empty content"});
    }

    let result = ctx
        .storage
        .with_transaction(|conn| create_memory_batch(conn, &inputs));

    match result {
        Ok(batch) => {
            ctx.search_cache
                .invalidate_for_workspace(input.workspace.as_deref());

            {
                let mut fuzzy = ctx.fuzzy_engine.lock();
                for memory in &batch.created {
                    fuzzy.add_to_vocabulary(&memory.content);
                }
            }

            for memory in &batch.created {
                if let Some(ref manager) = ctx.realtime {
                    manager.broadcast(RealtimeEvent::memory_created(
                        memory.id,
                        memory.content.clone(),
                    ));
                }
            }

            json!({
                "status": "success",
                "seeded_count": batch.total_created,
                "memory_ids": batch.created.iter().map(|m| m.id).collect::<Vec<_>>(),
                "entity": if entity_context.is_empty() { "General" } else { entity_context.as_str() },
                "failed": batch.failed
            })
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

pub fn memory_get(ctx: &HandlerContext, params: Value) -> Value {
    let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    ctx.storage
        .with_connection(|conn| {
            let memory = get_memory(conn, id)?;
            Ok(json!(memory))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_update(ctx: &HandlerContext, params: Value) -> Value {
    let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let input: UpdateMemoryInput = match serde_json::from_value(params.clone()) {
        Ok(i) => i,
        Err(e) => return json!({"error": e.to_string()}),
    };

    let mut changes = Vec::new();
    if input.content.is_some() {
        changes.push("content".to_string());
    }
    if input.memory_type.is_some() {
        changes.push("memory_type".to_string());
    }
    if input.tags.is_some() {
        changes.push("tags".to_string());
    }
    if input.metadata.is_some() {
        changes.push("metadata".to_string());
    }
    if input.importance.is_some() {
        changes.push("importance".to_string());
    }

    let result = ctx.storage.with_transaction(|conn| {
        let memory = update_memory(conn, id, &input)?;
        Ok(memory)
    });

    match result {
        Ok(memory) => {
            ctx.search_cache.invalidate_for_memory(memory.id);
            if let Some(ref manager) = ctx.realtime {
                manager.broadcast(RealtimeEvent::memory_updated(memory.id, changes));
            }
            json!(memory)
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

pub fn memory_delete(ctx: &HandlerContext, params: Value) -> Value {
    let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);

    let result = ctx.storage.with_transaction(|conn| {
        delete_memory(conn, id)?;
        Ok(id)
    });

    match result {
        Ok(deleted_id) => {
            ctx.search_cache.invalidate_for_memory(deleted_id);
            if let Some(ref manager) = ctx.realtime {
                manager.broadcast(RealtimeEvent::memory_deleted(deleted_id));
            }
            json!({"deleted": deleted_id})
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

pub fn memory_list(ctx: &HandlerContext, params: Value) -> Value {
    let options: ListOptions = serde_json::from_value(params).unwrap_or_default();
    ctx.storage
        .with_connection(|conn| {
            let memories = list_memories(conn, &options)?;
            Ok(json!(memories))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_create_daily(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::create_memory;

    let content = match params.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json!({"error": "content is required"}),
    };

    let memory_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(MemoryType::Note);

    let tags: Vec<String> = params
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let metadata: std::collections::HashMap<String, serde_json::Value> = params
        .get("metadata")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let importance = params
        .get("importance")
        .and_then(|v| v.as_f64())
        .map(|v| v as f32);

    let ttl_seconds = params
        .get("ttl_seconds")
        .and_then(|v| v.as_i64())
        .unwrap_or(86400);

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(String::from);

    let input = CreateMemoryInput {
        content,
        memory_type,
        tags,
        metadata,
        importance,
        scope: Default::default(),
        workspace,
        tier: MemoryTier::Daily,
        defer_embedding: false,
        ttl_seconds: Some(ttl_seconds),
        dedup_mode: Default::default(),
        dedup_threshold: None,
        event_time: None,
        event_duration_seconds: None,
        trigger_pattern: None,
        summary_of_id: None,
    };

    ctx.storage
        .with_connection(|conn| {
            let memory = create_memory(conn, &input)?;
            Ok(json!(memory))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_promote_to_permanent(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::promote_to_permanent;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let memory = promote_to_permanent(conn, id)?;
            Ok(json!({"success": true, "memory": memory}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_checkpoint(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::create_checkpoint;
    use std::collections::HashMap;

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return json!({"error": "session_id is required"}),
    };

    let summary = match params.get("summary").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return json!({"error": "summary is required"}),
    };

    let context: HashMap<String, Value> = params
        .get("context")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    let workspace = params.get("workspace").and_then(|v| v.as_str());

    ctx.storage
        .with_connection(|conn| {
            let memory = create_checkpoint(conn, session_id, summary, &context, workspace)?;
            Ok(json!(memory))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_boost(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::boost_memory;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    let boost_amount = params
        .get("boost_amount")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.2) as f32;
    let duration_seconds = params.get("duration_seconds").and_then(|v| v.as_i64());

    ctx.storage
        .with_connection(|conn| {
            let memory = boost_memory(conn, id, boost_amount, duration_seconds)?;
            Ok(json!(memory))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_create_episodic(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::create_memory;
    use chrono::DateTime;

    let content = match params.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json!({"error": "content is required"}),
    };

    let event_time = match params.get("event_time").and_then(|v| v.as_str()) {
        Some(s) => match DateTime::parse_from_rfc3339(s) {
            Ok(dt) => Some(dt.with_timezone(&chrono::Utc)),
            Err(e) => return json!({"error": format!("Invalid event_time format: {}", e)}),
        },
        None => return json!({"error": "event_time is required for episodic memories"}),
    };

    let event_duration_seconds = params
        .get("event_duration_seconds")
        .and_then(|v| v.as_i64());
    let tags: Vec<String> = params
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let metadata: std::collections::HashMap<String, Value> = params
        .get("metadata")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    let importance = params
        .get("importance")
        .and_then(|v| v.as_f64())
        .map(|f| f as f32);
    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(String::from);

    let input = CreateMemoryInput {
        content,
        memory_type: MemoryType::Episodic,
        tags,
        metadata,
        importance,
        scope: MemoryScope::Global,
        workspace,
        tier: MemoryTier::Permanent,
        defer_embedding: false,
        ttl_seconds: None,
        dedup_mode: DedupMode::Allow,
        dedup_threshold: None,
        event_time,
        event_duration_seconds,
        trigger_pattern: None,
        summary_of_id: None,
    };

    ctx.storage
        .with_transaction(|conn| {
            let memory = create_memory(conn, &input)?;
            Ok(json!(memory))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_create_procedural(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::create_memory;

    let content = match params.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json!({"error": "content is required"}),
    };

    let trigger_pattern = match params.get("trigger_pattern").and_then(|v| v.as_str()) {
        Some(p) => Some(p.to_string()),
        None => return json!({"error": "trigger_pattern is required for procedural memories"}),
    };

    let tags: Vec<String> = params
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let metadata: std::collections::HashMap<String, Value> = params
        .get("metadata")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    let importance = params
        .get("importance")
        .and_then(|v| v.as_f64())
        .map(|f| f as f32);
    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(String::from);

    let input = CreateMemoryInput {
        content,
        memory_type: MemoryType::Procedural,
        tags,
        metadata,
        importance,
        scope: MemoryScope::Global,
        workspace,
        tier: MemoryTier::Permanent,
        defer_embedding: false,
        ttl_seconds: None,
        dedup_mode: DedupMode::Allow,
        dedup_threshold: None,
        event_time: None,
        event_duration_seconds: None,
        trigger_pattern,
        summary_of_id: None,
    };

    ctx.storage
        .with_transaction(|conn| {
            let memory = create_memory(conn, &input)?;
            Ok(json!(memory))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_get_timeline(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::get_episodic_timeline;

    let start_time = params
        .get("start_time")
        .and_then(|v| v.as_str())
        .and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))
        });
    let end_time = params
        .get("end_time")
        .and_then(|v| v.as_str())
        .and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc))
        });
    let workspace = params.get("workspace").and_then(|v| v.as_str());
    let tags: Option<Vec<String>> = params.get("tags").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    });
    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);

    ctx.storage
        .with_connection(|conn| {
            let memories = get_episodic_timeline(
                conn,
                start_time,
                end_time,
                workspace,
                tags.as_deref(),
                limit,
            )?;
            Ok(json!(memories))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_get_procedures(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::get_procedural_memories;

    let trigger_pattern = params.get("trigger_pattern").and_then(|v| v.as_str());
    let workspace = params.get("workspace").and_then(|v| v.as_str());
    let min_success_rate = params
        .get("min_success_rate")
        .and_then(|v| v.as_f64())
        .map(|f| f as f32);
    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);

    ctx.storage
        .with_connection(|conn| {
            let memories =
                get_procedural_memories(conn, trigger_pattern, workspace, min_success_rate, limit)?;
            Ok(json!(memories))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn record_procedure_outcome(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::record_procedure_outcome;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };
    let success = match params.get("success").and_then(|v| v.as_bool()) {
        Some(s) => s,
        None => return json!({"error": "success (boolean) is required"}),
    };

    ctx.storage
        .with_transaction(|conn| {
            let memory = record_procedure_outcome(conn, id, success)?;
            Ok(json!(memory))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn set_expiration(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::set_memory_expiration;

    let id = params.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let ttl_seconds = params.get("ttl_seconds").and_then(|v| v.as_i64());

    if ttl_seconds.is_none() {
        return json!({"error": "ttl_seconds is required"});
    }

    ctx.storage
        .with_transaction(|conn| {
            let memory = set_memory_expiration(conn, id, ttl_seconds)?;
            Ok(json!({
                "success": true,
                "memory": memory,
                "message": if ttl_seconds == Some(0) {
                    "Expiration removed".to_string()
                } else {
                    format!("Expiration set to {} seconds from now", ttl_seconds.unwrap())
                }
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn cleanup_expired(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::{cleanup_expired_memories, count_expired_memories};

    let _ = params;

    ctx.storage
        .with_transaction(|conn| {
            let _count_before = count_expired_memories(conn)?;
            let deleted = cleanup_expired_memories(conn)?;
            Ok(json!({
                "success": true,
                "deleted": deleted,
                "message": format!("Cleaned up {} expired memories", deleted)
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_create_batch(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::create_memory_batch;

    let memories = match params.get("memories").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return json!({"error": "memories array is required"}),
    };

    let inputs: Vec<CreateMemoryInput> = memories
        .iter()
        .filter_map(|m| serde_json::from_value(m.clone()).ok())
        .collect();

    if inputs.is_empty() {
        return json!({"error": "No valid memory inputs provided"});
    }

    ctx.storage
        .with_connection(|conn| {
            let result = create_memory_batch(conn, &inputs)?;
            Ok(json!(result))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_delete_batch(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::delete_memory_batch;

    let ids: Vec<i64> = match params.get("ids").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_i64()).collect(),
        None => return json!({"error": "ids array is required"}),
    };

    if ids.is_empty() {
        return json!({"error": "No valid IDs provided"});
    }

    ctx.storage
        .with_connection(|conn| {
            let result = delete_memory_batch(conn, &ids)?;
            Ok(json!(result))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_create_section(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::create_section_memory;

    let title = match params.get("title").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return json!({"error": "title is required"}),
    };

    let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let parent_id = params.get("parent_id").and_then(|v| v.as_i64());
    let level = params.get("level").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
    let workspace = params.get("workspace").and_then(|v| v.as_str());

    ctx.storage
        .with_connection(|conn| {
            let memory = create_section_memory(conn, title, content, parent_id, level, workspace)?;
            Ok(json!(memory))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn create_todo(ctx: &HandlerContext, params: Value) -> Value {
    let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let priority = params
        .get("priority")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let tags: Vec<String> = params
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut metadata = std::collections::HashMap::new();
    metadata.insert("priority".to_string(), json!(priority));
    if let Some(due) = params.get("due_date") {
        metadata.insert("due_date".to_string(), due.clone());
    }

    let importance: f32 = match priority {
        "critical" => 1.0,
        "high" => 0.8,
        "medium" => 0.5,
        "low" => 0.3,
        _ => 0.5,
    };

    let input = CreateMemoryInput {
        content: content.to_string(),
        memory_type: MemoryType::Todo,
        tags,
        metadata,
        importance: Some(importance),
        scope: Default::default(),
        workspace: None,
        tier: Default::default(),
        defer_embedding: false,
        ttl_seconds: None,
        dedup_mode: Default::default(),
        dedup_threshold: None,
        event_time: None,
        event_duration_seconds: None,
        trigger_pattern: None,
        summary_of_id: None,
    };

    memory_create(ctx, serde_json::to_value(input).unwrap_or_default())
}

pub fn create_issue(ctx: &HandlerContext, params: Value) -> Value {
    let title = params.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let description = params
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let severity = params
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");
    let tags: Vec<String> = params
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let content = if description.is_empty() {
        title.to_string()
    } else {
        format!("{}\n\n{}", title, description)
    };

    let mut metadata = std::collections::HashMap::new();
    metadata.insert("severity".to_string(), json!(severity));
    metadata.insert("title".to_string(), json!(title));

    let importance: f32 = match severity {
        "critical" => 1.0,
        "high" => 0.8,
        "medium" => 0.5,
        "low" => 0.3,
        _ => 0.5,
    };

    let input = CreateMemoryInput {
        content,
        memory_type: MemoryType::Issue,
        tags,
        metadata,
        importance: Some(importance),
        scope: Default::default(),
        workspace: None,
        tier: Default::default(),
        defer_embedding: false,
        ttl_seconds: None,
        dedup_mode: Default::default(),
        dedup_threshold: None,
        event_time: None,
        event_duration_seconds: None,
        trigger_pattern: None,
        summary_of_id: None,
    };

    memory_create(ctx, serde_json::to_value(input).unwrap_or_default())
}
