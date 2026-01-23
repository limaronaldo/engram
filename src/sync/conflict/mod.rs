//! Conflict Resolution with Three-Way Merge (RML-887)
//!
//! Provides:
//! - Automatic conflict detection during sync
//! - Three-way merge for text content
//! - Conflict resolution strategies
//! - Manual conflict review queue

mod detector;
mod merge;
mod resolver;

pub use detector::{ConflictDetector, ConflictInfo, ConflictType};
pub use merge::{MergeResult, ThreeWayMerge};
pub use resolver::{ConflictQueue, ConflictResolver, Resolution, ResolutionStrategy};

use crate::types::Memory;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A version of a memory for conflict resolution (different from types::MemoryVersion)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMemoryVersion {
    /// The memory content
    pub memory: Memory,
    /// Version identifier (e.g., device ID + timestamp)
    pub version_id: String,
    /// When this version was created
    pub created_at: DateTime<Utc>,
    /// Device/client that created this version
    pub source: String,
    /// Hash of the content for quick comparison
    pub content_hash: String,
}

impl SyncMemoryVersion {
    /// Create a new memory version
    pub fn new(memory: Memory, source: impl Into<String>) -> Self {
        let source_str = source.into();
        let content_hash = Self::compute_hash(&memory);
        Self {
            memory,
            version_id: format!("{}_{}", source_str, Utc::now().timestamp_millis()),
            created_at: Utc::now(),
            source: source_str,
            content_hash,
        }
    }

    /// Compute hash of memory content
    fn compute_hash(memory: &Memory) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(memory.content.as_bytes());
        hasher.update(
            serde_json::to_string(&memory.metadata)
                .unwrap_or_default()
                .as_bytes(),
        );
        hex::encode(hasher.finalize())[..16].to_string()
    }

    /// Check if this version has the same content as another
    pub fn has_same_content(&self, other: &SyncMemoryVersion) -> bool {
        self.content_hash == other.content_hash
    }
}

/// Represents a conflict between versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// Unique conflict identifier
    pub id: String,
    /// Memory ID that has conflict
    pub memory_id: i64,
    /// The base version (common ancestor)
    pub base: Option<SyncMemoryVersion>,
    /// Local version
    pub local: SyncMemoryVersion,
    /// Remote version
    pub remote: SyncMemoryVersion,
    /// Type of conflict
    pub conflict_type: ConflictType,
    /// When the conflict was detected
    pub detected_at: DateTime<Utc>,
    /// Whether this has been resolved
    pub resolved: bool,
    /// Resolution if resolved
    pub resolution: Option<Resolution>,
}

impl Conflict {
    /// Create a new conflict
    pub fn new(
        memory_id: i64,
        base: Option<SyncMemoryVersion>,
        local: SyncMemoryVersion,
        remote: SyncMemoryVersion,
        conflict_type: ConflictType,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            memory_id,
            base,
            local,
            remote,
            conflict_type,
            detected_at: Utc::now(),
            resolved: false,
            resolution: None,
        }
    }

    /// Check if auto-resolution is possible
    pub fn can_auto_resolve(&self) -> bool {
        matches!(
            self.conflict_type,
            ConflictType::MetadataOnly | ConflictType::TagsOnly | ConflictType::NonOverlapping
        )
    }

    /// Mark as resolved
    pub fn resolve(&mut self, resolution: Resolution) {
        self.resolved = true;
        self.resolution = Some(resolution);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MemoryType;
    use std::collections::HashMap;

    fn create_test_memory(content: &str) -> Memory {
        Memory {
            id: 1,
            content: content.to_string(),
            memory_type: MemoryType::Note,
            tags: vec!["test".to_string()],
            metadata: HashMap::new(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_accessed_at: None,
            owner_id: None,
            visibility: crate::types::Visibility::Private,
            version: 1,
            has_embedding: false,
        }
    }

    #[test]
    fn test_memory_version_hash() {
        let memory = create_test_memory("Test content");
        let v1 = SyncMemoryVersion::new(memory.clone(), "device1");
        let v2 = SyncMemoryVersion::new(memory, "device2");

        // Same content should have same hash
        assert!(v1.has_same_content(&v2));
    }

    #[test]
    fn test_different_content_different_hash() {
        let m1 = create_test_memory("Content A");
        let m2 = create_test_memory("Content B");

        let v1 = SyncMemoryVersion::new(m1, "device1");
        let v2 = SyncMemoryVersion::new(m2, "device1");

        assert!(!v1.has_same_content(&v2));
    }

    #[test]
    fn test_conflict_creation() {
        let base = SyncMemoryVersion::new(create_test_memory("Original"), "base");
        let local = SyncMemoryVersion::new(create_test_memory("Local change"), "local");
        let remote = SyncMemoryVersion::new(create_test_memory("Remote change"), "remote");

        let conflict = Conflict::new(1, Some(base), local, remote, ConflictType::ContentConflict);

        assert!(!conflict.resolved);
        assert!(conflict.resolution.is_none());
        assert!(!conflict.can_auto_resolve());
    }

    #[test]
    fn test_auto_resolvable_conflict() {
        let local = SyncMemoryVersion::new(create_test_memory("Same"), "local");
        let remote = SyncMemoryVersion::new(create_test_memory("Same"), "remote");

        let conflict = Conflict::new(1, None, local, remote, ConflictType::MetadataOnly);

        assert!(conflict.can_auto_resolve());
    }
}
