//! Conflict detection logic

use super::SyncMemoryVersion;
use serde::{Deserialize, Serialize};

/// Types of conflicts that can occur
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictType {
    /// Both sides modified content in overlapping ways
    ContentConflict,
    /// Only metadata differs (can auto-merge)
    MetadataOnly,
    /// Only tags differ (can auto-merge by union)
    TagsOnly,
    /// Both modified but in non-overlapping sections
    NonOverlapping,
    /// One side deleted, other modified
    DeleteModify,
    /// Both sides created same memory ID (rare)
    CreateCreate,
}

/// Information about a detected conflict
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictInfo {
    /// Type of conflict
    pub conflict_type: ConflictType,
    /// Severity (0-100, higher = more severe)
    pub severity: u8,
    /// Human-readable description
    pub description: String,
    /// Suggested resolution strategy
    pub suggested_strategy: String,
}

/// Conflict detector
pub struct ConflictDetector {
    /// Threshold for considering changes as overlapping (0.0 - 1.0)
    overlap_threshold: f32,
}

impl Default for ConflictDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ConflictDetector {
    /// Create a new conflict detector
    pub fn new() -> Self {
        Self {
            overlap_threshold: 0.3,
        }
    }

    /// Set overlap threshold
    pub fn with_overlap_threshold(mut self, threshold: f32) -> Self {
        self.overlap_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Detect if there's a conflict between local and remote versions
    pub fn detect(
        &self,
        base: Option<&SyncMemoryVersion>,
        local: &SyncMemoryVersion,
        remote: &SyncMemoryVersion,
    ) -> Option<ConflictInfo> {
        // If content is identical, no conflict
        if local.has_same_content(remote) {
            return None;
        }

        // Analyze the type of differences
        let content_differs = local.memory.content != remote.memory.content;
        let tags_differ = local.memory.tags != remote.memory.tags;
        let metadata_differs = local.memory.metadata != remote.memory.metadata;

        // Determine conflict type
        let conflict_type = if !content_differs && !tags_differ && metadata_differs {
            ConflictType::MetadataOnly
        } else if !content_differs && tags_differ && !metadata_differs {
            ConflictType::TagsOnly
        } else if content_differs {
            // Check if changes are overlapping
            if let Some(base) = base {
                if self.are_changes_overlapping(
                    &base.memory.content,
                    &local.memory.content,
                    &remote.memory.content,
                ) {
                    ConflictType::ContentConflict
                } else {
                    ConflictType::NonOverlapping
                }
            } else {
                // No base, assume conflict
                ConflictType::ContentConflict
            }
        } else {
            // Tags and metadata both differ
            ConflictType::MetadataOnly
        };

        let (severity, description, suggested_strategy) = match conflict_type {
            ConflictType::ContentConflict => (
                80,
                "Both local and remote modified the content in overlapping sections".to_string(),
                "manual_review".to_string(),
            ),
            ConflictType::MetadataOnly => (
                20,
                "Only metadata differs between versions".to_string(),
                "merge_metadata".to_string(),
            ),
            ConflictType::TagsOnly => (
                10,
                "Only tags differ between versions".to_string(),
                "union_tags".to_string(),
            ),
            ConflictType::NonOverlapping => (
                40,
                "Changes are in different sections and can be merged".to_string(),
                "three_way_merge".to_string(),
            ),
            ConflictType::DeleteModify => (
                90,
                "One side deleted the memory while the other modified it".to_string(),
                "manual_review".to_string(),
            ),
            ConflictType::CreateCreate => (
                70,
                "Both sides created a memory with the same ID".to_string(),
                "keep_both".to_string(),
            ),
        };

        Some(ConflictInfo {
            conflict_type,
            severity,
            description,
            suggested_strategy,
        })
    }

    /// Check if changes between base->local and base->remote overlap
    fn are_changes_overlapping(&self, base: &str, local: &str, remote: &str) -> bool {
        let base_lines: Vec<&str> = base.lines().collect();
        let local_lines: Vec<&str> = local.lines().collect();
        let remote_lines: Vec<&str> = remote.lines().collect();

        // Find lines changed in local
        let local_changes: Vec<usize> = local_lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                if i >= base_lines.len() || base_lines[i] != *line {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        // Find lines changed in remote
        let remote_changes: Vec<usize> = remote_lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                if i >= base_lines.len() || base_lines[i] != *line {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        // Check for overlapping line numbers
        let overlap_count = local_changes
            .iter()
            .filter(|l| remote_changes.contains(l))
            .count();

        let total_changes = local_changes.len().max(remote_changes.len());
        if total_changes == 0 {
            return false;
        }

        (overlap_count as f32 / total_changes as f32) > self.overlap_threshold
    }

    /// Detect delete-modify conflict
    pub fn detect_delete_modify(
        &self,
        local_exists: bool,
        remote_exists: bool,
        local_modified: bool,
        remote_modified: bool,
    ) -> Option<ConflictType> {
        match (local_exists, remote_exists, local_modified, remote_modified) {
            (false, true, _, true) => Some(ConflictType::DeleteModify),
            (true, false, true, _) => Some(ConflictType::DeleteModify),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Memory, MemoryType, Visibility};
    use chrono::Utc;
    use std::collections::HashMap;

    fn create_memory(content: &str) -> Memory {
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
            visibility: Visibility::Private,
            scope: crate::types::MemoryScope::Global,
            workspace: "default".to_string(),
            tier: crate::types::MemoryTier::Permanent,
            version: 1,
            has_embedding: false,
            expires_at: None,
            content_hash: None,
        }
    }

    fn create_version(content: &str, source: &str) -> SyncMemoryVersion {
        SyncMemoryVersion::new(create_memory(content), source)
    }

    #[test]
    fn test_no_conflict_same_content() {
        let detector = ConflictDetector::new();
        let local = create_version("Same content", "local");
        let remote = create_version("Same content", "remote");

        let conflict = detector.detect(None, &local, &remote);
        assert!(conflict.is_none());
    }

    #[test]
    fn test_content_conflict() {
        let detector = ConflictDetector::new();
        let base = create_version("Original line 1\nOriginal line 2", "base");
        let local = create_version("Modified line 1\nOriginal line 2", "local");
        let remote = create_version("Different line 1\nOriginal line 2", "remote");

        let conflict = detector.detect(Some(&base), &local, &remote);
        assert!(conflict.is_some());
        let info = conflict.unwrap();
        assert_eq!(info.conflict_type, ConflictType::ContentConflict);
        assert!(info.severity >= 70);
    }

    #[test]
    fn test_non_overlapping_changes() {
        let detector = ConflictDetector::new();
        let base = create_version("Line 1\nLine 2\nLine 3\nLine 4", "base");
        let local = create_version("Modified 1\nLine 2\nLine 3\nLine 4", "local");
        let remote = create_version("Line 1\nLine 2\nLine 3\nModified 4", "remote");

        let conflict = detector.detect(Some(&base), &local, &remote);
        assert!(conflict.is_some());
        let info = conflict.unwrap();
        assert_eq!(info.conflict_type, ConflictType::NonOverlapping);
    }

    #[test]
    fn test_metadata_only_conflict() {
        let detector = ConflictDetector::new();

        let mut local_mem = create_memory("Same content");
        local_mem
            .metadata
            .insert("key".to_string(), serde_json::json!("local_value"));

        let mut remote_mem = create_memory("Same content");
        remote_mem
            .metadata
            .insert("key".to_string(), serde_json::json!("remote_value"));

        let local = SyncMemoryVersion::new(local_mem, "local");
        let remote = SyncMemoryVersion::new(remote_mem, "remote");

        let conflict = detector.detect(None, &local, &remote);
        assert!(conflict.is_some());
        let info = conflict.unwrap();
        assert_eq!(info.conflict_type, ConflictType::MetadataOnly);
        assert!(info.severity < 50);
    }

    #[test]
    fn test_delete_modify_detection() {
        let detector = ConflictDetector::new();

        // Local deleted, remote modified
        let result = detector.detect_delete_modify(false, true, false, true);
        assert_eq!(result, Some(ConflictType::DeleteModify));

        // Local modified, remote deleted
        let result = detector.detect_delete_modify(true, false, true, false);
        assert_eq!(result, Some(ConflictType::DeleteModify));

        // Both exist, no delete-modify conflict
        let result = detector.detect_delete_modify(true, true, true, true);
        assert!(result.is_none());
    }
}
