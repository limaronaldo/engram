//! Miscellaneous tool handlers: tags, export/import, maintenance, images,
//! auto-tagging, Langfuse integration (feature-gated), and Meilisearch tools
//! (feature-gated).
//!
//! Stats/cache/compact handlers moved to `stats.rs`.
//! Project context handlers moved to `project_context.rs`.
//! Document ingestion handler moved to `document_ingest.rs`.
//! Summarization/archival handlers moved to `summarize.rs`.

use serde_json::{json, Value};

use super::HandlerContext;

// ── Tag Utilities ─────────────────────────────────────────────────────────────

pub fn memory_tags(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::list_tags;

    ctx.storage
        .with_connection(|conn| {
            let tags = list_tags(conn)?;
            Ok(json!({"tags": tags}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_tag_hierarchy(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::get_tag_hierarchy;

    ctx.storage
        .with_connection(|conn| {
            let hierarchy = get_tag_hierarchy(conn)?;
            Ok(json!({"hierarchy": hierarchy}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_validate_tags(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::validate_tags;

    ctx.storage
        .with_connection(|conn| {
            let result = validate_tags(conn)?;
            Ok(json!(result))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── Import / Export ───────────────────────────────────────────────────────────

pub fn memory_export(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::export_memories;

    ctx.storage
        .with_connection(|conn| {
            let data = export_memories(conn)?;
            Ok(json!(data))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_import(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::{import_memories, ExportData};

    let data: ExportData = match params
        .get("data")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(d) => d,
        None => return json!({"error": "data object is required"}),
    };

    let skip_duplicates = params
        .get("skip_duplicates")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    ctx.storage
        .with_connection(|conn| {
            let result = import_memories(conn, &data, skip_duplicates)?;
            Ok(json!(result))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── Maintenance ───────────────────────────────────────────────────────────────

pub fn memory_rebuild_embeddings(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::rebuild_embeddings;

    ctx.storage
        .with_connection(|conn| {
            let count = rebuild_embeddings(conn)?;
            Ok(json!({"rebuilt": count}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_rebuild_crossrefs(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::rebuild_crossrefs;

    ctx.storage
        .with_connection(|conn| {
            let count = rebuild_crossrefs(conn)?;
            Ok(json!({"rebuilt": count}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── Image Handling ────────────────────────────────────────────────────────────

pub fn memory_upload_image(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::{upload_image, ImageStorageConfig, LocalImageStorage};

    let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "memory_id is required"}),
    };

    let file_path = match params.get("file_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return json!({"error": "file_path is required"}),
    };

    let image_index = params
        .get("image_index")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let caption = params.get("caption").and_then(|v| v.as_str());

    let config = ImageStorageConfig::default();
    let image_storage = match LocalImageStorage::new(config.local_dir) {
        Ok(s) => s,
        Err(e) => return json!({"error": format!("Failed to initialize image storage: {}", e)}),
    };

    ctx.storage
        .with_connection(|conn| {
            let image_ref = upload_image(
                conn,
                &image_storage,
                memory_id,
                file_path,
                image_index,
                caption,
            )?;
            Ok(json!({
                "success": true,
                "image": image_ref
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_migrate_images(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::{migrate_images, ImageStorageConfig, LocalImageStorage};

    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let config = ImageStorageConfig::default();
    let image_storage = match LocalImageStorage::new(config.local_dir) {
        Ok(s) => s,
        Err(e) => return json!({"error": format!("Failed to initialize image storage: {}", e)}),
    };

    ctx.storage
        .with_connection(|conn| {
            let result = migrate_images(conn, &image_storage, dry_run)?;
            Ok(json!(result))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── Auto-Tagging ──────────────────────────────────────────────────────────────

pub fn memory_suggest_tags(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{AutoTagConfig, AutoTagger};
    use crate::storage::queries::get_memory;

    let (content, memory_type, existing_tags) = if let Some(id) = params
        .get("id")
        .or_else(|| params.get("memory_id"))
        .and_then(|v| v.as_i64())
    {
        match ctx.storage.with_connection(|conn| get_memory(conn, id)) {
            Ok(memory) => (memory.content, Some(memory.memory_type), memory.tags),
            Err(e) => return json!({"error": e.to_string()}),
        }
    } else if let Some(content) = params.get("content").and_then(|v| v.as_str()) {
        let memory_type = params
            .get("type")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok());
        let existing: Vec<String> = params
            .get("existing_tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        (content.to_string(), memory_type, existing)
    } else {
        return json!({"error": "Either 'id'/'memory_id' or 'content' is required"});
    };

    let mut config = AutoTagConfig::default();

    if let Some(min_conf) = params.get("min_confidence").and_then(|v| v.as_f64()) {
        config.min_confidence = min_conf as f32;
    }
    if let Some(max) = params.get("max_tags").and_then(|v| v.as_u64()) {
        config.max_tags = max as usize;
    }
    if let Some(v) = params.get("enable_patterns").and_then(|v| v.as_bool()) {
        config.enable_patterns = v;
    }
    if let Some(v) = params.get("enable_keywords").and_then(|v| v.as_bool()) {
        config.enable_keywords = v;
    }
    if let Some(v) = params.get("enable_entities").and_then(|v| v.as_bool()) {
        config.enable_entities = v;
    }
    if let Some(v) = params.get("enable_type_tags").and_then(|v| v.as_bool()) {
        config.enable_type_tags = v;
    }

    if let Some(mappings) = params.get("keyword_mappings").and_then(|v| v.as_object()) {
        for (keyword, tag) in mappings {
            if let Some(tag_str) = tag.as_str() {
                config
                    .keyword_mappings
                    .insert(keyword.clone(), tag_str.to_string());
            }
        }
    }

    let tagger = AutoTagger::new(config);
    let result = tagger.suggest_tags(&content, memory_type, &existing_tags);

    json!({
        "suggestions": result.suggestions,
        "analysis_count": result.analysis_count
    })
}

pub fn memory_auto_tag(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{AutoTagConfig, AutoTagger};
    use crate::storage::queries::{get_memory, update_memory};
    use crate::types::UpdateMemoryInput;

    let id = match params
        .get("id")
        .or_else(|| params.get("memory_id"))
        .and_then(|v| v.as_i64())
    {
        Some(id) => id,
        None => return json!({"error": "id or memory_id is required"}),
    };

    let apply = params
        .get("apply")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let merge = params
        .get("merge")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let mut config = AutoTagConfig::default();

    if let Some(min_conf) = params.get("min_confidence").and_then(|v| v.as_f64()) {
        config.min_confidence = min_conf as f32;
    }
    if let Some(max) = params.get("max_tags").and_then(|v| v.as_u64()) {
        config.max_tags = max as usize;
    }

    if let Some(mappings) = params.get("keyword_mappings").and_then(|v| v.as_object()) {
        for (keyword, tag) in mappings {
            if let Some(tag_str) = tag.as_str() {
                config
                    .keyword_mappings
                    .insert(keyword.clone(), tag_str.to_string());
            }
        }
    }

    let (memory, suggestions) = match ctx.storage.with_connection(|conn| {
        let memory = get_memory(conn, id)?;
        let tagger = AutoTagger::new(config);
        let result = tagger.suggest_for_memory(&memory);
        Ok((memory, result))
    }) {
        Ok(r) => r,
        Err(e) => return json!({"error": e.to_string()}),
    };

    if !apply {
        return json!({
            "memory_id": id,
            "suggestions": suggestions.suggestions,
            "applied": false,
            "message": "Tags suggested but not applied. Set apply=true to apply them."
        });
    }

    let suggested_tags: Vec<String> = suggestions
        .suggestions
        .iter()
        .map(|s| s.tag.clone())
        .collect();

    let new_tags = if merge {
        let mut tags = memory.tags.clone();
        for tag in suggested_tags.iter() {
            if !tags.iter().any(|t| t.to_lowercase() == tag.to_lowercase()) {
                tags.push(tag.clone());
            }
        }
        tags
    } else {
        suggested_tags.clone()
    };

    let update_input = UpdateMemoryInput {
        content: None,
        memory_type: None,
        tags: Some(new_tags.clone()),
        metadata: None,
        importance: None,
        scope: None,
        ttl_seconds: None,
        event_time: None,
        trigger_pattern: None,
        media_url: None,
    };

    match ctx
        .storage
        .with_transaction(|conn| update_memory(conn, id, &update_input))
    {
        Ok(updated_memory) => {
            json!({
                "memory_id": id,
                "suggestions": suggestions.suggestions,
                "applied": true,
                "applied_tags": suggested_tags,
                "final_tags": updated_memory.tags,
                "merged": merge
            })
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── Langfuse Integration (feature-gated) ──────────────────────────────────────

#[cfg(feature = "langfuse")]
pub fn langfuse_connect(ctx: &HandlerContext, params: Value) -> Value {
    use crate::integrations::langfuse::{LangfuseClient, LangfuseConfig};

    let public_key = params
        .get("public_key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("LANGFUSE_PUBLIC_KEY").ok());

    let secret_key = params
        .get("secret_key")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("LANGFUSE_SECRET_KEY").ok());

    let base_url = params
        .get("base_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://cloud.langfuse.com")
        .to_string();

    let (public_key, secret_key) = match (public_key, secret_key) {
        (Some(pk), Some(sk)) => (pk, sk),
        _ => {
            return json!({
                "error": "Missing credentials. Provide public_key and secret_key or set LANGFUSE_PUBLIC_KEY and LANGFUSE_SECRET_KEY environment variables."
            });
        }
    };

    let config = LangfuseConfig {
        public_key: public_key.clone(),
        secret_key,
        base_url: base_url.clone(),
    };

    let client = LangfuseClient::new(config);

    let connected = ctx
        .langfuse_runtime
        .block_on(async { client.test_connection().await });

    match connected {
        Ok(true) => json!({
            "status": "connected",
            "base_url": base_url,
            "public_key_prefix": &public_key[..8.min(public_key.len())]
        }),
        Ok(false) => json!({
            "status": "failed",
            "error": "Connection test failed"
        }),
        Err(e) => json!({
            "status": "error",
            "error": e.to_string()
        }),
    }
}

#[cfg(feature = "langfuse")]
pub fn langfuse_sync(ctx: &HandlerContext, params: Value) -> Value {
    use crate::integrations::langfuse::{LangfuseClient, LangfuseConfig};
    use crate::storage::queries::{upsert_sync_task, SyncTask};
    use chrono::{Duration, Utc};

    let config = match LangfuseConfig::from_env() {
        Some(c) => c,
        None => {
            return json!({
                "error": "Langfuse not configured. Call langfuse_connect first or set environment variables."
            });
        }
    };

    let since = params
        .get("since")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| Utc::now() - Duration::hours(24));

    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let task_id = uuid::Uuid::new_v4().to_string();
    let started_at = Utc::now().to_rfc3339();

    let initial_task = SyncTask {
        task_id: task_id.clone(),
        task_type: "langfuse_sync".to_string(),
        status: "running".to_string(),
        progress_percent: 0,
        traces_processed: 0,
        memories_created: 0,
        error_message: None,
        started_at: started_at.clone(),
        completed_at: None,
    };

    if let Err(e) = ctx
        .storage
        .with_connection(|conn| upsert_sync_task(conn, &initial_task))
    {
        return json!({"error": format!("Failed to create sync task: {}", e)});
    }

    let client = LangfuseClient::new(config);

    let result = ctx
        .langfuse_runtime
        .block_on(async { client.fetch_traces(since, limit).await });

    match result {
        Ok(traces) => {
            if dry_run {
                let trace_summaries: Vec<_> = traces
                    .iter()
                    .map(|t| {
                        json!({
                            "id": t.id,
                            "name": t.name,
                            "timestamp": t.timestamp.to_rfc3339(),
                            "user_id": t.user_id,
                            "tags": t.tags
                        })
                    })
                    .collect();

                let final_task = SyncTask {
                    task_id: task_id.clone(),
                    task_type: "langfuse_sync".to_string(),
                    status: "completed".to_string(),
                    progress_percent: 100,
                    traces_processed: traces.len() as i64,
                    memories_created: 0,
                    error_message: None,
                    started_at,
                    completed_at: Some(Utc::now().to_rfc3339()),
                };
                let _ = ctx
                    .storage
                    .with_connection(|conn| upsert_sync_task(conn, &final_task));

                return json!({
                    "task_id": task_id,
                    "dry_run": true,
                    "traces_found": traces.len(),
                    "traces": trace_summaries
                });
            }

            use crate::integrations::langfuse::trace_to_memory_content;
            use crate::storage::queries::create_memory;
            use crate::types::{CreateMemoryInput, MemoryType};

            let mut memories_created = 0i64;
            let mut errors: Vec<String> = Vec::new();

            for trace in &traces {
                let content = trace_to_memory_content(trace, &[]);

                let input = CreateMemoryInput {
                    content,
                    memory_type: MemoryType::Episodic,
                    importance: Some(0.5),
                    tags: {
                        let mut tags = trace.tags.clone();
                        tags.push("langfuse".to_string());
                        tags
                    },
                    workspace: workspace.clone(),
                    event_time: Some(trace.timestamp),
                    ..Default::default()
                };

                match ctx
                    .storage
                    .with_connection(|conn| create_memory(conn, &input))
                {
                    Ok(_) => memories_created += 1,
                    Err(e) => errors.push(format!("Trace {}: {}", trace.id, e)),
                }
            }

            let final_task = SyncTask {
                task_id: task_id.clone(),
                task_type: "langfuse_sync".to_string(),
                status: if errors.is_empty() {
                    "completed".to_string()
                } else {
                    "completed_with_errors".to_string()
                },
                progress_percent: 100,
                traces_processed: traces.len() as i64,
                memories_created,
                error_message: if errors.is_empty() {
                    None
                } else {
                    Some(errors.join("; "))
                },
                started_at,
                completed_at: Some(Utc::now().to_rfc3339()),
            };
            let _ = ctx
                .storage
                .with_connection(|conn| upsert_sync_task(conn, &final_task));

            json!({
                "task_id": task_id,
                "status": final_task.status,
                "traces_processed": traces.len(),
                "memories_created": memories_created,
                "errors": errors
            })
        }
        Err(e) => {
            let final_task = SyncTask {
                task_id: task_id.clone(),
                task_type: "langfuse_sync".to_string(),
                status: "failed".to_string(),
                progress_percent: 0,
                traces_processed: 0,
                memories_created: 0,
                error_message: Some(e.to_string()),
                started_at,
                completed_at: Some(Utc::now().to_rfc3339()),
            };
            let _ = ctx
                .storage
                .with_connection(|conn| upsert_sync_task(conn, &final_task));

            json!({
                "task_id": task_id,
                "status": "failed",
                "error": e.to_string()
            })
        }
    }
}

#[cfg(feature = "langfuse")]
pub fn langfuse_sync_status(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::get_sync_task;

    let task_id = match params.get("task_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "task_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| match get_sync_task(conn, task_id)? {
            Some(task) => Ok(json!(task)),
            None => Ok(json!({"error": "Task not found", "task_id": task_id})),
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

#[cfg(feature = "langfuse")]
pub fn langfuse_extract_patterns(ctx: &HandlerContext, params: Value) -> Value {
    use crate::integrations::langfuse::{extract_patterns, LangfuseClient, LangfuseConfig};
    use chrono::{Duration, Utc};

    let config = match LangfuseConfig::from_env() {
        Some(c) => c,
        None => {
            return json!({
                "error": "Langfuse not configured. Set LANGFUSE_PUBLIC_KEY and LANGFUSE_SECRET_KEY environment variables."
            });
        }
    };

    let since = params
        .get("since")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| Utc::now() - Duration::days(7));

    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

    let min_confidence = params
        .get("min_confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.7);

    let client = LangfuseClient::new(config);

    let result = ctx
        .langfuse_runtime
        .block_on(async { client.fetch_traces(since, limit).await });

    match result {
        Ok(traces) => {
            let patterns = extract_patterns(&traces);
            let filtered: Vec<_> = patterns
                .into_iter()
                .filter(|p| p.confidence >= min_confidence)
                .collect();

            json!({
                "traces_analyzed": traces.len(),
                "patterns_found": filtered.len(),
                "patterns": filtered
            })
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

#[cfg(feature = "langfuse")]
pub fn memory_from_trace(ctx: &HandlerContext, params: Value) -> Value {
    use crate::integrations::langfuse::{trace_to_memory_content, LangfuseClient, LangfuseConfig};
    use crate::storage::queries::create_memory;
    use crate::types::{CreateMemoryInput, MemoryType};

    let trace_id = match params.get("trace_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "trace_id is required"}),
    };

    let memory_type_str = params
        .get("memory_type")
        .and_then(|v| v.as_str())
        .unwrap_or("episodic");

    let memory_type = match memory_type_str {
        "note" => MemoryType::Note,
        "episodic" => MemoryType::Episodic,
        "procedural" => MemoryType::Procedural,
        "learning" => MemoryType::Learning,
        _ => MemoryType::Episodic,
    };

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let extra_tags: Vec<String> = params
        .get("tags")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let config = match LangfuseConfig::from_env() {
        Some(c) => c,
        None => {
            return json!({
                "error": "Langfuse not configured. Set environment variables."
            });
        }
    };

    let client = LangfuseClient::new(config);

    let trace_result = ctx
        .langfuse_runtime
        .block_on(async { client.fetch_trace(trace_id).await });

    match trace_result {
        Ok(Some(trace)) => {
            let content = trace_to_memory_content(&trace, &[]);

            let mut tags = trace.tags.clone();
            tags.push("langfuse".to_string());
            tags.push(format!("trace:{}", trace_id));
            tags.extend(extra_tags);

            let input = CreateMemoryInput {
                content,
                memory_type,
                importance: Some(0.6),
                tags,
                workspace,
                event_time: Some(trace.timestamp),
                ..Default::default()
            };

            ctx.storage
                .with_connection(|conn| {
                    let memory = create_memory(conn, &input)?;
                    Ok(json!({
                        "id": memory.id,
                        "trace_id": trace_id,
                        "memory_type": memory_type_str,
                        "content_length": memory.content.len()
                    }))
                })
                .unwrap_or_else(|e| json!({"error": e.to_string()}))
        }
        Ok(None) => json!({"error": format!("Trace {} not found", trace_id)}),
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── Meilisearch Tools (feature-gated) ─────────────────────────────────────────

#[cfg(feature = "meilisearch")]
pub fn meilisearch_search(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::StorageBackend;
    use crate::types::SearchOptions;

    let meili = match &ctx.meili {
        Some(m) => m,
        None => {
            return json!({"error": "Meilisearch not configured. Start server with --meilisearch-url and --meilisearch-indexer."})
        }
    };

    let query = match params.get("query").and_then(|v| v.as_str()) {
        Some(q) => q.to_string(),
        None => return json!({"error": "query is required"}),
    };

    let options = SearchOptions {
        limit: params.get("limit").and_then(|v| v.as_i64()),
        workspace: params
            .get("workspace")
            .and_then(|v| v.as_str())
            .map(String::from),
        tags: params.get("tags").and_then(|v| v.as_array()).map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect()
        }),
        memory_type: params
            .get("memory_type")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok()),
        ..Default::default()
    };

    match meili.search_memories(&query, options) {
        Ok(results) => {
            let items: Vec<Value> = results
                .iter()
                .map(|r| {
                    json!({
                        "id": r.memory.id,
                        "content": r.memory.content,
                        "memory_type": r.memory.memory_type.as_str(),
                        "tags": r.memory.tags,
                        "workspace": r.memory.workspace,
                        "score": r.score,
                        "created_at": r.memory.created_at.to_rfc3339(),
                    })
                })
                .collect();
            json!({
                "results": items,
                "count": items.len(),
                "backend": "meilisearch"
            })
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

#[cfg(feature = "meilisearch")]
pub fn meilisearch_reindex(ctx: &HandlerContext, _params: Value) -> Value {
    let indexer = match &ctx.meili_indexer {
        Some(i) => i.clone(),
        None => {
            return json!({"error": "Meilisearch indexer not configured. Start server with --meilisearch-url and --meilisearch-indexer."})
        }
    };

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");
        if let Err(e) = rt.block_on(indexer.run_full_sync()) {
            tracing::error!("Meilisearch reindex failed: {}", e);
        }
    });

    json!({
        "status": "reindex_started",
        "message": "Full re-sync from SQLite to Meilisearch started in background."
    })
}

#[cfg(feature = "meilisearch")]
pub fn meilisearch_status(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::StorageBackend;

    let meili = match &ctx.meili {
        Some(m) => m,
        None => return json!({"error": "Meilisearch not configured."}),
    };

    match meili.get_index_stats() {
        Ok(stats) => {
            let health = meili.health_check();
            json!({
                "configured": true,
                "url": meili.url(),
                "index_stats": stats,
                "healthy": health.as_ref().map(|h| h.healthy).unwrap_or(false),
                "health_error": health.as_ref().ok().and_then(|h| h.error.clone()),
            })
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

#[cfg(feature = "meilisearch")]
pub fn meilisearch_config(ctx: &HandlerContext, _params: Value) -> Value {
    match &ctx.meili {
        Some(meili) => json!({
            "configured": true,
            "url": meili.url(),
            "has_api_key": meili.has_api_key(),
            "indexer_enabled": ctx.meili_indexer.is_some(),
            "sync_interval_seconds": ctx.meili_sync_interval,
        }),
        None => json!({
            "configured": false,
            "message": "Meilisearch not configured. Use --meilisearch-url to enable."
        }),
    }
}
