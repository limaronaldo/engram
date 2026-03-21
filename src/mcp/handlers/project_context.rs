//! Project context scanning and instruction file discovery handlers.

use serde_json::{json, Value};

use super::HandlerContext;

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
                        media_url: None,
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
                    media_url: None,
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
                        media_url: None,
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
                        media_url: None,
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

    // Phase L: best-effort attestation for each discovered file (agent-portability feature).
    // Errors are logged but never propagated — attestation is a non-blocking enhancement.
    #[cfg(feature = "agent-portability")]
    {
        use crate::attestation::AttestationChain;
        let chain = AttestationChain::new(ctx.storage.clone());
        for file in &discovered {
            let content_bytes = file.content.as_bytes();
            if let Err(e) =
                chain.log_document(content_bytes, &file.filename, None, &[], None)
            {
                tracing::warn!(
                    "Attestation hook (scan_project): failed to log '{}': {}",
                    file.filename,
                    e
                );
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
