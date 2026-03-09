//! Snapshot loader — loads .egm archives into storage with configurable strategies

use std::io::Read;
use std::path::Path;

use chrono::Utc;

use crate::error::{EngramError, Result};
use crate::storage::Storage;
use crate::types::{CreateMemoryInput, DedupMode, Memory, MemoryScope, MemoryTier};

use super::builder::SnapshotEdge;
use super::crypto::decrypt_aes256;
use super::types::{LoadResult, LoadStrategy, SnapshotInfo, SnapshotManifest};

/// Loads .egm snapshot archives into storage
pub struct SnapshotLoader;

impl SnapshotLoader {
    /// Inspect a snapshot file and return metadata without loading any memories.
    pub fn inspect(path: &Path) -> Result<SnapshotInfo> {
        let file_size_bytes = std::fs::metadata(path)?.len();

        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;

        let files: Vec<String> = (0..archive.len())
            .map(|i| {
                archive
                    .by_index(i)
                    .map(|f| f.name().to_string())
                    .unwrap_or_default()
            })
            .collect();

        let manifest = Self::read_manifest(&mut archive)?;

        Ok(SnapshotInfo {
            manifest,
            file_size_bytes,
            files,
        })
    }

    /// Load a snapshot into storage using the specified strategy.
    ///
    /// - `strategy`: How to handle conflicts and existing data
    /// - `target_workspace`: Override for the workspace (None = use original workspace)
    /// - `decrypt_key`: Decryption key for encrypted snapshots
    pub fn load(
        storage: &Storage,
        path: &Path,
        strategy: LoadStrategy,
        target_workspace: Option<&str>,
        decrypt_key: Option<&[u8; 32]>,
    ) -> Result<LoadResult> {
        let snapshot_origin = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.egm")
            .to_string();

        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;

        let manifest = Self::read_manifest(&mut archive)?;

        // Load content (plaintext or decrypted)
        let (memories, edges) = if manifest.encrypted {
            let key = decrypt_key.ok_or_else(|| {
                EngramError::Encryption(
                    "Snapshot is encrypted but no decryption key was provided".to_string(),
                )
            })?;
            Self::read_encrypted_content(&mut archive, key)?
        } else {
            Self::read_plaintext_content(&mut archive)?
        };

        // Determine the workspace name
        let resolved_workspace = Self::resolve_workspace(
            strategy,
            target_workspace,
            &manifest,
            &memories,
        );

        // DryRun: return counts without making changes
        if strategy == LoadStrategy::DryRun {
            return Self::dry_run(storage, &memories, &resolved_workspace);
        }

        // Replace: clear workspace first
        if strategy == LoadStrategy::Replace {
            Self::clear_workspace(storage, &resolved_workspace)?;
        }

        let now_str = Utc::now().to_rfc3339();

        // Insert memories
        let mut memories_loaded = 0usize;
        let mut memories_skipped = 0usize;

        // Collect IDs of newly inserted memories (original_id -> new_id) for edge remapping
        let mut id_map: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();

        for memory in &memories {
            let ws = resolved_workspace.clone();

            // Merge: skip if content_hash already exists in target workspace
            if strategy == LoadStrategy::Merge {
                if let Some(hash) = &memory.content_hash {
                    let exists = Self::content_hash_exists(storage, hash, &ws)?;
                    if exists {
                        memories_skipped += 1;
                        continue;
                    }
                }
            }

            let input = CreateMemoryInput {
                content: memory.content.clone(),
                memory_type: memory.memory_type,
                tags: memory.tags.clone(),
                metadata: memory.metadata.clone(),
                importance: Some(memory.importance),
                scope: MemoryScope::Global,
                workspace: Some(ws),
                tier: MemoryTier::Permanent,
                defer_embedding: true,
                ttl_seconds: None,
                dedup_mode: DedupMode::Allow,
                dedup_threshold: None,
                event_time: memory.event_time,
                event_duration_seconds: memory.event_duration_seconds,
                trigger_pattern: memory.trigger_pattern.clone(),
                summary_of_id: None,
            };

            let new_memory = storage.with_transaction(|conn| {
                use crate::storage::queries::create_memory;
                let m = create_memory(conn, &input)?;

                // Set snapshot provenance columns
                conn.execute(
                    "UPDATE memories SET snapshot_origin = ?, snapshot_loaded_at = ? WHERE id = ?",
                    rusqlite::params![snapshot_origin, now_str, m.id],
                )?;

                Ok(m)
            })?;

            id_map.insert(memory.id, new_memory.id);
            memories_loaded += 1;
        }

        // Insert graph edges (remapping old IDs to new IDs)
        let mut edges_loaded = 0usize;
        for edge in &edges {
            let new_from = match id_map.get(&edge.from_id) {
                Some(id) => *id,
                None => continue,
            };
            let new_to = match id_map.get(&edge.to_id) {
                Some(id) => *id,
                None => continue,
            };

            let edge_type_str = &edge.edge_type;
            let inserted = storage.with_connection(|conn| {
                let now_edge = Utc::now().to_rfc3339();
                let result = conn.execute(
                    "INSERT OR IGNORE INTO cross_references
                         (from_id, to_id, relation_type, score, confidence, strength,
                          source, source_context, created_at, valid_from)
                     VALUES (?, ?, ?, ?, ?, ?, 'snapshot', ?, ?, ?)",
                    rusqlite::params![
                        new_from,
                        new_to,
                        edge_type_str,
                        edge.score,
                        edge.confidence,
                        edge.strength,
                        edge.source_context,
                        now_edge,
                        now_edge,
                    ],
                );
                match result {
                    Ok(n) => Ok(n > 0),
                    // Table may not exist — skip silently
                    Err(_) => Ok(false),
                }
            })?;

            if inserted {
                edges_loaded += 1;
            }
        }

        Ok(LoadResult {
            strategy,
            memories_loaded,
            memories_skipped,
            entities_loaded: 0, // Entity re-insertion requires full extraction pipeline
            edges_loaded,
            target_workspace: resolved_workspace,
            snapshot_origin,
        })
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Read and parse the manifest from an open archive
    fn read_manifest(archive: &mut zip::ZipArchive<std::fs::File>) -> Result<SnapshotManifest> {
        let mut entry = archive.by_name("manifest.json").map_err(|_| {
            EngramError::Storage("Snapshot archive missing manifest.json".to_string())
        })?;

        let mut json = String::new();
        entry.read_to_string(&mut json)?;

        serde_json::from_str(&json).map_err(|e| {
            EngramError::Storage(format!("Failed to parse snapshot manifest: {}", e))
        })
    }

    /// Read memories and edges from a plaintext archive
    fn read_plaintext_content(
        archive: &mut zip::ZipArchive<std::fs::File>,
    ) -> Result<(Vec<Memory>, Vec<SnapshotEdge>)> {
        let memories = Self::read_json_file(archive, "memories.json")?;
        let edges = Self::read_json_file(archive, "graph_edges.json").unwrap_or_default();
        Ok((memories, edges))
    }

    /// Read memories and edges from an encrypted archive (`payload.enc`)
    fn read_encrypted_content(
        archive: &mut zip::ZipArchive<std::fs::File>,
        key: &[u8; 32],
    ) -> Result<(Vec<Memory>, Vec<SnapshotEdge>)> {
        let mut entry = archive.by_name("payload.enc").map_err(|_| {
            EngramError::Storage("Encrypted snapshot missing payload.enc".to_string())
        })?;

        let mut ciphertext = Vec::new();
        entry.read_to_end(&mut ciphertext)?;

        let plaintext = decrypt_aes256(&ciphertext, key)?;

        // The plaintext is itself a ZIP archive
        let cursor = std::io::Cursor::new(plaintext);
        let mut inner = zip::ZipArchive::new(cursor).map_err(|e| {
            EngramError::Encryption(format!("Failed to open decrypted inner archive: {}", e))
        })?;

        let memories: Vec<Memory> = Self::read_json_from_inner(&mut inner, "memories.json")?;
        let edges: Vec<SnapshotEdge> =
            Self::read_json_from_inner(&mut inner, "graph_edges.json").unwrap_or_default();

        Ok((memories, edges))
    }

    /// Read and deserialize a JSON file from an open archive by name
    fn read_json_file<T: serde::de::DeserializeOwned>(
        archive: &mut zip::ZipArchive<std::fs::File>,
        name: &str,
    ) -> Result<T> {
        let mut entry = archive
            .by_name(name)
            .map_err(|_| EngramError::Storage(format!("Snapshot archive missing {}", name)))?;

        let mut json = String::new();
        entry.read_to_string(&mut json)?;

        serde_json::from_str(&json).map_err(|e| {
            EngramError::Storage(format!("Failed to parse {}: {}", name, e))
        })
    }

    /// Read and deserialize a JSON file from an inner in-memory archive
    fn read_json_from_inner<T: serde::de::DeserializeOwned>(
        archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
        name: &str,
    ) -> Result<T> {
        let mut entry = archive
            .by_name(name)
            .map_err(|_| EngramError::Storage(format!("Inner archive missing {}", name)))?;

        let mut json = String::new();
        entry.read_to_string(&mut json)?;

        serde_json::from_str(&json).map_err(|e| {
            EngramError::Storage(format!("Failed to parse {}: {}", name, e))
        })
    }

    /// Determine the workspace name to use based on strategy and inputs
    fn resolve_workspace(
        strategy: LoadStrategy,
        target_workspace: Option<&str>,
        manifest: &SnapshotManifest,
        memories: &[Memory],
    ) -> String {
        if strategy == LoadStrategy::Isolate {
            // Generate a unique workspace from the snapshot timestamp
            let ts = manifest.created_at.format("%Y%m%d%H%M%S").to_string();
            let base = memories
                .first()
                .map(|m| m.workspace.clone())
                .unwrap_or_else(|| "snapshot".to_string());
            format!("{}-snapshot-{}", base, ts)
        } else if let Some(ws) = target_workspace {
            ws.to_string()
        } else {
            // Use the workspace of the first memory, or "default"
            memories
                .first()
                .map(|m| m.workspace.clone())
                .unwrap_or_else(|| "default".to_string())
        }
    }

    /// Check whether a content_hash already exists in a workspace
    fn content_hash_exists(storage: &Storage, hash: &str, workspace: &str) -> Result<bool> {
        storage.with_connection(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memories WHERE content_hash = ? AND workspace = ? AND valid_to IS NULL",
                rusqlite::params![hash, workspace],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
    }

    /// Delete all memories in a workspace (for Replace strategy)
    fn clear_workspace(storage: &Storage, workspace: &str) -> Result<()> {
        storage.with_transaction(|conn| {
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE memories SET valid_to = ? WHERE workspace = ? AND valid_to IS NULL",
                rusqlite::params![now, workspace],
            )?;
            Ok(())
        })
    }

    /// Compute DryRun result without modifying storage
    fn dry_run(storage: &Storage, memories: &[Memory], workspace: &str) -> Result<LoadResult> {
        let mut would_load = 0usize;
        let mut would_skip = 0usize;

        for memory in memories {
            if let Some(hash) = &memory.content_hash {
                let exists = Self::content_hash_exists(storage, hash, workspace)?;
                if exists {
                    would_skip += 1;
                } else {
                    would_load += 1;
                }
            } else {
                would_load += 1;
            }
        }

        Ok(LoadResult {
            strategy: LoadStrategy::DryRun,
            memories_loaded: would_load,
            memories_skipped: would_skip,
            entities_loaded: 0,
            edges_loaded: 0,
            target_workspace: workspace.to_string(),
            snapshot_origin: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use crate::snapshot::builder::SnapshotBuilder;
    use crate::storage::queries::create_memory;
    use crate::types::{CreateMemoryInput, DedupMode, MemoryScope, MemoryTier, MemoryType};
    use tempfile::tempdir;

    fn make_storage() -> Storage {
        Storage::open_in_memory().expect("in-memory storage")
    }

    fn insert_test_memory(storage: &Storage, content: &str, workspace: &str) {
        storage
            .with_transaction(|conn| {
                let input = CreateMemoryInput {
                    content: content.to_string(),
                    memory_type: MemoryType::Note,
                    tags: vec!["test".to_string()],
                    metadata: Default::default(),
                    importance: Some(0.7),
                    scope: MemoryScope::Global,
                    workspace: Some(workspace.to_string()),
                    tier: MemoryTier::Permanent,
                    defer_embedding: false,
                    ttl_seconds: None,
                    dedup_mode: DedupMode::Allow,
                    dedup_threshold: None,
                    event_time: None,
                    event_duration_seconds: None,
                    trigger_pattern: None,
                    summary_of_id: None,
                };
                create_memory(conn, &input)?;
                Ok(())
            })
            .expect("insert");
    }

    #[test]
    fn test_build_and_inspect() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.egm");

        let storage = make_storage();
        insert_test_memory(&storage, "Hello snapshot world", "test-ws");
        insert_test_memory(&storage, "Second memory entry", "test-ws");

        let manifest = SnapshotBuilder::new(storage)
            .workspace("test-ws")
            .description("Unit test snapshot")
            .build(&path)
            .expect("build");

        assert_eq!(manifest.memory_count, 2);
        assert!(!manifest.encrypted);
        assert!(!manifest.signed);

        let info = SnapshotLoader::inspect(&path).expect("inspect");
        assert_eq!(info.manifest.memory_count, 2);
        assert!(info.file_size_bytes > 0);
        assert!(info.files.contains(&"manifest.json".to_string()));
        assert!(info.files.contains(&"memories.json".to_string()));
    }

    #[test]
    fn test_load_merge_strategy() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("merge.egm");

        let src = make_storage();
        insert_test_memory(&src, "Mergeable memory", "src-ws");

        SnapshotBuilder::new(src)
            .workspace("src-ws")
            .build(&path)
            .expect("build");

        let dst = make_storage();
        let result = SnapshotLoader::load(&dst, &path, LoadStrategy::Merge, Some("dst-ws"), None)
            .expect("load");

        assert_eq!(result.memories_loaded, 1);
        assert_eq!(result.memories_skipped, 0);
        assert_eq!(result.target_workspace, "dst-ws");
    }

    #[test]
    fn test_load_dry_run() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("dryrun.egm");

        let src = make_storage();
        insert_test_memory(&src, "DryRun memory A", "default");
        insert_test_memory(&src, "DryRun memory B", "default");

        SnapshotBuilder::new(src).build(&path).expect("build");

        let dst = make_storage();
        let result =
            SnapshotLoader::load(&dst, &path, LoadStrategy::DryRun, None, None).expect("load");

        assert_eq!(result.strategy, LoadStrategy::DryRun);
        assert_eq!(result.memories_loaded, 2);
        assert_eq!(result.memories_skipped, 0);
    }

    #[test]
    fn test_encrypted_roundtrip() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("encrypted.egm");

        let key = [0xABu8; 32];

        let src = make_storage();
        insert_test_memory(&src, "Top secret memory", "secret-ws");

        SnapshotBuilder::new(src)
            .workspace("secret-ws")
            .build_encrypted(&path, &key)
            .expect("build_encrypted");

        let dst = make_storage();
        let result =
            SnapshotLoader::load(&dst, &path, LoadStrategy::Merge, Some("loaded-ws"), Some(&key))
                .expect("load encrypted");

        assert_eq!(result.memories_loaded, 1);
    }

    #[test]
    fn test_encrypted_wrong_key_fails() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("encrypted_bad.egm");

        let key = [0x11u8; 32];
        let wrong_key = [0x22u8; 32];

        let src = make_storage();
        insert_test_memory(&src, "Encrypted content", "ws");

        SnapshotBuilder::new(src)
            .build_encrypted(&path, &key)
            .expect("build_encrypted");

        let dst = make_storage();
        let result = SnapshotLoader::load(&dst, &path, LoadStrategy::Merge, None, Some(&wrong_key));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_replace_strategy() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("replace.egm");

        // Build snapshot with one memory
        let src = make_storage();
        insert_test_memory(&src, "Replace source memory", "replace-ws");
        SnapshotBuilder::new(src)
            .workspace("replace-ws")
            .build(&path)
            .expect("build");

        // Destination already has a memory in the same workspace
        let dst = make_storage();
        insert_test_memory(&dst, "Pre-existing memory", "replace-ws");

        let result = SnapshotLoader::load(&dst, &path, LoadStrategy::Replace, Some("replace-ws"), None)
            .expect("load replace");

        assert_eq!(result.strategy, LoadStrategy::Replace);
        // The new memory from the snapshot should be loaded
        assert_eq!(result.memories_loaded, 1);
        assert_eq!(result.target_workspace, "replace-ws");

        // Verify old memory was cleared (soft-deleted) and new one exists
        dst.with_connection(|conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memories WHERE workspace = ? AND valid_to IS NULL",
                rusqlite::params!["replace-ws"],
                |row| row.get(0),
            )?;
            // Only the snapshot memory should be active
            assert_eq!(count, 1);
            Ok(())
        }).expect("count query");
    }

    #[test]
    fn test_load_isolate_strategy() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("isolate.egm");

        // Build snapshot
        let src = make_storage();
        insert_test_memory(&src, "Isolated memory A", "source-ws");
        insert_test_memory(&src, "Isolated memory B", "source-ws");
        SnapshotBuilder::new(src)
            .workspace("source-ws")
            .build(&path)
            .expect("build");

        let dst = make_storage();
        let result = SnapshotLoader::load(&dst, &path, LoadStrategy::Isolate, None, None)
            .expect("load isolate");

        assert_eq!(result.strategy, LoadStrategy::Isolate);
        assert_eq!(result.memories_loaded, 2);
        // Isolate creates a new workspace name — it should not be "source-ws"
        assert_ne!(result.target_workspace, "source-ws");
        // It should contain "snapshot" in the generated name
        assert!(result.target_workspace.contains("snapshot"));
    }

    #[test]
    fn test_signing_and_verification() {
        use crate::snapshot::crypto::{public_key_from_secret, verify_ed25519};

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("signed.egm");
        let secret_key = [0x55u8; 32];
        let public_key = public_key_from_secret(&secret_key);

        let src = make_storage();
        insert_test_memory(&src, "Signed memory content", "signed-ws");

        let manifest = SnapshotBuilder::new(src)
            .workspace("signed-ws")
            .description("Signed snapshot test")
            .build_signed(&path, &secret_key)
            .expect("build_signed");

        assert!(manifest.signed);
        assert!(!manifest.encrypted);

        // Extract signature from archive and verify it
        let file = std::fs::File::open(&path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();

        // Read manifest JSON
        let manifest_json = {
            let mut entry = archive.by_name("manifest.json").unwrap();
            let mut s = String::new();
            std::io::Read::read_to_string(&mut entry, &mut s).unwrap();
            s
        };

        // Read signature
        let sig_hex = {
            let mut entry = archive.by_name("manifest.sig").unwrap();
            let mut s = String::new();
            std::io::Read::read_to_string(&mut entry, &mut s).unwrap();
            s
        };

        let sig_bytes = hex::decode(&sig_hex).expect("decode hex sig");
        let valid = verify_ed25519(manifest_json.as_bytes(), &sig_bytes, &public_key)
            .expect("verify_ed25519");
        assert!(valid, "signature should be valid");

        // Tamper: verification of different data should fail
        let tampered = format!("{}tampered", manifest_json);
        let invalid = verify_ed25519(tampered.as_bytes(), &sig_bytes, &public_key)
            .expect("verify_ed25519 tampered");
        assert!(!invalid, "tampered data should not verify");
    }
}
