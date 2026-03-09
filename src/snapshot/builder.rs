//! Snapshot builder — creates .egm portable knowledge package archives

use std::io::Write;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{EngramError, Result};
use crate::intelligence::Entity;
use crate::storage::Storage;
use crate::types::{CrossReference, ListOptions, Memory};

use super::crypto::{encrypt_aes256, sign_ed25519};
use super::types::SnapshotManifest;

// =============================================================================
// Serializable edge for archive storage
// =============================================================================

/// A simplified graph edge stored inside a .egm archive
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEdge {
    pub from_id: i64,
    pub to_id: i64,
    pub edge_type: String,
    pub score: f32,
    pub confidence: f32,
    pub strength: f32,
    pub source_context: Option<String>,
    pub created_at: String,
}

impl From<&CrossReference> for SnapshotEdge {
    fn from(cr: &CrossReference) -> Self {
        Self {
            from_id: cr.from_id,
            to_id: cr.to_id,
            edge_type: cr.edge_type.as_str().to_string(),
            score: cr.score,
            confidence: cr.confidence,
            strength: cr.strength,
            source_context: cr.source_context.clone(),
            created_at: cr.created_at.to_rfc3339(),
        }
    }
}

// =============================================================================
// SnapshotBuilder
// =============================================================================

/// Builds a .egm snapshot archive from live storage
pub struct SnapshotBuilder {
    storage: Storage,
    workspace: Option<String>,
    tags: Option<Vec<String>>,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
    importance_min: Option<f32>,
    memory_types: Option<Vec<String>>,
    description: Option<String>,
    creator: Option<String>,
}

impl SnapshotBuilder {
    /// Create a new builder backed by the given storage
    pub fn new(storage: Storage) -> Self {
        Self {
            storage,
            workspace: None,
            tags: None,
            start_date: None,
            end_date: None,
            importance_min: None,
            memory_types: None,
            description: None,
            creator: None,
        }
    }

    /// Filter memories to a specific workspace
    pub fn workspace(mut self, ws: impl Into<String>) -> Self {
        self.workspace = Some(ws.into());
        self
    }

    /// Filter memories by tags (any matching tag is included)
    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    /// Filter memories created on or after this date
    pub fn start_date(mut self, dt: DateTime<Utc>) -> Self {
        self.start_date = Some(dt);
        self
    }

    /// Filter memories created on or before this date
    pub fn end_date(mut self, dt: DateTime<Utc>) -> Self {
        self.end_date = Some(dt);
        self
    }

    /// Filter memories with at least this importance score
    pub fn importance_min(mut self, min: f32) -> Self {
        self.importance_min = Some(min);
        self
    }

    /// Filter memories to specific type strings (e.g., "note", "decision")
    pub fn memory_types(mut self, types: Vec<String>) -> Self {
        self.memory_types = Some(types);
        self
    }

    /// Human-readable description embedded in the manifest
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Name of the agent or user creating this snapshot
    pub fn creator(mut self, creator: impl Into<String>) -> Self {
        self.creator = Some(creator.into());
        self
    }

    // -------------------------------------------------------------------------
    // Core build logic
    // -------------------------------------------------------------------------

    /// Query memories from storage, applying all filters
    fn query_memories(&self) -> Result<Vec<Memory>> {
        self.storage.with_connection(|conn| {
            use crate::storage::queries::list_memories;

            let options = ListOptions {
                limit: Some(100_000),
                workspace: self.workspace.clone(),
                tags: self.tags.clone(),
                ..Default::default()
            };

            let mut memories = list_memories(conn, &options)?;

            // Apply in-memory filters that list_memories doesn't support directly
            if let Some(start) = self.start_date {
                memories.retain(|m| m.created_at >= start);
            }
            if let Some(end) = self.end_date {
                memories.retain(|m| m.created_at <= end);
            }
            if let Some(min_imp) = self.importance_min {
                memories.retain(|m| m.importance >= min_imp);
            }
            if let Some(ref types) = self.memory_types {
                memories.retain(|m| types.contains(&m.memory_type.as_str().to_string()));
            }

            Ok(memories)
        })
    }

    /// Query graph edges between the given memory IDs
    fn query_edges(&self, memory_ids: &[i64]) -> Result<Vec<CrossReference>> {
        if memory_ids.is_empty() {
            return Ok(Vec::new());
        }

        self.storage.with_connection(|conn| {
            // Build the IN clause
            let placeholders: Vec<String> = memory_ids.iter().map(|_| "?".to_string()).collect();
            let in_clause = placeholders.join(", ");

            let sql = format!(
                "SELECT cr.from_id, cr.to_id, cr.relation_type, cr.score,
                        cr.confidence, cr.strength, cr.source_context, cr.created_at,
                        cr.valid_from, cr.pinned, cr.metadata
                 FROM cross_references cr
                 WHERE cr.from_id IN ({in_clause})
                   AND cr.to_id IN ({in_clause})
                   AND cr.valid_to IS NULL",
                in_clause = in_clause,
            );

            let mut stmt = match conn.prepare(&sql) {
                Ok(s) => s,
                // cross_references table may not exist yet — return empty
                Err(_) => return Ok(Vec::new()),
            };

            let params: Vec<&dyn rusqlite::ToSql> = memory_ids
                .iter()
                .chain(memory_ids.iter())
                .map(|id| id as &dyn rusqlite::ToSql)
                .collect();

            let now = Utc::now();

            let edges: Vec<CrossReference> = stmt
                .query_map(params.as_slice(), |row| {
                    let from_id: i64 = row.get(0)?;
                    let to_id: i64 = row.get(1)?;
                    let relation_type: String = row.get(2)?;
                    let score: f32 = row.get(3)?;
                    let confidence: f32 = row.get(4)?;
                    let strength: f32 = row.get(5)?;
                    let source_context: Option<String> = row.get(6)?;
                    let created_at_str: String = row.get(7)?;
                    let valid_from_str: String = row.get(8)?;
                    let pinned: bool = row.get(9)?;
                    let metadata_str: Option<String> = row.get(10)?;

                    Ok((
                        from_id,
                        to_id,
                        relation_type,
                        score,
                        confidence,
                        strength,
                        source_context,
                        created_at_str,
                        valid_from_str,
                        pinned,
                        metadata_str,
                    ))
                })
                .map_err(EngramError::Database)?
                .filter_map(|r| r.ok())
                .map(
                    |(
                        from_id,
                        to_id,
                        relation_type,
                        score,
                        confidence,
                        strength,
                        source_context,
                        created_at_str,
                        valid_from_str,
                        pinned,
                        metadata_str,
                    )| {
                        let edge_type = relation_type.parse().unwrap_or_default();
                        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or(now);
                        let valid_from = DateTime::parse_from_rfc3339(&valid_from_str)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or(now);
                        let metadata = metadata_str
                            .and_then(|s| serde_json::from_str(&s).ok())
                            .unwrap_or_default();

                        CrossReference {
                            from_id,
                            to_id,
                            edge_type,
                            score,
                            confidence,
                            strength,
                            source: crate::types::RelationSource::Manual,
                            source_context,
                            created_at,
                            valid_from,
                            valid_to: None,
                            pinned,
                            metadata,
                        }
                    },
                )
                .collect();

            Ok(edges)
        })
    }

    /// Query entities that are linked to any of the given memory IDs
    fn query_entities(&self, memory_ids: &[i64]) -> Result<Vec<Entity>> {
        if memory_ids.is_empty() {
            return Ok(Vec::new());
        }

        self.storage.with_connection(|conn| {
            let placeholders: Vec<String> = memory_ids.iter().map(|_| "?".to_string()).collect();
            let in_clause = placeholders.join(", ");

            let sql = format!(
                "SELECT DISTINCT e.id, e.name, e.normalized_name, e.entity_type,
                        e.aliases, e.metadata, e.created_at, e.updated_at, e.mention_count
                 FROM entities e
                 JOIN memory_entities me ON e.id = me.entity_id
                 WHERE me.memory_id IN ({in_clause})",
                in_clause = in_clause,
            );

            let mut stmt = match conn.prepare(&sql) {
                Ok(s) => s,
                Err(_) => return Ok(Vec::new()),
            };

            let params: Vec<&dyn rusqlite::ToSql> = memory_ids
                .iter()
                .map(|id| id as &dyn rusqlite::ToSql)
                .collect();

            use crate::intelligence::{EntityType, Entity};
            use std::collections::HashMap;

            let entities: Vec<Entity> = stmt
                .query_map(params.as_slice(), |row| {
                    let id: i64 = row.get(0)?;
                    let name: String = row.get(1)?;
                    let normalized_name: String = row.get(2)?;
                    let entity_type_str: String = row.get(3)?;
                    let aliases_str: String = row.get(4)?;
                    let metadata_str: String = row.get(5)?;
                    let created_at_str: String = row.get(6)?;
                    let updated_at_str: String = row.get(7)?;
                    let mention_count: i32 = row.get(8)?;
                    Ok((
                        id,
                        name,
                        normalized_name,
                        entity_type_str,
                        aliases_str,
                        metadata_str,
                        created_at_str,
                        updated_at_str,
                        mention_count,
                    ))
                })
                .map_err(EngramError::Database)?
                .filter_map(|r| r.ok())
                .map(
                    |(
                        id,
                        name,
                        normalized_name,
                        entity_type_str,
                        aliases_str,
                        metadata_str,
                        created_at_str,
                        updated_at_str,
                        mention_count,
                    )| {
                        let now_dt = Utc::now();
                        let entity_type: EntityType =
                            entity_type_str.parse().unwrap_or(EntityType::Other);
                        let aliases: Vec<String> =
                            serde_json::from_str(&aliases_str).unwrap_or_default();
                        let metadata: HashMap<String, serde_json::Value> =
                            serde_json::from_str(&metadata_str).unwrap_or_default();
                        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or(now_dt);
                        let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or(now_dt);

                        Entity {
                            id,
                            name,
                            normalized_name,
                            entity_type,
                            aliases,
                            metadata,
                            created_at,
                            updated_at,
                            mention_count,
                        }
                    },
                )
                .collect();

            Ok(entities)
        })
    }

    /// Compute SHA-256 of all memory contents concatenated
    fn compute_content_hash(memories: &[Memory]) -> String {
        let mut hasher = Sha256::new();
        for m in memories {
            hasher.update(m.content.as_bytes());
        }
        hex::encode(hasher.finalize())
    }

    /// Generate README.md content for the archive
    fn generate_readme(manifest: &SnapshotManifest) -> String {
        format!(
            "# Engram Snapshot (.egm)\n\n\
             **Format Version:** {}\n\
             **Created By Engram:** {}\n\
             **Created At:** {}\n\
             **Schema Version:** v{}\n\
             **Memories:** {}\n\
             **Entities:** {}\n\
             **Graph Edges:** {}\n\n\
             ## Description\n\n\
             {}\n\n\
             ## Contents\n\n\
             - `manifest.json` — Snapshot metadata and integrity hash\n\
             - `memories.json` — All memory records\n\
             - `entities.json` — Named entities extracted from memories\n\
             - `graph_edges.json` — Typed relationships between memories\n\
             - `README.md` — This file\n\n\
             ## Loading\n\n\
             ```bash\n\
             # Via engram-cli\n\
             engram-cli snapshot load path/to/snapshot.egm --strategy merge\n\
             ```\n",
            manifest.format_version,
            manifest.engram_version,
            manifest.created_at.to_rfc3339(),
            manifest.schema_version,
            manifest.memory_count,
            manifest.entity_count,
            manifest.edge_count,
            manifest
                .description
                .as_deref()
                .unwrap_or("No description provided."),
        )
    }

    /// Write the ZIP archive to `output_path` and return the manifest
    fn write_archive(
        output_path: &Path,
        manifest: &SnapshotManifest,
        memories: &[Memory],
        entities: &[Entity],
        edges: &[SnapshotEdge],
    ) -> Result<()> {
        let file = std::fs::File::create(output_path)?;
        let mut zip = zip::ZipWriter::new(file);

        let options =
            zip::write::FileOptions::<()>::default().compression_method(zip::CompressionMethod::Deflated);

        // manifest.json
        zip.start_file("manifest.json", options)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        let manifest_json = serde_json::to_string_pretty(manifest)?;
        zip.write_all(manifest_json.as_bytes())?;

        // memories.json
        zip.start_file("memories.json", options)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        let memories_json = serde_json::to_string_pretty(memories)?;
        zip.write_all(memories_json.as_bytes())?;

        // entities.json
        zip.start_file("entities.json", options)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        let entities_json = serde_json::to_string_pretty(entities)?;
        zip.write_all(entities_json.as_bytes())?;

        // graph_edges.json
        zip.start_file("graph_edges.json", options)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        let edges_json = serde_json::to_string_pretty(edges)?;
        zip.write_all(edges_json.as_bytes())?;

        // README.md
        zip.start_file("README.md", options)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        let readme = Self::generate_readme(manifest);
        zip.write_all(readme.as_bytes())?;

        zip.finish()
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;

        Ok(())
    }

    /// Collect and prepare all data, returning manifest + content
    fn prepare(&self) -> Result<(SnapshotManifest, Vec<Memory>, Vec<Entity>, Vec<SnapshotEdge>)> {
        let memories = self.query_memories()?;
        let memory_ids: Vec<i64> = memories.iter().map(|m| m.id).collect();

        let entities = self.query_entities(&memory_ids)?;
        let raw_edges = self.query_edges(&memory_ids)?;
        let edges: Vec<SnapshotEdge> = raw_edges.iter().map(SnapshotEdge::from).collect();

        let content_hash = Self::compute_content_hash(&memories);

        let manifest = SnapshotManifest {
            format_version: "1.0".to_string(),
            engram_version: crate::VERSION.to_string(),
            min_engram_version: "0.12.0".to_string(),
            schema_version: 32,
            creator: self.creator.clone(),
            description: self.description.clone(),
            created_at: Utc::now(),
            content_hash,
            memory_count: memories.len(),
            entity_count: entities.len(),
            edge_count: edges.len(),
            embedding_model: None,
            embedding_dimensions: None,
            encrypted: false,
            signed: false,
        };

        Ok((manifest, memories, entities, edges))
    }

    // -------------------------------------------------------------------------
    // Public build methods
    // -------------------------------------------------------------------------

    /// Build the snapshot archive and write it to `output_path`.
    pub fn build(&self, output_path: &Path) -> Result<SnapshotManifest> {
        let (manifest, memories, entities, edges) = self.prepare()?;
        Self::write_archive(output_path, &manifest, &memories, &entities, &edges)?;
        Ok(manifest)
    }

    /// Build the snapshot archive, then encrypt all non-manifest content.
    ///
    /// The manifest is stored in plaintext so callers can inspect metadata
    /// without the key.  The encrypted payload is stored as `payload.enc`
    /// inside the outer archive.
    pub fn build_encrypted(&self, output_path: &Path, key: &[u8; 32]) -> Result<SnapshotManifest> {
        let (mut manifest, memories, entities, edges) = self.prepare()?;
        manifest.encrypted = true;

        // Build an inner archive in memory
        let mut inner_buf: Vec<u8> = Vec::new();
        {
            let mut inner_zip = zip::ZipWriter::new(std::io::Cursor::new(&mut inner_buf));
            let opts = zip::write::FileOptions::<()>::default()
                .compression_method(zip::CompressionMethod::Deflated);

            inner_zip.start_file("memories.json", opts)
                .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
            inner_zip.write_all(serde_json::to_string_pretty(&memories)?.as_bytes())?;

            inner_zip.start_file("entities.json", opts)
                .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
            inner_zip.write_all(serde_json::to_string_pretty(&entities)?.as_bytes())?;

            inner_zip.start_file("graph_edges.json", opts)
                .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
            inner_zip.write_all(serde_json::to_string_pretty(&edges)?.as_bytes())?;

            inner_zip.finish()
                .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        }

        let encrypted = encrypt_aes256(&inner_buf, key)?;

        // Outer archive: manifest + encrypted payload + README
        let file = std::fs::File::create(output_path)?;
        let mut outer_zip = zip::ZipWriter::new(file);
        let opts = zip::write::FileOptions::<()>::default()
            .compression_method(zip::CompressionMethod::Deflated);

        outer_zip.start_file("manifest.json", opts)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        outer_zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;

        outer_zip.start_file("payload.enc", opts)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        outer_zip.write_all(&encrypted)?;

        outer_zip.start_file("README.md", opts)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        outer_zip.write_all(Self::generate_readme(&manifest).as_bytes())?;

        outer_zip.finish()
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;

        Ok(manifest)
    }

    /// Build the snapshot archive and sign the manifest with Ed25519.
    ///
    /// The signature is stored as `manifest.sig` (hex-encoded) alongside
    /// the manifest in the archive.
    pub fn build_signed(&self, output_path: &Path, secret_key: &[u8; 32]) -> Result<SnapshotManifest> {
        let (mut manifest, memories, entities, edges) = self.prepare()?;
        manifest.signed = true;

        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        let sig_bytes = sign_ed25519(manifest_json.as_bytes(), secret_key)?;
        let sig_hex = hex::encode(&sig_bytes);

        let file = std::fs::File::create(output_path)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::FileOptions::<()>::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("manifest.json", opts)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        zip.write_all(manifest_json.as_bytes())?;

        zip.start_file("manifest.sig", opts)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        zip.write_all(sig_hex.as_bytes())?;

        zip.start_file("memories.json", opts)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        zip.write_all(serde_json::to_string_pretty(&memories)?.as_bytes())?;

        zip.start_file("entities.json", opts)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        zip.write_all(serde_json::to_string_pretty(&entities)?.as_bytes())?;

        zip.start_file("graph_edges.json", opts)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        zip.write_all(serde_json::to_string_pretty(&edges)?.as_bytes())?;

        zip.start_file("README.md", opts)
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;
        zip.write_all(Self::generate_readme(&manifest).as_bytes())?;

        zip.finish()
            .map_err(|e| EngramError::Io(std::io::Error::other(e.to_string())))?;

        Ok(manifest)
    }
}
