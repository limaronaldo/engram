//! Miscellaneous tool handlers: stats, versions, caches, tags, export/import,
//! project scanning, document ingestion, summarization, archival, Langfuse
//! integration (feature-gated), and Meilisearch tools (feature-gated).

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

// ── Project Context / Scanning ────────────────────────────────────────────────

pub fn scan_project(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{ProjectContextConfig, ProjectContextEngine, ScanResult};
    use crate::storage::queries::{create_memory, delete_memory, list_memories, update_memory};
    use crate::types::{CreateMemoryInput, ListOptions, UpdateMemoryInput};
    use chrono::Utc;
    use std::collections::HashSet;
    use std::path::PathBuf;

    let path_str = params
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string())
        });

    let path = PathBuf::from(&path_str);
    let canonical_path = path.canonicalize().unwrap_or(path.clone());
    let canonical_path_str = canonical_path.to_string_lossy().to_string();

    let scan_parents = params
        .get("scan_parents")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let extract_sections = params
        .get("extract_sections")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let config = ProjectContextConfig {
        scan_parents,
        extract_sections,
        ..Default::default()
    };
    let engine = ProjectContextEngine::with_config(config);

    let (discovered, files_skipped) = match engine.scan_directory_with_stats(&canonical_path) {
        Ok(result) => result,
        Err(e) => return json!({"error": format!("Scan failed: {}", e)}),
    };

    let mut result = ScanResult {
        project_path: canonical_path_str.clone(),
        files_found: discovered.len(),
        memories_created: 0,
        memories_updated: 0,
        files_skipped,
        errors: Vec::new(),
        scanned_at: Utc::now(),
    };

    for file in &discovered {
        let file_path_canonical = file
            .path
            .canonicalize()
            .unwrap_or_else(|_| file.path.clone())
            .to_string_lossy()
            .to_string();

        let mut filter = std::collections::HashMap::new();
        filter.insert(
            "source_file".to_string(),
            json!(file_path_canonical.clone()),
        );

        let existing_parent = ctx.storage.with_connection(|conn| {
            let options = ListOptions {
                metadata_filter: Some(filter),
                limit: Some(1),
                ..Default::default()
            };
            let memories = list_memories(conn, &options)?;
            Ok(memories.into_iter().next())
        });

        let mut sections_to_process = Vec::new();

        if extract_sections {
            match engine.parse_file(file) {
                Ok(parsed) => {
                    for section in parsed.sections {
                        if !section.content.trim().is_empty() {
                            sections_to_process.push(section);
                        }
                    }
                }
                Err(e) => {
                    result
                        .errors
                        .push(format!("Failed to parse {}: {}", file.filename, e));
                }
            }
        }

        let parent_id = match existing_parent {
            Ok(Some(existing_memory)) => {
                let existing_hash = existing_memory
                    .metadata
                    .get("file_hash")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if existing_hash == file.content_hash {
                    existing_memory.id
                } else {
                    let memory = engine.file_to_memory(file);
                    let mut metadata = memory.metadata.clone();
                    metadata.insert(
                        "source_file".to_string(),
                        json!(file_path_canonical.clone()),
                    );
                    metadata.insert(
                        "project_path".to_string(),
                        json!(canonical_path_str.clone()),
                    );

                    let update_input = UpdateMemoryInput {
                        content: Some(memory.content),
                        memory_type: Some(memory.memory_type),
                        tags: Some(memory.tags),
                        metadata: Some(metadata),
                        importance: Some(memory.importance),
                        scope: None,
                        ttl_seconds: None,
                        event_time: None,
                        trigger_pattern: None,
                    };

                    match ctx.storage.with_transaction(|conn| {
                        update_memory(conn, existing_memory.id, &update_input)
                    }) {
                        Ok(updated) => {
                            result.memories_updated += 1;
                            updated.id
                        }
                        Err(e) => {
                            result
                                .errors
                                .push(format!("Failed to update {}: {}", file.filename, e));
                            continue;
                        }
                    }
                }
            }
            Ok(None) => {
                let memory = engine.file_to_memory(file);
                let mut metadata = memory.metadata.clone();
                metadata.insert(
                    "source_file".to_string(),
                    json!(file_path_canonical.clone()),
                );
                metadata.insert(
                    "project_path".to_string(),
                    json!(canonical_path_str.clone()),
                );

                let input = CreateMemoryInput {
                    content: memory.content,
                    memory_type: memory.memory_type,
                    tags: memory.tags,
                    metadata,
                    importance: Some(memory.importance),
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

                match ctx
                    .storage
                    .with_transaction(|conn| create_memory(conn, &input))
                {
                    Ok(created) => {
                        result.memories_created += 1;
                        created.id
                    }
                    Err(e) => {
                        result.errors.push(format!(
                            "Failed to create memory for {}: {}",
                            file.filename, e
                        ));
                        continue;
                    }
                }
            }
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to check existing: {}", e));
                continue;
            }
        };

        if extract_sections && !sections_to_process.is_empty() {
            let mut section_filter = std::collections::HashMap::new();
            section_filter.insert(
                "source_file".to_string(),
                json!(file_path_canonical.clone()),
            );

            let existing_sections = ctx
                .storage
                .with_connection(|conn| {
                    let options = ListOptions {
                        tags: Some(vec!["section".to_string()]),
                        metadata_filter: Some(section_filter),
                        limit: Some(1000),
                        ..Default::default()
                    };
                    list_memories(conn, &options)
                })
                .unwrap_or_default();

            let existing_sections_by_path: std::collections::HashMap<
                String,
                &crate::types::Memory,
            > = existing_sections
                .iter()
                .filter_map(|mem| {
                    mem.metadata
                        .get("section_path")
                        .and_then(|v| v.as_str())
                        .map(|path| (path.to_string(), mem))
                })
                .collect();

            let mut processed_section_paths: HashSet<String> = HashSet::new();

            for section in &sections_to_process {
                processed_section_paths.insert(section.section_path.clone());

                let existing_section = existing_sections_by_path.get(&section.section_path);

                if let Some(existing) = existing_section {
                    let existing_hash = existing
                        .metadata
                        .get("content_hash")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    if existing_hash == section.content_hash {
                        continue;
                    }

                    let section_memory = engine.section_to_memory(section, file, parent_id);
                    let mut metadata = section_memory.metadata.clone();
                    metadata.insert(
                        "source_file".to_string(),
                        json!(file_path_canonical.clone()),
                    );
                    metadata.insert(
                        "project_path".to_string(),
                        json!(canonical_path_str.clone()),
                    );

                    let update_input = UpdateMemoryInput {
                        content: Some(section_memory.content),
                        memory_type: Some(section_memory.memory_type),
                        tags: Some(section_memory.tags),
                        metadata: Some(metadata),
                        importance: Some(section_memory.importance),
                        scope: None,
                        ttl_seconds: None,
                        event_time: None,
                        trigger_pattern: None,
                    };

                    match ctx
                        .storage
                        .with_transaction(|conn| update_memory(conn, existing.id, &update_input))
                    {
                        Ok(_) => result.memories_updated += 1,
                        Err(e) => {
                            result.errors.push(format!(
                                "Failed to update section '{}': {}",
                                section.title, e
                            ));
                        }
                    }
                } else {
                    let section_memory = engine.section_to_memory(section, file, parent_id);
                    let mut metadata = section_memory.metadata.clone();
                    metadata.insert(
                        "source_file".to_string(),
                        json!(file_path_canonical.clone()),
                    );
                    metadata.insert(
                        "project_path".to_string(),
                        json!(canonical_path_str.clone()),
                    );

                    let input = CreateMemoryInput {
                        content: section_memory.content,
                        memory_type: section_memory.memory_type,
                        tags: section_memory.tags,
                        metadata,
                        importance: Some(section_memory.importance),
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

                    match ctx
                        .storage
                        .with_transaction(|conn| create_memory(conn, &input))
                    {
                        Ok(_) => result.memories_created += 1,
                        Err(e) => {
                            result.errors.push(format!(
                                "Failed to create section '{}': {}",
                                section.title, e
                            ));
                        }
                    }
                }
            }

            for (path, existing) in &existing_sections_by_path {
                if !processed_section_paths.contains(path) {
                    match ctx
                        .storage
                        .with_transaction(|conn| delete_memory(conn, existing.id))
                    {
                        Ok(_) => {
                            tracing::info!("Deleted stale section: {}", path);
                        }
                        Err(e) => {
                            result
                                .errors
                                .push(format!("Failed to delete stale section '{}': {}", path, e));
                        }
                    }
                }
            }
        }
    }

    json!(result)
}

pub fn get_project_context(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::list_memories;
    use crate::types::ListOptions;
    use std::path::PathBuf;

    let path_str = params
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string())
        });

    let path = PathBuf::from(&path_str);
    let canonical_path_str = path
        .canonicalize()
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let include_sections = params
        .get("include_sections")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let file_types: Option<Vec<String>> =
        params
            .get("file_types")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

    let mut filter = std::collections::HashMap::new();
    filter.insert(
        "project_path".to_string(),
        json!(canonical_path_str.clone()),
    );

    ctx.storage
        .with_connection(|conn| {
            let options = ListOptions {
                limit: Some(1000),
                tags: Some(vec!["project-context".to_string()]),
                metadata_filter: Some(filter),
                ..Default::default()
            };
            let all_memories = list_memories(conn, &options)?;

            let filtered: Vec<_> = all_memories
                .into_iter()
                .filter(|m| {
                    if !include_sections && m.tags.contains(&"section".to_string()) {
                        return false;
                    }

                    if let Some(ref types) = file_types {
                        let file_type = m
                            .metadata
                            .get("file_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if !types.iter().any(|t| t == file_type) {
                            return false;
                        }
                    }

                    true
                })
                .collect();

            Ok(json!({
                "project_path": canonical_path_str,
                "count": filtered.len(),
                "memories": filtered
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn list_instruction_files(_ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{ProjectContextConfig, ProjectContextEngine};
    use std::path::PathBuf;

    let path_str = params
        .get("path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string())
        });

    let path = PathBuf::from(&path_str);

    if !path.exists() {
        return json!({
            "error": format!("Path does not exist: {}", path_str),
            "files": []
        });
    }

    let canonical_path = path.canonicalize().unwrap_or(path.clone());
    let canonical_path_str = canonical_path.to_string_lossy().to_string();

    let scan_parents = params
        .get("scan_parents")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let config = ProjectContextConfig {
        scan_parents,
        ..Default::default()
    };
    let engine = ProjectContextEngine::with_config(config);

    match engine.scan_directory_with_stats(&canonical_path) {
        Ok((discovered, files_skipped)) => {
            let files: Vec<Value> = discovered
                .iter()
                .map(|f| {
                    json!({
                        "path": f.path.to_string_lossy(),
                        "filename": f.filename,
                        "file_type": f.file_type.as_tag(),
                        "format": f.format.as_str(),
                        "size": f.size,
                        "content_hash": f.content_hash
                    })
                })
                .collect();

            json!({
                "project_path": canonical_path_str,
                "files_found": discovered.len(),
                "files_skipped": files_skipped,
                "files": files
            })
        }
        Err(e) => json!({
            "error": format!("Scan failed: {}", e),
            "files": []
        }),
    }
}

pub fn ingest_document(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{DocumentFormat, DocumentIngestor, IngestConfig};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct IngestParams {
        path: String,
        format: Option<String>,
        chunk_size: Option<usize>,
        overlap: Option<usize>,
        max_file_size: Option<u64>,
        tags: Option<Vec<String>>,
    }

    let input: IngestParams = match serde_json::from_value(params) {
        Ok(i) => i,
        Err(e) => return json!({"error": e.to_string()}),
    };

    let format = match input.format.as_deref() {
        None | Some("auto") => None,
        Some("md") | Some("markdown") => Some(DocumentFormat::Markdown),
        Some("pdf") => Some(DocumentFormat::Pdf),
        Some(other) => {
            return json!({"error": format!("Invalid format: {}", other)});
        }
    };

    let default_config = IngestConfig::default();
    let config = IngestConfig {
        format,
        chunk_size: input.chunk_size.unwrap_or(default_config.chunk_size),
        overlap: input.overlap.unwrap_or(default_config.overlap),
        max_file_size: input.max_file_size.unwrap_or(default_config.max_file_size),
        extra_tags: input.tags.unwrap_or_default(),
    };

    let ingestor = DocumentIngestor::new(&ctx.storage);
    match ingestor.ingest_file(&input.path, config) {
        Ok(result) => json!(result),
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── Summarization & Archival ──────────────────────────────────────────────────

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
