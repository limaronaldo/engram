//! Snapshot MCP tool handlers for agent-portability.
//!
//! Provides three tools for creating, loading, and inspecting .egm snapshot
//! archives — portable knowledge packages for distributing AI memory between
//! agents and deployments.

use std::path::Path;

use serde_json::{json, Value};

use super::HandlerContext;

// ── Hex key parsing helper ────────────────────────────────────────────────────

/// Parse a hex-encoded 32-byte key string into a fixed-size array.
fn parse_hex_key(hex_str: &str) -> std::result::Result<[u8; 32], String> {
    if hex_str.len() != 64 {
        return Err(format!(
            "Key must be 64 hex characters (32 bytes), got {}",
            hex_str.len()
        ));
    }
    let bytes: Vec<u8> = (0..hex_str.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16))
        .collect::<std::result::Result<Vec<u8>, _>>()
        .map_err(|e| format!("Invalid hex: {}", e))?;
    if bytes.len() != 32 {
        return Err(format!("Key must be 32 bytes, got {}", bytes.len()));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

// ── snapshot_create ───────────────────────────────────────────────────────────

/// Create a .egm snapshot archive from the current storage.
///
/// Accepts workspace/tag/importance/type filters and optional encryption or
/// signing. Returns the snapshot manifest as JSON.
pub fn snapshot_create(ctx: &HandlerContext, params: Value) -> Value {
    use crate::snapshot::SnapshotBuilder;

    let output_path = match params.get("output_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return json!({"error": "output_path is required"}),
    };

    // Build the snapshot builder with optional filters
    let mut builder = SnapshotBuilder::new(ctx.storage.clone());

    if let Some(ws) = params.get("workspace").and_then(|v| v.as_str()) {
        builder = builder.workspace(ws);
    }

    if let Some(tags_arr) = params.get("tags").and_then(|v| v.as_array()) {
        let tags: Vec<String> = tags_arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if !tags.is_empty() {
            builder = builder.tags(tags);
        }
    }

    if let Some(min_imp) = params.get("importance_min").and_then(|v| v.as_f64()) {
        builder = builder.importance_min(min_imp as f32);
    }

    if let Some(types_arr) = params.get("memory_types").and_then(|v| v.as_array()) {
        let types: Vec<String> = types_arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if !types.is_empty() {
            builder = builder.memory_types(types);
        }
    }

    if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
        builder = builder.description(desc);
    }

    if let Some(creator) = params.get("creator").and_then(|v| v.as_str()) {
        builder = builder.creator(creator);
    }

    let path = Path::new(&output_path);

    // Parse optional keys
    let encrypt_key_str = params.get("encrypt_key").and_then(|v| v.as_str());
    let sign_key_str = params.get("sign_key").and_then(|v| v.as_str());

    let manifest_result = if let Some(hex) = encrypt_key_str {
        match parse_hex_key(hex) {
            Ok(key) => builder.build_encrypted(path, &key),
            Err(e) => return json!({"error": format!("Invalid encrypt_key: {}", e)}),
        }
    } else if let Some(hex) = sign_key_str {
        match parse_hex_key(hex) {
            Ok(key) => builder.build_signed(path, &key),
            Err(e) => return json!({"error": format!("Invalid sign_key: {}", e)}),
        }
    } else {
        builder.build(path)
    };

    match manifest_result {
        Ok(manifest) => json!({
            "output_path": output_path,
            "format_version": manifest.format_version,
            "engram_version": manifest.engram_version,
            "schema_version": manifest.schema_version,
            "memory_count": manifest.memory_count,
            "entity_count": manifest.entity_count,
            "edge_count": manifest.edge_count,
            "encrypted": manifest.encrypted,
            "signed": manifest.signed,
            "created_at": manifest.created_at.to_rfc3339(),
            "content_hash": manifest.content_hash,
            "creator": manifest.creator,
            "description": manifest.description,
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── snapshot_load ─────────────────────────────────────────────────────────────

/// Load a .egm snapshot archive into storage.
///
/// Accepts a load strategy ("merge", "replace", "isolate", or "dry_run"),
/// an optional target workspace override, and an optional decryption key.
/// Returns a LoadResult describing what was inserted.
pub fn snapshot_load(ctx: &HandlerContext, params: Value) -> Value {
    use crate::snapshot::{LoadStrategy, SnapshotLoader};
    use std::str::FromStr;

    let path_str = match params.get("path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return json!({"error": "path is required"}),
    };

    let strategy_str = match params.get("strategy").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "strategy is required"}),
    };

    let strategy = match LoadStrategy::from_str(&strategy_str) {
        Ok(s) => s,
        Err(e) => return json!({"error": format!("Invalid strategy: {}", e)}),
    };

    let target_workspace = params
        .get("target_workspace")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let decrypt_key_bytes: Option<[u8; 32]> = match params
        .get("decrypt_key")
        .and_then(|v| v.as_str())
    {
        Some(hex) => match parse_hex_key(hex) {
            Ok(key) => Some(key),
            Err(e) => return json!({"error": format!("Invalid decrypt_key: {}", e)}),
        },
        None => None,
    };

    let path = Path::new(&path_str);
    let result = SnapshotLoader::load(
        &ctx.storage,
        path,
        strategy,
        target_workspace.as_deref(),
        decrypt_key_bytes.as_ref(),
    );

    match result {
        Ok(load_result) => {
            // Phase L: log attestation for the loaded snapshot manifest (best-effort).
            // Use the raw snapshot archive bytes as document content so the hash
            // matches what an external verifier would compute over the .egm file.
            {
                use crate::attestation::AttestationChain;
                let chain = AttestationChain::new(ctx.storage.clone());
                let snapshot_name = Path::new(&path_str)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path_str.clone());
                // Read the archive bytes for attestation (best-effort).
                if let Ok(archive_bytes) = std::fs::read(path) {
                    if let Err(e) =
                        chain.log_document(&archive_bytes, &snapshot_name, None, &[], None)
                    {
                        tracing::warn!(
                            "Attestation hook (snapshot_load): failed to log '{}': {}",
                            snapshot_name,
                            e
                        );
                    }
                }
            }
            json!({
                "strategy": load_result.strategy.to_string(),
                "memories_loaded": load_result.memories_loaded,
                "memories_skipped": load_result.memories_skipped,
                "entities_loaded": load_result.entities_loaded,
                "edges_loaded": load_result.edges_loaded,
                "target_workspace": load_result.target_workspace,
                "snapshot_origin": load_result.snapshot_origin,
            })
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── snapshot_inspect ──────────────────────────────────────────────────────────

/// Inspect a .egm snapshot archive and return metadata without loading.
///
/// Returns manifest information, file list, and file size.
pub fn snapshot_inspect(_ctx: &HandlerContext, params: Value) -> Value {
    use crate::snapshot::SnapshotLoader;

    let path_str = match params.get("path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return json!({"error": "path is required"}),
    };

    let path = Path::new(&path_str);

    match SnapshotLoader::inspect(path) {
        Ok(info) => {
            let manifest = &info.manifest;
            json!({
                "file_size_bytes": info.file_size_bytes,
                "files": info.files,
                "manifest": {
                    "format_version": manifest.format_version,
                    "engram_version": manifest.engram_version,
                    "min_engram_version": manifest.min_engram_version,
                    "schema_version": manifest.schema_version,
                    "creator": manifest.creator,
                    "description": manifest.description,
                    "created_at": manifest.created_at.to_rfc3339(),
                    "content_hash": manifest.content_hash,
                    "memory_count": manifest.memory_count,
                    "entity_count": manifest.entity_count,
                    "edge_count": manifest.edge_count,
                    "embedding_model": manifest.embedding_model,
                    "embedding_dimensions": manifest.embedding_dimensions,
                    "encrypted": manifest.encrypted,
                    "signed": manifest.signed,
                }
            })
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}
