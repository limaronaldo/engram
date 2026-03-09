//! Snapshot type definitions for .egm portable knowledge packages

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Manifest embedded in every .egm snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    /// Format version of this manifest (currently "1.0")
    pub format_version: String,
    /// Engram version that created this snapshot
    pub engram_version: String,
    /// Minimum Engram version required to load this snapshot
    pub min_engram_version: String,
    /// Database schema version at creation time
    pub schema_version: i32,
    /// Agent or user name that created this snapshot
    pub creator: Option<String>,
    /// Human-readable description of this snapshot
    pub description: Option<String>,
    /// When this snapshot was created
    pub created_at: DateTime<Utc>,
    /// SHA-256 of all memory contents concatenated (hex-encoded)
    pub content_hash: String,
    /// Number of memories in this snapshot
    pub memory_count: usize,
    /// Number of entities in this snapshot
    pub entity_count: usize,
    /// Number of graph edges in this snapshot
    pub edge_count: usize,
    /// Embedding model used to compute vectors (if any)
    pub embedding_model: Option<String>,
    /// Embedding vector dimensions (if any)
    pub embedding_dimensions: Option<usize>,
    /// Whether the archive content is AES-256-GCM encrypted
    pub encrypted: bool,
    /// Whether the manifest has an Ed25519 signature
    pub signed: bool,
}

/// Strategy for loading a snapshot into storage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadStrategy {
    /// Skip memories that already exist (by content hash) — additive load
    Merge,
    /// Clear the target workspace first, then load all memories
    Replace,
    /// Load into a new auto-named workspace to avoid any conflicts
    Isolate,
    /// Report what would happen without making any changes
    DryRun,
}

/// Result of inspecting a snapshot file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInfo {
    /// Parsed manifest from the snapshot
    pub manifest: SnapshotManifest,
    /// Size of the .egm file on disk in bytes
    pub file_size_bytes: u64,
    /// List of files inside the archive
    pub files: Vec<String>,
}

/// Result of loading a snapshot into storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadResult {
    /// Strategy that was used for this load
    pub strategy: LoadStrategy,
    /// Number of memories actually inserted
    pub memories_loaded: usize,
    /// Number of memories skipped (duplicates in Merge mode)
    pub memories_skipped: usize,
    /// Number of entities inserted
    pub entities_loaded: usize,
    /// Number of graph edges inserted
    pub edges_loaded: usize,
    /// Name of the workspace where memories were loaded
    pub target_workspace: String,
    /// Filename of the .egm file that was loaded
    pub snapshot_origin: String,
}

impl std::fmt::Display for LoadStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Merge => write!(f, "merge"),
            Self::Replace => write!(f, "replace"),
            Self::Isolate => write!(f, "isolate"),
            Self::DryRun => write!(f, "dry_run"),
        }
    }
}

impl std::str::FromStr for LoadStrategy {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "merge" => Ok(Self::Merge),
            "replace" => Ok(Self::Replace),
            "isolate" => Ok(Self::Isolate),
            "dry_run" | "dryrun" | "dry-run" => Ok(Self::DryRun),
            _ => Err(format!("Unknown load strategy: {}", s)),
        }
    }
}
